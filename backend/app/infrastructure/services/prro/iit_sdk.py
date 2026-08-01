"""
Інтеграція з крипто-ядром ІІТ (SDK EUSignCP, lib euscp.so) через ctypes.

Призначення: підпис ДСТУ 4145-2002 + ДСТУ 7564:2014 (Стрибог-256) для
фіскального сервера ДПС (cabinet.tax.gov.ua). Це єдине крипто-ядро, яке
вміє читати контейнери ІІТ «ЦСК-1» (Key-6.dat) та JKS-ключі ДСТУ 4145
і формувати CAdES-підпис у форматі, який розуміє сервер ДПС
(`ee.SignInternal(true, xml)` з офіційного семпла programika/prro_sample).

Бібліотека:
  - завантажується з `backend/vendor/iit-sdk/opt/iit/eu/sw/euscp.so`;
  - SDK встановлюється скриптом `scripts/setup_iit_sdk.sh` (завантаження
    `euswi.64.deb` з https://iit.com.ua/download/productfiles/euswi.64.deb
    та розпакування без root);
  - файл налаштувань `osplm.ini` та сертифікатне сховище — там само.

Використання:
    sdk = IitSdk.get()
    sdk.load_jks_key("certs/...jks", "password")   # один раз
    signature = sdk.sign_data_internal(xml_bytes)  # CAdES-BES bytes
    ok = sdk.verify_data_internal(signature, xml_bytes)
    serial = sdk.get_signer_serial()
    name = sdk.get_signer_name()
"""

from __future__ import annotations

import base64
import ctypes
import logging
from pathlib import Path
from typing import Any

from cryptography import x509

logger = logging.getLogger(__name__)


class IitSdkError(Exception):
    """Помилка роботи з крипто-ядром ІІТ (SDK EUSignCP)."""


# Шлях до SDK відносно backend/ (vendor у .gitignore, встановлюється скриптом)
_BACKEND_DIR = Path(__file__).resolve().parents[4]
_VENDOR_SDK_DIR = _BACKEND_DIR / "vendor" / "iit-sdk" / "opt" / "iit" / "eu" / "sw"
_DEFAULT_CERT_STORE = _BACKEND_DIR / "vendor" / "iit-sdk" / "certs"


