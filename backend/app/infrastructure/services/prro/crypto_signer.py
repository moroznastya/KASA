"""
Підписання XML-документів ПРРО (XAdES) з підтримкою різних форматів ключів.

Підтримувані формати ключа (рішення користувача):
  - `pfx` / `p12` — PKCS#12 (cryptography.load_pkcs12);
  - `jks`         — Java KeyStore (бібліотека pyjks, модуль `jks`),
                    конвертація приватного ключа та сертифіката в PEM;
  - `pem`         — приватний ключ + сертифікат (окремі файли або об'єднані
                    в один файл PEM);
  - `dat`         — ІІТ «ЦСК-1» (Key-6.dat). Спеціалізована бібліотека
                    (iit / pycachecrypto / privetsigner) у відкритому PyPI
                    недоступна. Якщо файл .dat фактично є PKCS#12-контейнером
                    (перший байт 0x30 — ASN.1 SEQUENCE) — буде завантажений
                    через load_pkcs12; інакше піднімається PrroCryptoError
                    з чітким повідомленням про обмеження.

Формат підпису: XAdES-BES (enveloped), RSA-SHA256, SHA-256.

Використання:
    signer = PrroCryptoSigner(
        key_path=Path("/secure/Key-6.pfx"),
        key_password="secret",
    )
    signed = signer.sign(dat_xml.encode("utf-8"))
    ok = signer.verify(signed)
    serial = signer.get_serial_number()   # для prro_shifts.signer_serial
    name = signer.get_signer_name()       # для prro_shifts.signer_name
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from cryptography import x509
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa
from cryptography.hazmat.primitives.asymmetric.types import PrivateKeyTypes
from cryptography.hazmat.primitives.serialization import pkcs12
from lxml import etree
from signxml import XMLSigner, XMLVerifier, methods

from app.infrastructure.services.prro.iit_sdk import IitSdk, IitSdkError

logger = logging.getLogger(__name__)

# Розширення → формат ключа
_EXTENSION_FORMATS: dict[str, str] = {
    ".pfx": "pfx",
    ".p12": "p12",
    ".jks": "jks",
    ".pem": "pem",
    ".crt": "pem",
    ".cer": "pem",
    ".key": "pem",
    ".dat": "dat",
}

# Перші байти сигнатур форматів
_PKCS12_MAGIC = b"\x30"          # ASN.1 SEQUENCE (PKCS#12 / DER)
_JKS_MAGIC = b"\xfe\xed\xfe\xed"  # Java KeyStore (JKS)
_JCEKS_MAGIC = b"\xce\xce\xce\xce"  # JCEKS

_PEM_MARKERS = (b"-----BEGIN",)


class PrroCryptoError(Exception):
    """Помилка роботи з ключем або підписом ПРРО."""


def _is_pkcs12(data: bytes) -> bool:
    """Перевіряє, чи дані схожі на PKCS#12 (ASN.1-контейнер)."""
    return data.startswith(_PKCS12_MAGIC) and len(data) > 4


# OID ІІТ «ЦСК-1» (1.3.6.1.4.1.19398.*) — контейнер ключа КЕП виробництва
# «Інститут інформаційних технологій» (Використовується АЦСК «Україна»,
# ДПС тощо). .1 — RSA, .2 — ДСТУ 4145-2002.
_IIT_CONTAINER_OID = "1.3.6.1.4.1.19398"


