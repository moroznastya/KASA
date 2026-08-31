"""
Безпечне зберігання налаштувань ключа ПРРО (шлях, формат, пароль).

Зберігає у JSON-файлі (за замовчуванням `backend/.prro_keystore.json`):
  - key_path          — шлях до файлу ключа;
  - key_format        — формат ключа (pfx/p12/jks/pem/dat);
  - password_encrypted — пароль ключа, зашифрований Fernet.

Master-ключ Fernet:
  - береться з env PRRO_MASTER_KEY (якщо задано);
  - інакше генерується при першому запуску та зберігається у файлі
    `backend/.prro_master.key` з правами 0600.

ВАЖЛИВО:
  - пароль ключа НІКОЛИ не логується і не повертається у відповідях API;
  - для відображення використовуйте маску PrroKeyStore.mask_password() → "••••";
  - файли .prro_master.key та .prro_keystore.json не повинні потрапляти
    у git (додайте в .gitignore).

Використання:
    store = PrroKeyStore()
    store.save_key_path("/secure/Key-6.pfx", key_format="pfx")
    store.save_password_encrypted("secret")
    ...
    path = store.get_key_path()
    fmt = store.get_key_format()
    password = store.decrypt_password()
"""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken

logger = logging.getLogger(__name__)

# Ім'я файлу master-ключа та файлу налаштувань (у директорії backend/)
MASTER_KEY_FILE_NAME = ".prro_master.key"
KEYSTORE_FILE_NAME = ".prro_keystore.json"

# Змінна середовища для master-ключа
PRRO_MASTER_KEY_ENV = "PRRO_MASTER_KEY"

# Маска для відображення пароля
PASSWORD_MASK = "••••"


class PrroKeyStoreError(Exception):
    """Помилка роботи зі сховищем ключів ПРРО."""


