"""
Конфігурація застосунку Kasa POS.

Використовує Pydantic Settings для читання змінних оточення
або .env файлу (в корені проєкту).
"""

from pydantic_settings import BaseSettings, SettingsConfigDict
from pathlib import Path


class Settings(BaseSettings):
    # ──────────────────────────────────────────────
    # База даних
    # ──────────────────────────────────────────────
    DB_HOST: str = "localhost"
    DB_PORT: int = 5432
    DB_USER: str = "kasa_user"
    DB_PASSWORD: str = "kasa_pass"
    DB_NAME: str = "kasa_db"

    @property
    def DATABASE_URL(self) -> str:
        """Асинхронний DSN для SQLAlchemy + asyncpg."""
        return (
            f"postgresql+asyncpg://{self.DB_USER}:{self.DB_PASSWORD}"
            f"@{self.DB_HOST}:{self.DB_PORT}/{self.DB_NAME}"
        )

    @property
    def DATABASE_URL_SYNC(self) -> str:
        """Синхронний DSN (для Alembic)."""
        return (
            f"postgresql://{self.DB_USER}:{self.DB_PASSWORD}"
            f"@{self.DB_HOST}:{self.DB_PORT}/{self.DB_NAME}"
        )

    # ──────────────────────────────────────────────
    # Безпека
    # ──────────────────────────────────────────────
    SECRET_KEY: str = "change-me-in-production"
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 480  # 8 годин

    # ──────────────────────────────────────────────
    # Налаштування застосунку
    # ──────────────────────────────────────────────
    APP_NAME: str = "Kasa POS"
    DEBUG: bool = False

    model_config = SettingsConfigDict(
        env_file=Path(__file__).resolve().parent.parent / ".env",
        env_file_encoding="utf-8",
        case_sensitive=True,
    )


# Глобальний екземпляр налаштувань
settings = Settings()
