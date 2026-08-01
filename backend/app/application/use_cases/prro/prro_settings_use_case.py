"""
Application Layer: PrroSettingsUseCase — налаштування ПРРО.

Відповідає за:
  - get_settings()      — повертає PrroSettingsDTO (пароль замасковано);
  - save_settings(...)  — збереження ключа КЕП (копіювання у certs/),
                          пароля (Fernet через key_store), реквізитів ПРРО;
  - test_connection()   — перевірка зв'язку з фіскальним сервером (ping):
                          формує службовий XML T=111, підписує КЕП (якщо
                          доступний) та повертає статус сервера зі
                          зрозумілим поясненням.

ВАЖЛИВО:
  - пароль ключа НІКОЛИ не повертається — тільки маска "••••";
  - файл ключа копіюється у backend/certs/prro-test/ або prro-prod/
    залежно від mode.
"""

from __future__ import annotations

import logging
import re
import shutil
from pathlib import Path
from typing import Optional

from app.application.dto.prro_dto import PrroSettingsDTO
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.key_store import (
    PrroKeyStore,
    PrroKeyStoreError,
    PASSWORD_MASK,
)
from app.infrastructure.services.prro.xml_builder import SERVICE_PING
from app.application.use_cases.prro.context import (
    PrroContextFactory,
    KEY_PRRO_FN,
    KEY_PRRO_TN,
    KEY_PRRO_ZN,
    KEY_PRRO_MODE,
    KEY_AUTO_FISCALIZE,
)

logger = logging.getLogger(__name__)

# Директорія сертифікатів: backend/certs/prro-{mode}/
_BACKEND_DIR = Path(__file__).resolve().parents[4]
CERTS_ROOT = _BACKEND_DIR / "certs"


class PrroSettingsError(Exception):
    """Помилка роботи з налаштуваннями ПРРО."""