def _parse_oid(der: bytes, offset: int) -> tuple[str | None, int]:
    """
    Наївний парсер одного OID з DER-послідовності.

    Args:
        der: DER-байти.
        offset: позиція, з якої починається TLV (tag вже зчитано).

    Returns:
        (oid_str, next_offset) або (None, offset) при помилці.
    """
    if offset >= len(der):
        return None, offset
    length = der[offset]
    offset += 1
    if length & 0x80:
        num = length & 0x7F
        if num > 4 or offset + num > len(der):
            return None, offset
        length = int.from_bytes(der[offset:offset + num], "big")
        offset += num
    if offset + length > len(der):
        return None, offset

    content = der[offset:offset + length]
    # Перші два арки: first*40 + second
    parts: list[int] = []
    value = 0
    first = True
    for byte in content:
        value = (value << 7) | (byte & 0x7F)
        if not (byte & 0x80):
            if first:
                parts.extend(divmod(value, 40) if value < 80 else (2, value - 80))
                first = False
            else:
                parts.append(value)
            value = 0
    if first:  # жодної завершеної арки
        return None, offset
    return ".".join(str(p) for p in parts), offset + length


def _is_iit_container(data: bytes) -> bool:
    """
    Визначає, чи файл є контейнером ІІТ «ЦСК-1» (Key-6.dat).

    Структура контейнера (ASN.1 DER):
        SEQUENCE {
          SEQUENCE {
            OBJECT IDENTIFIER 1.3.6.1.4.1.19398.1.1.1.{1|2}
            SEQUENCE { OCTET STRING S1, OCTET STRING S2 }
          }
          OCTET STRING (зашифровані дані)
        }

    Returns:
        True, якщо це ІІТ-контейнер.
    """
    if len(data) < 20 or data[0] != 0x30:  # SEQUENCE
        return False
    # tag SEQUENCE (0x30), потім довжина (може бути 2-байтова 0x82)
    offset = 1
    length = data[offset]
    offset += 1
    if length & 0x80:
        num = length & 0x7F
        if num > 2 or offset + num > len(data):
            return False
        offset += num
    # Внутрішня SEQUENCE (0x30) з OID
    if offset >= len(data) or data[offset] != 0x30:
        return False
    offset += 1
    inner_len = data[offset]
    offset += 1
    if inner_len & 0x80:
        num = inner_len & 0x7F
        if num > 2 or offset + num > len(data):
            return False
        offset += num
    if offset >= len(data) or data[offset] != 0x06:  # OBJECT IDENTIFIER
        return False
    oid, _ = _parse_oid(data, offset + 1)
    return bool(oid) and oid.startswith(_IIT_CONTAINER_OID)