class PrroKeyStore:
    """
    Зберігає шлях/формат ключа ПРРО та зашифрований пароль.

    Args:
        master_key: готовий master-ключ Fernet (32 байти, url-safe base64).
            Якщо None — береться з env PRRO_MASTER_KEY, інакше
            генерується та зберігається у .prro_master.key.
        keystore_path: шлях до JSON-файлу налаштувань.
            За замовчуванням — backend/.prro_keystore.json.
        master_key_path: шлях до файлу master-ключа.
            За замовчуванням — backend/.prro_master.key.
    """

    def __init__(
        self,
        *,
        master_key: str | None = None,
        keystore_path: Path | str | None = None,
        master_key_path: Path | str | None = None,
    ) -> None:
        backend_dir = Path(__file__).resolve().parents[2]  # backend/
        self._keystore_path = Path(keystore_path or backend_dir / KEYSTORE_FILE_NAME)
        self._master_key_path = Path(
            master_key_path or backend_dir / MASTER_KEY_FILE_NAME
        )

        self._fernet = Fernet(self._get_or_create_master_key(master_key))

    # ─── Master-ключ ───────────────────────────────────────────────────────

    def _get_or_create_master_key(self, master_key: str | None) -> bytes:
        """
        Повертає master-ключ Fernet: з аргумента, env або файлу.

        Порядок:
          1. master_key (аргумент конструктора);
          2. env PRRO_MASTER_KEY;
          3. файл .prro_master.key (якщо існує);
          4. генерація нового ключа → запис у файл з правами 0600.
        """
        if master_key:
            return master_key.encode("utf-8")

        env_key = os.environ.get(PRRO_MASTER_KEY_ENV)
        if env_key:
            return env_key.encode("utf-8")

        if self._master_key_path.is_file():
            try:
                return self._master_key_path.read_bytes().strip()
            except OSError as exc:
                raise PrroKeyStoreError(
                    f"Не вдалося прочитати master-ключ: {exc}"
                ) from exc

        # Генеруємо новий ключ і зберігаємо з правами 0600
        new_key = Fernet.generate_key()
        try:
            self._master_key_path.write_bytes(new_key + b"\n")
            os.chmod(self._master_key_path, 0o600)
            logger.info(
                "PRRO_KEYSTORE | master-ключ згенеровано та збережено: %s",
                self._master_key_path,
            )
        except OSError as exc:
            raise PrroKeyStoreError(
                f"Не вдалося зберегти master-ключ: {exc}"
            ) from exc

        return new_key

    # ─── Читання/запис JSON-файлу налаштувань ─────────────────────────────

    def _load_data(self) -> dict[str, str]:
        """Читає JSON-файл налаштувань (порожній dict, якщо файлу немає)."""
        if not self._keystore_path.is_file():
            return {}
        try:
            return json.loads(self._keystore_path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as exc:
            raise PrroKeyStoreError(
                f"Пошкоджений файл налаштувань ключа: {exc}"
            ) from exc

    def _save_data(self, data: dict[str, str]) -> None:
        """Записує JSON-файл налаштувань з правами 0600."""
        try:
            self._keystore_path.write_text(
                json.dumps(data, ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
            os.chmod(self._keystore_path, 0o600)
        except OSError as exc:
            raise PrroKeyStoreError(
                f"Не вдалося зберегти налаштування ключа: {exc}"
            ) from exc

    # ─── Публічні методи ───────────────────────────────────────────────────

    def save_key_path(self, key_path: str, key_format: str | None = None) -> None:
        """
        Зберігає шлях до файлу ключа та (опційно) формат ключа.

        Args:
            key_path: шлях до файлу ключа (pfx/p12/jks/pem/dat).
            key_format: формат ключа; якщо None — буде визначатись
                автоматично при створенні PrroCryptoSigner.
        """
        data = self._load_data()
        data["key_path"] = key_path
        if key_format:
            data["key_format"] = key_format
        self._save_data(data)
        logger.info("PRRO_KEYSTORE | збережено шлях ключа (без пароля)")

    def get_key_path(self) -> str:
        """
        Повертає збережений шлях до файлу ключа.

        Raises:
            PrroKeyStoreError: якщо шлях не налаштовано.
        """
        data = self._load_data()
        path = data.get("key_path")
        if not path:
            raise PrroKeyStoreError(
                "Шлях до ключа ПРРО не налаштовано. "
                "Викличте save_key_path()"
            )
        return path

    def get_key_format(self) -> str | None:
        """Повертає збережений формат ключа (або None)."""
        return self._load_data().get("key_format")

    def save_password_encrypted(self, password: str) -> None:
        """
        Шифрує пароль ключа (Fernet) та зберігає його у сховищі.

        Пароль ніколи не зберігається у відкритому вигляді та не логується.

        Args:
            password: пароль ключа ПРРО.
        """
        if not password:
            raise PrroKeyStoreError("Пароль ключа не може бути порожнім")

        token = self._fernet.encrypt(password.encode("utf-8"))
        data = self._load_data()
        data["password_encrypted"] = token.decode("ascii")
        self._save_data(data)
        logger.info("PRRO_KEYSTORE | пароль ключа зашифровано та збережено")

    def decrypt_password(self) -> str:
        """
        Розшифровує та повертає пароль ключа.

        Returns:
            str — пароль ключа ПРРО у відкритому вигляді.

        Raises:
            PrroKeyStoreError: якщо пароль не збережено або master-ключ
                не збігається (наприклад, змінено env PRRO_MASTER_KEY).
        """
        data = self._load_data()
        token = data.get("password_encrypted")
        if not token:
            raise PrroKeyStoreError(
                "Пароль ключа ПРРО не збережено. Викличте save_password_encrypted()"
            )
        try:
            return self._fernet.decrypt(token.encode("ascii")).decode("utf-8")
        except InvalidToken as exc:
            raise PrroKeyStoreError(
                "Не вдалося розшифрувати пароль: master-ключ не збігається "
                "(перевірте PRRO_MASTER_KEY або файл .prro_master.key)"
            ) from exc

    def is_configured(self) -> bool:
        """
        Перевіряє, чи налаштовано ключ (шлях + пароль).

        Returns:
            bool — True, якщо збережено і шлях, і пароль.
        """
        data = self._load_data()
        return bool(data.get("key_path") and data.get("password_encrypted"))

    @staticmethod
    def mask_password() -> str:
        """
        Повертає маску пароля для відображення в API/UI.

        Returns:
            str — "••••".
        """
        return PASSWORD_MASK


__all__ = ["PASSWORD_MASK", "PrroKeyStore", "PrroKeyStoreError"]
