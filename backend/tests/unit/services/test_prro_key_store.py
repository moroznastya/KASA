"""Unit tests: PrroKeyStore — безпечне зберігання ключа/пароля (Fernet)."""

from __future__ import annotations

import json
import os

import pytest
from cryptography.fernet import Fernet

from app.infrastructure.services.prro.key_store import (
    PrroKeyStore,
    PrroKeyStoreError,
    PASSWORD_MASK,
)


@pytest.fixture
def master_key() -> str:
    """Тестовий master-ключ Fernet."""
    return Fernet.generate_key().decode("ascii")


@pytest.fixture
def store(tmp_path, master_key) -> PrroKeyStore:
    """Сховище з тестовим master-ключем та tmp-файлами."""
    return PrroKeyStore(
        master_key=master_key,
        keystore_path=tmp_path / "keystore.json",
        master_key_path=tmp_path / "master.key",
    )


class TestKeyStore:
    """Основні операції сховища ключів."""

    def test_save_and_get_key_path(self, store):
        """Шлях до ключа зберігається та повертається."""
        store.save_key_path("/secure/Key-6.pfx", key_format="pfx")
        assert store.get_key_path() == "/secure/Key-6.pfx"
        assert store.get_key_format() == "pfx"

    def test_save_without_format(self, store):
        """Формат можна не вказувати."""
        store.save_key_path("/secure/Key-6.pfx")
        assert store.get_key_format() is None

    def test_get_key_path_not_configured(self, store):
        """Неналаштований шлях → PrroKeyStoreError."""
        with pytest.raises(PrroKeyStoreError):
            store.get_key_path()

    def test_password_roundtrip(self, store):
        """Пароль шифрується та розшифровується коректно."""
        store.save_password_encrypted("Super-Secret-123")
        assert store.decrypt_password() == "Super-Secret-123"

    def test_password_not_stored_plaintext(self, store, tmp_path):
        """Пароль НЕ зберігається у відкритому вигляді у файлі."""
        store.save_password_encrypted("Super-Secret-123")
        raw = (tmp_path / "keystore.json").read_text(encoding="utf-8")
        assert "Super-Secret-123" not in raw

    def test_password_not_configured(self, store):
        """Незбережений пароль → PrroKeyStoreError."""
        with pytest.raises(PrroKeyStoreError):
            store.decrypt_password()

    def test_wrong_master_key_fails(self, tmp_path, master_key):
        """Інший master-ключ не може розшифрувати пароль."""
        store = PrroKeyStore(
            master_key=master_key,
            keystore_path=tmp_path / "keystore.json",
            master_key_path=tmp_path / "master.key",
        )
        store.save_password_encrypted("Secret")

        other = PrroKeyStore(
            master_key=Fernet.generate_key().decode("ascii"),
            keystore_path=tmp_path / "keystore.json",
            master_key_path=tmp_path / "master.key",
        )
        with pytest.raises(PrroKeyStoreError):
            other.decrypt_password()

    def test_is_configured(self, store):
        """is_configured — True лише коли є шлях і пароль."""
        assert store.is_configured() is False
        store.save_key_path("/secure/key.pfx")
        assert store.is_configured() is False
        store.save_password_encrypted("secret")
        assert store.is_configured() is True

    def test_mask_password(self):
        """Маска пароля — '••••'."""
        assert PrroKeyStore.mask_password() == PASSWORD_MASK == "••••"

    def test_empty_password_raises(self, store):
        """Порожній пароль → PrroKeyStoreError."""
        with pytest.raises(PrroKeyStoreError):
            store.save_password_encrypted("")


class TestMasterKeyFile:
    """Генерація master-ключа у файлі .prro_master.key."""

    def test_master_key_generated_with_0600(self, tmp_path, monkeypatch):
        """Master-ключ генерується та зберігається з правами 0600."""
        monkeypatch.delenv("PRRO_MASTER_KEY", raising=False)
        key_file = tmp_path / "master.key"
        store = PrroKeyStore(
            keystore_path=tmp_path / "keystore.json",
            master_key_path=key_file,
        )
        assert key_file.is_file()
        # Права 0600
        assert os.stat(key_file).st_mode & 0o777 == 0o600

        # Повторне створення — той самий ключ (з файлу)
        store2 = PrroKeyStore(
            keystore_path=tmp_path / "keystore.json",
            master_key_path=key_file,
        )
        assert store2._fernet._signing_key == store._fernet._signing_key

    def test_env_master_key_used(self, tmp_path, monkeypatch):
        """Master-ключ з env PRRO_MASTER_KEY використовується без файлу."""
        env_key = Fernet.generate_key().decode("ascii")
        monkeypatch.setenv("PRRO_MASTER_KEY", env_key)

        store = PrroKeyStore(
            keystore_path=tmp_path / "keystore.json",
            master_key_path=tmp_path / "never-created.key",
        )
        store.save_password_encrypted("env-secret")
        assert store.decrypt_password() == "env-secret"
        # Файл master-ключа не створюється
        assert not (tmp_path / "never-created.key").exists()

    def test_keystore_file_permissions(self, store, tmp_path):
        """Файл налаштувань зберігається з правами 0600."""
        store.save_key_path("/tmp/key.pfx")
        assert os.stat(tmp_path / "keystore.json").st_mode & 0o777 == 0o600