class IitSdk:
    """
    ctypes-обгортка над бібліотекою EUSignCP (euscp.so).

    Реалізує мінімально необхідний набір функцій для ПРРО:
      - завантаження JKS-ключа ДСТУ 4145 (EUGetJKSPrivateKeyFile);
      - збереження сертифікатів у файлове сховище (EUSaveCertificate);
      - читання ключа (EUReadPrivateKeyBinary);
      - підпис CAdES-BES (EUSignDataInternal) — формат офіційного семпла
        programika/prro_sample (`ee.SignInternal(true, data)`);
      - перевірка підпису (EUVerifyDataInternal).
    """

    _instance: "IitSdk | None" = None

    def __init__(self, lib_path: Path | None = None, cert_store: Path | None = None) -> None:
        self._lib: Any = None
        self._lib_path = Path(lib_path or _VENDOR_SDK_DIR / "euscp.so")
        self._cert_store = Path(cert_store or _DEFAULT_CERT_STORE)
        self._initialized = False
        self._key_loaded = False
        self._signer_cert: x509.Certificate | None = None

    # ─── Singleton ─────────────────────────────────────────────────────────

    @classmethod
    def get(cls) -> "IitSdk":
        """Повертає спільний екземпляр SDK (ініціалізується ліниво)."""
        if cls._instance is None:
            cls._instance = cls()
        return cls._instance

    @classmethod
    def reset(cls) -> None:
        """Скидає singleton (для тестів)."""
        cls._instance = None

    # ─── Завантаження та ініціалізація ────────────────────────────────────

    @property
    def available(self) -> bool:
        """Чи доступна бібліотека SDK (файл існує)."""
        return self._lib_path.is_file()

    @property
    def key_loaded(self) -> bool:
        """Чи завантажено ключ у крипто-ядро."""
        return self._key_loaded

    def _ensure_library(self) -> Any:
        """Завантажує euscp.so (один раз)."""
        if self._lib is not None:
            return self._lib
        if not self._lib_path.is_file():
            raise IitSdkError(
                "Крипто-ядро ІІТ (SDK EUSignCP) не знайдено: "
                f"{self._lib_path}. Встановіть його скриптом "
                "backend/scripts/setup_iit_sdk.sh"
            )
        try:
            self._lib = ctypes.CDLL(str(self._lib_path))
        except OSError as exc:
            raise IitSdkError(
                f"Не вдалося завантажити {self._lib_path}: {exc}"
            ) from exc
        logger.info("PRRO_IIT_SDK | бібліотеку завантажено: %s", self._lib_path)
        return self._lib

    def _init(self) -> Any:
        """Ініціалізує SDK (settings + EUInitialize), якщо ще не ініціалізовано."""
        lib = self._ensure_library()
        if self._initialized:
            return lib

        self._cert_store.mkdir(parents=True, exist_ok=True)
        osplm = _VENDOR_SDK_DIR / "osplm.ini"
        if osplm.is_file():
            self._call_simple("EUSetSettingsFilePath", str(osplm))

        # файлове сховище сертифікатів
        self._call_typed(
            "EUSetFileStoreSettings",
            [ctypes.c_char_p] + [ctypes.c_int] * 6 + [ctypes.c_ulong],
            str(self._cert_store).encode(),
            0, 0, 0, 0, 0, 0,
            3600,
        )

        rc = self._call_int("EUInitialize")
        if rc != 0:
            raise IitSdkError(f"EUInitialize: {self._error_text(rc)}")
        self._initialized = True
        logger.info("PRRO_IIT_SDK | SDK ініціалізовано (store=%s)", self._cert_store)
        return lib

    # ─── Низькорівневі виклики ────────────────────────────────────────────

    @staticmethod
    def _cast_arg(value: Any) -> Any:
        """Перетворює Python-значення у ctypes-аргумент (char*/int)."""
        if isinstance(value, str):
            return value.encode("utf-8")
        if isinstance(value, bytes):
            return value
        if isinstance(value, bool):
            return ctypes.c_int(int(value))
        return value

    def _call_typed(
        self, name: str, argtypes: list[Any], *args: Any
    ) -> int:
        """Викликає функцію SDK з явними типами аргументів."""
        lib = self._ensure_library()
        fn = getattr(lib, name, None)
        if fn is None:
            raise IitSdkError(f"Функція {name} не знайдена в euscp.so")
        fn.argtypes = argtypes
        fn.restype = ctypes.c_ulong
        return int(fn(*args))

    def _call_simple(self, name: str, *args: Any) -> int:
        """Викликає функцію SDK з аргументами char*/int за замовчуванням."""
        lib = self._ensure_library()
        fn = getattr(lib, name, None)
        if fn is None:
            raise IitSdkError(f"Функція {name} не знайдена в euscp.so")
        fn.restype = ctypes.c_ulong
        converted = [self._cast_arg(a) for a in args]
        return int(fn(*converted))

    def _call_int(self, name: str) -> int:
        lib = self._ensure_library()
        fn = getattr(lib, name, None)
        if fn is None:
            raise IitSdkError(f"Функція {name} не знайдена в euscp.so")
        fn.restype = ctypes.c_int
        return int(fn())

    def _error_text(self, code: int) -> str:
        """Людиночитабельний текст помилки SDK."""
        lib = self._ensure_library()
        fn = getattr(lib, "EUGetErrorDesc", None)
        if fn is None:
            return f"код {code}"
        fn.argtypes = [ctypes.c_int]
        fn.restype = ctypes.c_char_p
        desc = fn(code)
        if not desc:
            return f"код {code}"
        try:
            return desc.decode("cp1251", errors="replace")
        except Exception:  # noqa: BLE001
            return desc.decode("utf-8", errors="replace")

    def _free(self, ptr: Any) -> None:
        """Звільняє пам'ять, виділену SDK."""
        if not ptr:
            return
        lib = self._ensure_library()
        fn = getattr(lib, "EUFreeMemory", None)
        if fn is not None:
            fn.argtypes = [ctypes.c_void_p]
            fn(ptr)

    # ─── Завантаження ключа ───────────────────────────────────────────────

    def load_jks_key(self, jks_path: str | Path, password: str) -> None:
        """
        Завантажує JKS-ключ (ДСТУ 4145) у крипто-ядро.

        Кроки (відтворюють офіційний семпл ДПС):
          1. EUGetJKSPrivateKeyFile — дістати ключ + ланцюг сертифікатів;
          2. EUSaveCertificate — зберегти сертифікати у файлове сховище
             (SDK шукає сертифікат підписанта за issuer+serial ключа);
          3. EUReadPrivateKeyBinary — завантажити ключ у ядро.

        Args:
            jks_path: шлях до JKS-файлу.
            password: пароль сховища/ключа.
        """
        lib = self._init()
        jks_path = str(Path(jks_path).resolve())
        if not Path(jks_path).is_file():
            raise IitSdkError(f"Файл JKS не знайдено: {jks_path}")

        # 1) ключ + сертифікати з JKS
        key_ptr = ctypes.c_void_p()
        key_len = ctypes.c_ulong()
        cert_cnt = ctypes.c_ulong()
        certs_ptr = ctypes.POINTER(ctypes.c_void_p)()
        cert_lens_ptr = ctypes.POINTER(ctypes.c_ulong)()

        fn = getattr(lib, "EUGetJKSPrivateKeyFile")
        fn.argtypes = [
            ctypes.c_char_p,
            ctypes.c_char_p,
            ctypes.POINTER(ctypes.c_void_p),
            ctypes.POINTER(ctypes.c_ulong),
            ctypes.POINTER(ctypes.c_ulong),
            ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p)),
            ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)),
        ]
        fn.restype = ctypes.c_ulong
        rc = fn(
            jks_path.encode(),
            None,  # alias — перший ключ
            ctypes.byref(key_ptr),
            ctypes.byref(key_len),
            ctypes.byref(cert_cnt),
            ctypes.byref(certs_ptr),
            ctypes.byref(cert_lens_ptr),
        )
        if rc != 0:
            raise IitSdkError(
                f"Не вдалося прочитати JKS ({Path(jks_path).name}): "
                f"{self._error_text(rc)}"
            )

        try:
            key_bytes = ctypes.string_at(key_ptr.value, key_len.value)
            certs: list[bytes] = []
            for i in range(cert_cnt.value):
                cert_ptr = certs_ptr[i]
                cert_len = cert_lens_ptr[i]
                certs.append(ctypes.string_at(cert_ptr, cert_len))
        finally:
            self._free(key_ptr)
            if certs_ptr:
                for i in range(cert_cnt.value):
                    self._free(certs_ptr[i])
                lib.EUFreeMemory(ctypes.cast(certs_ptr, ctypes.c_void_p))
            if cert_lens_ptr:
                lib.EUFreeMemory(ctypes.cast(cert_lens_ptr, ctypes.c_void_p))

        # 2) сертифікати у файлове сховище
        save = getattr(lib, "EUSaveCertificate")
        save.argtypes = [ctypes.c_char_p, ctypes.c_ulong]
        save.restype = ctypes.c_ulong
        for cert in certs:
            rc = save(cert, len(cert))
            if rc != 0:
                logger.warning(
                    "PRRO_IIT_SDK | EUSaveCertificate: %s",
                    self._error_text(rc),
                )

        # 3) ключ у ядро
        read = getattr(lib, "EUReadPrivateKeyBinary")
        read.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p]
        read.restype = ctypes.c_int
        rc = read(key_bytes, len(key_bytes), password.encode("utf-8"))
        if rc != 0:
            raise IitSdkError(
                f"Не вдалося завантажити ключ у крипто-ядро ІІТ: "
                f"{self._error_text(rc)}"
            )
        self._key_loaded = True
        self._signer_cert = self._find_signer_cert(certs)
        logger.info(
            "PRRO_IIT_SDK | ключ JKS завантажено: %s (cert=%s)",
            Path(jks_path).name,
            self._signer_cert.subject.rfc4514_string() if self._signer_cert else "?",
        )

    @staticmethod
    def _find_signer_cert(certs: list[bytes]) -> x509.Certificate | None:
        """
        Знаходить сертифікат підписанта в ланцюгу JKS.

        У Java KeyStore ланцюг може йти в будь-якому порядку. Сертифікат
        підписанта — той, який НЕ самопідписаний і не є проміжним ЦСК
        (його subject не містить ознак ЦСК: CN != «Засвідчувальний центр»
        та issuer != subject).
        """
        parsed: list[x509.Certificate] = []
        for c in certs:
            try:
                parsed.append(x509.load_der_x509_certificate(c))
            except Exception:  # noqa: BLE001
                continue
        # Сертифікат підписанта — кінцевий (не ЦСК): BasicConstraints
        # відсутній або CA=False, subject != issuer.
        for cert in parsed:
            if cert.subject == cert.issuer:
                continue  # кореневий ЦСК
            try:
                bc = cert.extensions.get_extension_for_oid(
                    x509.ExtensionOID.BASIC_CONSTRAINTS
                ).value
                if bc.ca:
                    continue  # проміжний ЦСК
            except x509.ExtensionNotFound:
                pass  # кінцевий сертифікат без BasicConstraints
            return cert
        return parsed[0] if parsed else None

    # ─── Підписання ────────────────────────────────────────────────────────

    def sign_data_internal(self, data: bytes) -> bytes:
        """
        Формує CAdES-BES підпис (ДСТУ 4145-2002 + Стрибог-256).

        Відповідає `ee.SignInternal(true, data)` з офіційного семпла ДПС:
        повертає DER-підпис ContentInfo(signedData) з сертифікатом
        підписанта. Саме ці байти ДПС очікує в `Check.check_sign`.

        Args:
            data: дані для підпису (XML-документ СЗЗД, windows-1251).

        Returns:
            bytes — бінарний CAdES-BES підпис.

        Raises:
            IitSdkError: якщо ключ не завантажено або підпис не вдався.
        """
        if not self._key_loaded:
            raise IitSdkError(
                "Ключ не завантажено в крипто-ядро ІІТ. Спочатку "
                "викличте load_jks_key()."
            )
        lib = self._init()
        fn = getattr(lib, "EUSignDataInternal")
        fn.argtypes = [
            ctypes.c_int,                      # bAppendCert
            ctypes.c_char_p,                   # pbData
            ctypes.c_ulong,                    # dwDataLength
            ctypes.POINTER(ctypes.c_char_p),   # ppszSignedData (base64)
            ctypes.POINTER(ctypes.c_void_p),   # ppbSignedData
            ctypes.POINTER(ctypes.c_ulong),    # pdwSignedDataLength
        ]
        fn.restype = ctypes.c_ulong

        b64_out = ctypes.c_char_p()
        sign_ptr = ctypes.c_void_p()
        sign_len = ctypes.c_ulong()
        rc = fn(
            1,  # включати сертифікат підписанта
            data,
            len(data),
            ctypes.byref(b64_out),
            ctypes.byref(sign_ptr),
            ctypes.byref(sign_len),
        )
        if rc != 0:
            raise IitSdkError(
                f"Помилка формування підпису ДСТУ 4145: {self._error_text(rc)}"
            )

        # SDK повертає підпис як base64-строку (як Java ee.SignInternal)
        if b64_out.value:
            return base64.b64decode(b64_out.value)
        if sign_ptr.value and sign_len.value:
            sig = ctypes.string_at(sign_ptr.value, sign_len.value)
            self._free(sign_ptr)
            return sig
        raise IitSdkError("SDK повернув порожній підпис")

    def verify_data_internal(
        self, signature: bytes, expected_data: bytes | None = None
    ) -> bool:
        """
        Перевіряє CAdES-BES підпис (EUVerifyDataInternal).

        Args:
            signature: бінарний підпис (результат sign_data_internal).
            expected_data: очікувані дані (XML). Якщо None — перевіряється
                лише валідність підпису (дані витягуються з підпису).

        Returns:
            bool — True, якщо підпис валідний і (якщо задано) відповідає даним.
        """
        lib = self._init()
        fn = getattr(lib, "EUVerifyDataInternal")
        fn.argtypes = [
            ctypes.c_char_p,                  # pszSignedData (base64) — NULL
            ctypes.c_char_p,                  # pbSignedData
            ctypes.c_ulong,                   # dwSignedDataLength
            ctypes.POINTER(ctypes.c_void_p),  # ppbData
            ctypes.POINTER(ctypes.c_ulong),   # pdwDataLength
            ctypes.c_void_p,                  # pSignInfo — NULL
        ]
        fn.restype = ctypes.c_ulong

        data_out = ctypes.c_void_p()
        data_len = ctypes.c_ulong()
        rc = fn(
            None,
            signature,
            len(signature),
            ctypes.byref(data_out),
            ctypes.byref(data_len),
            None,
        )
        if rc != 0:
            logger.warning(
                "PRRO_IIT_SDK | verify: %s", self._error_text(rc)
            )
            return False
        try:
            verified = ctypes.string_at(data_out.value, data_len.value)
            if expected_data is None:
                return True  # підпис валідний, дані витягнуто
            return verified == expected_data
        finally:
            self._free(data_out)

    # ─── Дані підписанта ──────────────────────────────────────────────────

    def get_signer_serial(self) -> str:
        """Серійний номер сертифіката підписанта (hex, upper)."""
        if self._signer_cert is None:
            raise IitSdkError("Сертифікат підписанта не визначено")
        return format(self._signer_cert.serial_number, "X")

    def get_signer_name(self) -> str:
        """ПІБ підписанта з сертифіката."""
        if self._signer_cert is None:
            return ""
        cn = self._signer_cert.subject.get_attributes_for_oid(
            x509.NameOID.COMMON_NAME
        )
        if cn:
            return str(cn[0].value)
        return ""


__all__ = ["IitSdk", "IitSdkError"]