class PrroCryptoSigner:
    """
    XAdES-підписант XML-документів ПРРО з авто-визначенням формату ключа.

    Args:
        key_path: шлях до файлу ключа (pfx/p12/jks/pem/dat).
        key_password: пароль ключа (ніколи не логується).
        key_format: примусовий формат; якщо None — визначається автоматично
            за розширенням файлу та сигнатурою (перші байти).
    """

    def __init__(
        self,
        key_path: Path | str,
        key_password: str,
        key_format: str | None = None,
    ) -> None:
        self.key_path = Path(key_path)
        if not self.key_path.is_file():
            raise PrroCryptoError(f"Файл ключа не знайдено: {self.key_path}")

        self.key_password = key_password
        self.key_format = (key_format or self.detect_format()).lower()

        # Бекенд підписання: "signxml" (RSA/EC, XAdES) або "iit"
        # (ДСТУ 4145-2002, CAdES через крипто-ядро ІІТ SDK EUSignCP).
        self._backend: str = "signxml"

        # Для ДСТУ 4145 (JKS): шлях/пароль для лінивого завантаження в SDK.
        # Ініціалізуємо ДО _load_key_material() — він може їх заповнити.
        self._iit_jks_path: Path | None = None
        self._iit_jks_password: str = ""
        self._iit_loaded = False

        # Завантажуємо матеріали ключа (приватний ключ + сертифікат)
        self._private_key: PrivateKeyTypes | None = None
        self._certificate: x509.Certificate | None = None
        self._private_key, self._certificate = self._load_key_material()

    # ─── Визначення формату ────────────────────────────────────────────────

    def detect_format(self) -> str:
        """
        Визначає формат ключа за розширенням файлу та сигнатурою.

        Returns:
            str — "pfx" | "p12" | "jks" | "pem" | "dat".

        Raises:
            PrroCryptoError: якщо формат не вдалося визначити.
        """
        suffix = self.key_path.suffix.lower()
        fmt = _EXTENSION_FORMATS.get(suffix)
        if fmt is None:
            raise PrroCryptoError(
                f"Невідоме розширення ключа: {suffix!r}. "
                f"Підтримуються: {sorted(_EXTENSION_FORMATS)}"
            )

        # Додаткова перевірка сигнатури (перші байти) для уточнення формату
        try:
            head = self.key_path.open("rb").read(16)
        except OSError as exc:
            raise PrroCryptoError(f"Не вдалося прочитати ключ: {exc}") from exc

        if fmt in ("pfx", "p12", "dat") and not _is_pkcs12(head):
            # .dat може бути не PKCS#12 — залишаємо формат як є,
            # але при завантаженні буде піднято документовану помилку.
            logger.warning(
                "PRRO_CRYPTO | файл %s має формат %s, але перші байти "
                "не схожі на PKCS#12",
                self.key_path.name, fmt,
            )
        if fmt == "jks" and not (
            head.startswith(_JKS_MAGIC) or head.startswith(_JCEKS_MAGIC)
        ):
            logger.warning(
                "PRRO_CRYPTO | файл %s має розширення .jks, але сигнатура "
                "не схожа на JKS/JCEKS",
                self.key_path.name,
            )
        if fmt == "pem" and not head.startswith(_PEM_MARKERS):
            logger.warning(
                "PRRO_CRYPTO | файл %s має розширення .pem, але не містить "
                "PEM-маркерів -----BEGIN",
                self.key_path.name,
            )

        return fmt

    # ─── Завантаження ключових матеріалів ──────────────────────────────────

    def _load_key_material(self) -> tuple[PrivateKeyTypes, x509.Certificate]:
        """Завантажує (приватний ключ, сертифікат) залежно від формату."""
        data = self.key_path.read_bytes()

        if self.key_format in ("pfx", "p12"):
            return self._load_from_pkcs12(data)
        if self.key_format == "dat":
            return self._load_from_dat(data)
        if self.key_format == "jks":
            return self._load_from_jks()
        if self.key_format == "pem":
            return self._load_from_pem()
        raise PrroCryptoError(f"Непідтримуваний формат ключа: {self.key_format!r}")

    def _load_from_pkcs12(
        self, data: bytes
    ) -> tuple[PrivateKeyTypes, x509.Certificate]:
        """Завантажує ключ із PKCS#12-контейнера."""
        password = self.key_password.encode("utf-8") if self.key_password else None
        try:
            key, cert, _ = pkcs12.load_key_and_certificates(
                data, password
            )
        except ValueError as exc:
            raise PrroCryptoError(
                f"Не вдалося розібрати PKCS#12 ({self.key_path.name}): {exc}"
            ) from exc
        if key is None:
            raise PrroCryptoError(
                f"У PKCS#12 ({self.key_path.name}) відсутній приватний ключ"
            )
        if cert is None:
            raise PrroCryptoError(
                f"У PKCS#12 ({self.key_path.name}) відсутній сертифікат"
            )
        logger.info("PRRO_CRYPTO | PKCS#12 завантажено: %s", self.key_path.name)
        return key, cert

    def _load_from_dat(self, data: bytes) -> tuple[PrivateKeyTypes, x509.Certificate]:
        """
        Завантажує ключ із файлу .dat.

        Формат ІІТ «ЦСК-1» (Key-6.dat) не підтримується жодною відкритою
        бібліотекою PyPI (iit / pycachecrypto / privetsigner недоступні).
        Однак деякі .dat-файли насправді є PKCS#12-контейнерами —
        у цьому разі вони завантажуються через load_pkcs12.
        """
        if _is_iit_container(data):
            raise PrroCryptoError(
                "Файл ключа є контейнером ІІТ «ЦСК-1» (Key-6.dat, ДСТУ 4145-2002). "
                "Цей формат використовує закрите крипто-ядро ІІТ (SDK EUSign), "
                "яке недоступне для Python. Будь ласка, конвертуйте ключ у "
                "PKCS#12 (.pfx/.p12) або PEM (наприклад, через KeyConverter / "
                "програмне забезпечення ІІТ «Користувач ЦСК-1») і повторіть "
                "налаштування ПРРО."
            )

        if _is_pkcs12(data):
            logger.info(
                "PRRO_CRYPTO | %s визначено як PKCS#12-контейнер (у .dat)",
                self.key_path.name,
            )
            return self._load_from_pkcs12(data)

        raise PrroCryptoError(
            "Невідомий формат ключа .dat. Очікувався контейнер ІІТ «ЦСК-1» "
            "(Key-6.dat) або PKCS#12. Перевірте файл ключа та спробуйте "
            "конвертувати його у PKCS#12 (.pfx/.p12) або PEM."
        )

    _OID_DSTU4145_LE = "1.2.804.2.1.1.1.1.3.1.1"   # ДСТУ 4145-2002 little endian
    _OID_DSTU4145_BE = "1.2.804.2.1.1.1.1.3.1.2"   # ДСТУ 4145-2002 big endian

    def _load_from_jks(self) -> tuple[PrivateKeyTypes, x509.Certificate]:
        """Завантажує ключ із Java KeyStore (JKS/JCEKS) через pyjks.

        pyjks 20.0.0 падає при `decrypt()` на PKCS#8 з нестандартними
        параметрами алгоритму (ДСТУ 4145-2002 / ІІТ): валідація через
        pyasn1 rfc5208 не розуміє структуру AlgorithmIdentifier.
        Тут ця валідація обходиться (ключ просто розшифровується,
        структура PKCS#8 залишається незмінною).
        """
        try:
            import jks  # модуль pyjks
            from jks.jks import PrivateKeyEntry
            from jks import sun_crypto
            from pyasn1.codec.ber import decoder
            from pyasn1_modules import rfc5208
        except ImportError as exc:  # pragma: no cover
            raise PrroCryptoError(
                "Для роботи з JKS-ключами встановіть бібліотеку pyjks: "
                "pip install pyjks"
            ) from exc

        def _loose_decrypt(self, key_password: str) -> None:
            """decrypt без pyasn1-валідації PKCS#8 (підтримка ДСТУ 4145)."""
            encrypted_info = decoder.decode(
                self._encrypted, asn1Spec=rfc5208.EncryptedPrivateKeyInfo()
            )[0]
            algo_id = encrypted_info["encryptionAlgorithm"]["algorithm"].asTuple()
            encrypted_private_key = encrypted_info["encryptedData"].asOctets()
            if algo_id == sun_crypto.SUN_JKS_ALGO_ID:
                plaintext = sun_crypto.jks_pkey_decrypt(
                    encrypted_private_key, key_password
                )
            else:
                raise PrroCryptoError(
                    f"Непідтримуваний алгоритм захисту ключа JKS: {algo_id}"
                )
            self._encrypted = None
            self._pkey_pkcs8 = plaintext
            self._pkey = plaintext
            self._algorithm_oid = None

        # monkeypatch: обхід падіння pyjks на ДСТУ-ключах
        orig_decrypt = PrivateKeyEntry.decrypt
        PrivateKeyEntry.decrypt = _loose_decrypt
        try:
            store = jks.KeyStore.load(str(self.key_path), self.key_password)
        except Exception as exc:  # noqa: BLE001
            raise PrroCryptoError(
                f"Не вдалося відкрити JKS ({self.key_path.name}): {exc}"
            ) from exc
        finally:
            PrivateKeyEntry.decrypt = orig_decrypt

        if not store.private_keys:
            raise PrroCryptoError(
                f"У JKS ({self.key_path.name}) відсутні приватні ключі"
            )

        # Беремо перший приватний ключ
        alias, entry = next(iter(store.private_keys.items()))
        try:
            entry.decrypt(self.key_password)
            key_der: bytes = entry.pkey_pkcs8
            cert_der: bytes = entry.cert_chain[0][1]
        except Exception as exc:  # noqa: BLE001
            raise PrroCryptoError(
                f"Не вдалося розшифрувати ключ JKS ({alias}): {exc}"
            ) from exc

        # Діагностика алгоритму ключа з PKCS#8 (OID в AlgorithmIdentifier)
        algo_oid = self._detect_pkcs8_algorithm(key_der)
        if algo_oid in (self._OID_DSTU4145_LE, self._OID_DSTU4145_BE):
            # ДСТУ 4145-2002: підпис виконує крипто-ядро ІІТ (SDK EUSignCP).
            # Ключ/сертифікати читаються напряму з JKS (EUGetJKSPrivateKeyFile),
            # тому тут зберігаємо лише шлях/пароль для лінивого завантаження.
            self._backend = "iit"
            self._iit_jks_path = self.key_path
            self._iit_jks_password = self.key_password
            logger.info(
                "PRRO_CRYPTO | JKS містить ДСТУ 4145-2002 ключ (alias=%s) — "
                "бекенд ІІТ SDK", alias,
            )
            return None, None

        try:
            private_key = serialization.load_der_private_key(
                key_der, password=None
            )
            certificate = x509.load_der_x509_certificate(cert_der)
        except ValueError as exc:
            raise PrroCryptoError(
                f"Не вдалося розібрати ключ/сертифікат JKS ({alias}): {exc}"
            ) from exc

        logger.info("PRRO_CRYPTO | JKS завантажено: %s (alias=%s)", self.key_path.name, alias)
        return private_key, certificate

    @staticmethod
    def _detect_pkcs8_algorithm(key_der: bytes) -> str | None:
        """Витягує OID алгоритму з PKCS#8 (перший OBJECT у DER).

        Не покладається на pyasn1 (не розуміє ДСТУ 4145 параметри),
        а читає DER-структуру PKCS#8 вручну:
        SEQUENCE { INTEGER 0, SEQUENCE { OBJECT oid, ... }, ... }
        """
        try:
            i = 0
            assert key_der[i] == 0x30          # SEQUENCE (PrivateKeyInfo)
            i += 1
            ln = key_der[i]; i += 1
            if ln & 0x80:                      # довга форма довжини
                i += ln & 0x7F
            assert key_der[i] == 0x02          # INTEGER (version)
            i += 1
            ln = key_der[i]; i += 1
            if ln & 0x80:
                i += ln & 0x7F
            i += ln
            assert key_der[i] == 0x30          # SEQUENCE (AlgorithmIdentifier)
            i += 1
            ln = key_der[i]; i += 1
            if ln & 0x80:
                i += ln & 0x7F
            assert key_der[i] == 0x06          # OBJECT (algorithm OID)
            i += 1
            oid_len = key_der[i]; i += 1
            oid_bytes = key_der[i:i + oid_len]

            parts: list[str] = []
            first = oid_bytes[0]
            parts.append(str(first // 40))
            parts.append(str(first % 40))
            val = 0
            for b in oid_bytes[1:]:
                val = (val << 7) | (b & 0x7F)
                if not (b & 0x80):
                    parts.append(str(val))
                    val = 0
            return ".".join(parts)
        except Exception:  # noqa: BLE001
            return None

    def _load_from_pem(self) -> tuple[PrivateKeyTypes, x509.Certificate]:
        """Завантажує ключ із PEM-файлу (ключ + сертифікат)."""
        data = self.key_path.read_bytes()

        # ── Приватний ключ ────────────────────────────────────────────────
        password = self.key_password.encode("utf-8") if self.key_password else None
        try:
            private_key = serialization.load_pem_private_key(data, password=password)
        except (ValueError, TypeError) as exc:
            raise PrroCryptoError(
                f"Не вдалося завантажити PEM-ключ ({self.key_path.name}): {exc}"
            ) from exc

        # ── Сертифікат: спочатку в тому ж файлі, потім у файлі-сусіді ────
        certificate: x509.Certificate | None = None
        try:
            certificate = x509.load_pem_x509_certificate(data)
        except ValueError:
            certificate = None

        if certificate is None:
            # Шукаємо сертифікат у файлі з тим самим ім'ям (crt/cer/pem)
            for candidate in (
                self.key_path.with_suffix(".crt"),
                self.key_path.with_suffix(".cer"),
            ):
                if candidate.is_file():
                    try:
                        certificate = x509.load_pem_x509_certificate(
                            candidate.read_bytes()
                        )
                        break
                    except ValueError:
                        continue

        if certificate is None:
            raise PrroCryptoError(
                f"У PEM-файлі ({self.key_path.name}) та файлах-сусідах "
                "(.crt/.cer) не знайдено сертифікат X.509"
            )

        logger.info("PRRO_CRYPTO | PEM завантажено: %s", self.key_path.name)
        return private_key, certificate

    # ─── Допоміжні властивості ─────────────────────────────────────────────

    def _cert_pem(self) -> bytes:
        """Сертифікат у форматі PEM (для signxml)."""
        return self._certificate.public_bytes(serialization.Encoding.PEM)

    def _key_pem(self) -> bytes:
        """Приватний ключ у форматі PEM (для signxml)."""
        if isinstance(self._private_key, (rsa.RSAPrivateKey, ec.EllipticCurvePrivateKey)):
            return self._private_key.private_bytes(
                encoding=serialization.Encoding.PEM,
                format=serialization.PrivateFormat.TraditionalOpenSSL,
                encryption_algorithm=serialization.NoEncryption(),
            )
        return self._private_key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        )

    # ─── Підписання та перевірка ───────────────────────────────────────────

    def sign(self, xml_bytes: bytes) -> bytes:
        """
        Підписує XML-документ (XAdES-BES, enveloped, RSA-SHA256).

        Args:
            xml_bytes: канонічний XML (наприклад, <DAT>…</DAT>) у bytes.

        Returns:
            bytes — підписаний XML: до кореневого елемента додається
            тег <ds:Signature> (enveloped). Для ДСТУ 4145 (бекенд ІІТ) —
            бінарний CAdES-BES підпис від XML (формат офіційного семпла
            programika/prro_sample: `ee.SignInternal(true, data)`).

        Raises:
            PrroCryptoError: якщо підписання не вдалося.
        """
        if self._backend == "iit":
            return self._sign_iit(xml_bytes)

        try:
            root = etree.fromstring(xml_bytes)
        except etree.XMLSyntaxError as exc:
            raise PrroCryptoError(f"Некоректний XML для підписання: {exc}") from exc

        signer = XMLSigner(
            method=methods.enveloped,
            signature_algorithm="rsa-sha256",
            digest_algorithm="sha256",
        )
        try:
            signed_root = signer.sign(
                root,
                key=self._key_pem(),
                cert=self._cert_pem(),
            )
        except Exception as exc:  # noqa: BLE001
            raise PrroCryptoError(f"Помилка XAdES-підписання: {exc}") from exc

        return etree.tostring(
            signed_root,
            xml_declaration=True,
            encoding="UTF-8",
            pretty_print=False,
        )

    def verify(self, signed_xml: bytes) -> bool:
        """
        Перевіряє XAdES-підпис за допомогою сертифіката з ключа.

        Args:
            signed_xml: підписаний XML (результат sign()).

        Returns:
            bool — True, якщо підпис валідний.
        """
        if self._backend == "iit":
            return self._verify_iit(signed_xml)

        try:
            verifier = XMLVerifier()
            verifier.verify(signed_xml, x509_cert=self._cert_pem())
            return True
        except Exception as exc:  # noqa: BLE001
            logger.warning("PRRO_CRYPTO | перевірка підпису не пройшла: %s", exc)
            return False

    # ─── ДСТУ 4145-2002 (бекенд ІІТ SDK) ──────────────────────────────────

    def _ensure_iit(self) -> None:
        """Ліниво завантажує JKS-ключ у крипто-ядро ІІТ (один раз)."""
        if self._iit_loaded:
            return
        if self._backend != "iit":
            return
        if self._iit_jks_path is None:
            raise PrroCryptoError(
                "ДСТУ 4145 ключ: не задано шлях до JKS"
            )
        try:
            IitSdk.get().load_jks_key(
                self._iit_jks_path, self._iit_jks_password
            )
            self._iit_loaded = True
        except IitSdkError as exc:
            raise PrroCryptoError(
                f"Не вдалося завантажити ДСТУ 4145 ключ у крипто-ядро ІІТ: {exc}"
            ) from exc

    def _sign_iit(self, xml_bytes: bytes) -> bytes:
        """
        Підписує XML через крипто-ядро ІІТ (ДСТУ 4145-2002 + Стрибог-256).

        Returns:
            bytes — CAdES-BES підпис (ContentInfo/signedData), який ДПС
            очікує в `Check.check_sign` (формат `ee.SignInternal(true, data)`
            з офіційного семпла programika/prro_sample).
        """
        self._ensure_iit()
        try:
            return IitSdk.get().sign_data_internal(xml_bytes)
        except IitSdkError as exc:
            raise PrroCryptoError(f"Помилка підпису ДСТУ 4145: {exc}") from exc

    def _verify_iit(self, signed_xml: bytes) -> bool:
        """Перевіряє CAdES-BES підпис через крипто-ядро ІІТ.

        Дані знаходяться всередині підпису (CAdES-BES internal), тому
        перевіряється лише криптографічна валідність (дані витягуються
        успішно — отже, підпис цілісний і належить завантаженому ключу).
        """
        self._ensure_iit()
        try:
            return IitSdk.get().verify_data_internal(signed_xml, None)
        except IitSdkError as exc:
            logger.warning("PRRO_CRYPTO | verify(ІІТ): %s", exc)
            return False

    # ─── Дані підписанта ───────────────────────────────────────────────────

    def get_serial_number(self) -> str:
        """
        Повертає серійний номер сертифіката (шістнадцятковий, upper).

        Returns:
            str — наприклад, "3F2A9C01B7D4E8A2".
        """
        if self._backend == "iit":
            self._ensure_iit()
            return IitSdk.get().get_signer_serial()
        if self._certificate is None:
            raise PrroCryptoError("Сертифікат не завантажено")
        return format(self._certificate.serial_number, "X")

    def get_signer_name(self) -> str:
        """
        Повертає ПІБ підписанта з сертифіката.

        Спершу шукається CN (Common Name), потім — комбінація
        Given Name + Surname.

        Returns:
            str — ПІБ або порожній рядок, якщо визначити не вдалося.
        """
        if self._backend == "iit":
            self._ensure_iit()
            return IitSdk.get().get_signer_name()
        if self._certificate is None:
            return ""
        subject = self._certificate.subject

        cn = subject.get_attributes_for_oid(x509.NameOID.COMMON_NAME)
        if cn:
            return str(cn[0].value)

        given = subject.get_attributes_for_oid(x509.NameOID.GIVEN_NAME)
        surname = subject.get_attributes_for_oid(x509.NameOID.SURNAME)
        if given or surname:
            return " ".join(
                part for part in (
                    (surname[0].value if surname else ""),
                    (given[0].value if given else ""),
                ) if part
            )

        return ""


__all__ = ["PrroCryptoSigner", "PrroCryptoError"]