class PrroSettingsUseCase:
    """
    Use Case для налаштувань ПРРО.

    Args:
        settings_repo: репозиторій налаштувань ПРРО (PrroSetting).
        prro_repo: репозиторій змін/черги ПРРО.
        key_store: сховище шляху/пароля ключа КЕП.
        context_factory: фабрика компонентів ПРРО.
    """

    def __init__(
        self,
        settings_repo: PrroSettingsRepository,
        prro_repo: PrroRepository,
        key_store: PrroKeyStore,
        context_factory: PrroContextFactory,
    ) -> None:
        self._settings_repo = settings_repo
        self._prro_repo = prro_repo
        self._key_store = key_store
        self._context = context_factory

    # ─── Читання ───────────────────────────────────────────────────────────

    async def get_settings(self) -> PrroSettingsDTO:
        """
        Повертає поточні налаштування ПРРО.

        Пароль ключа завжди замаскований ("••••" або None).

        Returns:
            PrroSettingsDTO.
        """
        keys = await self._settings_repo.get_many(
            [KEY_PRRO_FN, KEY_PRRO_TN, KEY_PRRO_ZN, KEY_PRRO_MODE,
             KEY_AUTO_FISCALIZE]
        )
        mode = keys.get(KEY_PRRO_MODE) or self._context._config.PRRO_MODE
        url = self._context._config.PRRO_PROD_URL if mode == "prod" \
            else self._context._config.PRRO_TEST_URL

        # Шлях/формат/пароль — з key_store (не з БД)
        key_path: Optional[str] = None
        try:
            key_path = self._key_store.get_key_path()
        except PrroKeyStoreError:
            key_path = None
        key_format = self._key_store.get_key_format()
        has_password = self._key_store.is_configured()

        open_shift = await self._prro_repo.get_open_shift()

        return PrroSettingsDTO(
            key_file=key_path,
            key_password_masked=PASSWORD_MASK if has_password else None,
            key_format=key_format,
            prro_fn=keys.get(KEY_PRRO_FN),
            prro_tn=keys.get(KEY_PRRO_TN),
            prro_zn=keys.get(KEY_PRRO_ZN),
            mode=mode,
            url=url,
            shift_open=open_shift is not None,
            online=await self._check_online(),
            auto_fiscalize=self._parse_bool(keys.get(KEY_AUTO_FISCALIZE)),
        )

    async def _check_online(self) -> bool:
        """Перевіряє онлайн-статус ПРРО (best-effort, короткий таймаут)."""
        try:
            client = await self._context.grpc_client()
            response = await client.status(timeout=5)
            return bool(getattr(response, "online", False))
        except Exception:  # noqa: BLE001 — жодна помилка не блокує налаштування
            return False

    # ─── Швидкий доступ до ФН ─────────────────────────────────────────────

    async def get_prro_fn(self) -> str | None:
        """
        Повертає фіскальний номер ПРРО (без мережевих перевірок).

        Використовується для формування QR-посилань перевірки чеку.

        Returns:
            str | None — prro_fn або None, якщо не налаштовано.
        """
        return await self._settings_repo.get(KEY_PRRO_FN)

    # ─── Збереження ────────────────────────────────────────────────────────

    async def save_settings(
        self,
        *,
        key_file_path: str | None = None,
        key_file_content: bytes | None = None,
        key_file_name: str | None = None,
        key_password: str | None = None,
        prro_fn: str | None = None,
        prro_tn: str | None = None,
        prro_zn: str | None = None,
        mode: str | None = None,
        auto_fiscalize: bool | None = None,
    ) -> PrroSettingsDTO:
        """
        Зберігає налаштування ПРРО.

        Args:
            key_file_path: шлях до існуючого файлу ключа (копіюється у certs/).
            key_file_content: вміст завантаженого файлу ключа (UploadFile).
            key_file_name: ім'я завантаженого файлу ключа.
            key_password: пароль ключа (шифрується Fernet).
            prro_fn: фіскальний номер ПРРО.
            prro_tn: податковий номер платника.
            prro_zn: заводський номер ПРРО.
            mode: режим роботи ("test"/"prod").
            auto_fiscalize: автоматична фіскалізація чеків після продажу
                (True/False; None — не змінювати поточне значення).

        Returns:
            PrroSettingsDTO — оновлені налаштування.
        """
        if mode not in (None, "test", "prod"):
            raise PrroSettingsError(
                f"Невідомий режим ПРРО: {mode!r}. Допустимі: 'test', 'prod'"
            )

        # 1. Режим (новий або поточний)
        current_mode = await self._settings_repo.get(KEY_PRRO_MODE) \
            or self._context._config.PRRO_MODE
        target_mode = mode or current_mode

        # 2. Ключ: копіюємо у certs/prro-{mode}/
        if key_file_content is not None:
            if not key_file_name:
                raise PrroSettingsError(
                    "Не вказано ім'я файлу ключа (key_file_name)"
                )
            key_path = self._save_uploaded_key(
                key_file_content, key_file_name, target_mode
            )
        elif key_file_path:
            key_path = self._copy_key_file(key_file_path, target_mode)
        else:
            key_path = None

        if key_path:
            ext = Path(key_path).suffix.lower().lstrip(".")
            self._key_store.save_key_path(key_path, key_format=ext or None)
            logger.info("PRRO_SETTINGS | ключ збережено: %s", key_path)

        # 3. Пароль (Fernet)
        if key_password:
            self._key_store.save_password_encrypted(key_password)
            logger.info("PRRO_SETTINGS | пароль ключа збережено (зашифровано)")

        # 4. Реквізити ПРРО (з валідацією формату)
        if prro_fn is not None:
            prro_fn = prro_fn.strip()
            if not re.fullmatch(r"\d{5,15}", prro_fn):
                raise PrroSettingsError(
                    "Невірний фіскальний номер (prro_fn): очікується 5–15 цифр, "
                    f"отримано {prro_fn!r}"
                )
            await self._settings_repo.set(KEY_PRRO_FN, prro_fn)
        if prro_tn is not None:
            prro_tn = prro_tn.strip()
            if not (5 <= len(prro_tn) <= 20):
                raise PrroSettingsError(
                    "Невірний податковий номер (prro_tn): очікується 5–20 символів, "
                    f"отримано {prro_tn!r}"
                )
            await self._settings_repo.set(KEY_PRRO_TN, prro_tn)
        if prro_zn is not None:
            prro_zn = prro_zn.strip()
            if not (3 <= len(prro_zn) <= 30):
                raise PrroSettingsError(
                    "Невірний заводський номер (prro_zn): очікується 3–30 символів, "
                    f"отримано {prro_zn!r}"
                )
            await self._settings_repo.set(KEY_PRRO_ZN, prro_zn)
        if mode is not None:
            await self._settings_repo.set(KEY_PRRO_MODE, mode)

        # 5. Авто-фіскалізація
        if auto_fiscalize is not None:
            await self._settings_repo.set(
                KEY_AUTO_FISCALIZE, "true" if auto_fiscalize else "false"
            )
            logger.info(
                "PRRO_SETTINGS | auto_fiscalize = %s", auto_fiscalize
            )

        return await self.get_settings()

    @staticmethod
    def _parse_bool(value: str | None) -> bool:
        """Перетворює текстове значення прапора ('1'/'true'/'yes'/'on') у bool."""
        if value is None:
            return False
        return value.strip().lower() in ("1", "true", "yes", "on")

    def _certs_dir(self, mode: str) -> Path:
        """Директорія для ключів: backend/certs/prro-{mode}/."""
        certs_dir = CERTS_ROOT / f"prro-{mode}"
        certs_dir.mkdir(parents=True, exist_ok=True)
        return certs_dir

    def _save_uploaded_key(
        self, content: bytes, filename: str, mode: str
    ) -> str:
        """Зберігає завантажений файл ключа у certs/prro-{mode}/."""
        safe_name = Path(filename).name  # захист від path traversal
        if not safe_name:
            raise PrroSettingsError("Порожнє ім'я файлу ключа")
        dest = self._certs_dir(mode) / safe_name
        try:
            dest.write_bytes(content)
        except OSError as exc:
            raise PrroSettingsError(
                f"Не вдалося зберегти ключ у {dest}: {exc}"
            ) from exc
        return str(dest)

    def _copy_key_file(self, src_path: str, mode: str) -> str:
        """Копіює файл ключа у certs/prro-{mode}/."""
        src = Path(src_path)
        if not src.is_file():
            raise PrroSettingsError(f"Файл ключа не знайдено: {src_path}")
        dest = self._certs_dir(mode) / src.name
        try:
            shutil.copy2(src, dest)
        except OSError as exc:
            raise PrroSettingsError(
                f"Не вдалося скопіювати ключ у {dest}: {exc}"
            ) from exc
        return str(dest)

    # ─── Перевірка зв'язку ─────────────────────────────────────────────────

    # Людською мовою пояснення статусів CheckResponse (prro_pb2).
    _STATUS_MESSAGES: dict[int, str] = {
        1: "Зв'язок із фіскальним сервером встановлено (OK).",
        -1: "Помилка перевірки підпису/розбору XML (ERROR_VEREFY). "
            "Найчастіші причини: (1) ключ КЕП не завантажено або його "
            "формат не підтримується; (2) XML у check_sign порожній або "
            "непідписаний; (3) сертифікат підписанта не зареєстровано "
            "в тестовому середовищі ДПС.",
        -2: "Помилка перевірки ПРРО (ERROR_CHECK). Перевірте реквізити ПРРО.",
        -3: "Помилка запису на сервері (ERROR_SAVE). Спробуйте пізніше.",
        -4: "Загальна помилка сервера (ERROR_UNKNOWN).",
        -5: "Помилка типу посилки (ERROR_TYPE). Перевірте check_type.",
        -6: "Немає Z-звіту за попередній день (ERROR_NOT_PREV_ZREPORT).",
        -7: "Невірний формат XML (ERROR_XML).",
        -8: "Дата не відповідає Check.date (ERROR_XML_DATE).",
        -9: "Невірний формат XML чеку (ERROR_XML_CHK).",
        -10: "Невірний формат Z-звіту (ERROR_XML_ZREPORT).",
        -11: "Перевищено ліміт 168 годин офлайну (ERROR_OFFLINE_168).",
        -12: "Невірний хеш попереднього чеку (ERROR_BAD_HASH_PREV).",
        -13: "ПРРО не зареєстровано (ERROR_NOT_REGISTERED_RRO). "
             "Зареєструйте ПРРО в кабінеті платника податків.",
        -14: "Підписант не зареєстрований (ERROR_NOT_REGISTERED_SIGNER). "
             "Зареєструйте сертифікат підписанта в кабінеті платника.",
        -15: "Не відкрита зміна (ERROR_NOT_OPEN_SHIFT). "
             "Відкрийте зміну (POST /api/v2/prro/shift/open).",
        -16: "Невірний офлайн ID (ERROR_OFFLINE_ID).",
    }

    @staticmethod
    def _status_message(status: int) -> str:
        """Повертає зрозуміле пояснення статусу сервера."""
        return PrroSettingsUseCase._STATUS_MESSAGES.get(
            status, "Невідомий статус фіскального сервера."
        )

    async def _build_ping_check_sign(self) -> tuple[bytes | None, str | None]:
        """
        Формує XML службового чеку T=111 для ping.

        Returns:
            (check_sign, error):
              - check_sign: підписаний XML (якщо КЕП прочитано) або
                непідписаний XML (якщо ключ недоступний);
              - error: пояснення, чому ключ не вдалося використати
                (None, якщо ключ прочитано успішно).
        """
        xml_builder = await self._context.build_xml_builder()
        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_PING
        )
        # Згідно з документацією: «XML з типом <CT="111">. MAC не заповнюється».
        message = xml_builder.build_message(dat_xml, include_mac=False)

        try:
            crypto = await self._context.build_crypto_signer()
            signed = crypto.sign(message.encode("utf-8"))
            return signed, None
        except Exception as exc:  # noqa: BLE001 — ключ може бути будь-якого формату
            logger.warning("PRRO_SETTINGS | не вдалося підписати ping XML: %s", exc)
            return message.encode("utf-8"), str(exc)

    async def test_connection(self) -> dict:
        """
        Перевіряє зв'язок з фіскальним сервером (ping).

        Формує службовий XML T=111, підписує його КЕП (якщо ключ доступний)
        і надсилає через метод ping. Повертає статус сервера зі зрозумілим
        поясненням.

        Returns:
            dict: {"status": int, "ok": bool, "error": str | None}.
        """
        try:
            check_sign, sign_error = await self._build_ping_check_sign()
            client = await self._context.grpc_client()
            response = await client.ping(check_sign=check_sign)

            status = int(response.status)
            server_error = response.error_message or None

            # Складаємо пояснення: помилка підпису + текст сервера + маппінг
            parts: list[str] = []
            if sign_error:
                parts.append(f"КЕП не вдалося використати: {sign_error}")
            if server_error:
                parts.append(f"Відповідь сервера: {server_error}")
            parts.append(self._status_message(status))

            return {
                "status": status,
                "ok": status == 1,
                "error": " | ".join(parts),
            }
        except Exception as exc:  # noqa: BLE001
            logger.warning("PRRO_SETTINGS | ping не вдався: %s", exc)
            return {"status": 0, "ok": False, "error": str(exc)}


__all__ = ["PrroSettingsUseCase", "PrroSettingsError"]
