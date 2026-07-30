"""
Конфігурація застосунку Kasa POS.

Використовує Pydantic Settings для читання змінних оточення
або .env файлу (в корені проєкту).
"""

from pydantic_settings import BaseSettings, SettingsConfigDict
from pydantic import field_validator
from pathlib import Path


class Settings(BaseSettings):
    # ──────────────────────────────────────────────
    # База даних
    # ──────────────────────────────────────────────
    DB_HOST: str = "localhost"
    DB_PORT: int = 5432
    DB_USER: str = "postgres"
    DB_PASSWORD: str = "VgxWd7MBJ10X"
    DB_NAME: str = "pos_system"

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
    SECRET_KEY: str = "cd0e5965b2bca226a1270c59716fe7e83341ec18212f0bfdbd592aaa8ba7c881"
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 480  # 8 годин
    REFRESH_TOKEN_EXPIRE_MINUTES: int = 10080  # 7 днів

    @field_validator("SECRET_KEY")
    @classmethod
    def validate_secret_key(cls, v: str) -> str:
        """Валідація SECRET_KEY: не дорівнює дефолту та має довжину >= 32."""
        if v == "change-me-in-production":
            raise ValueError(
                "SECRET_KEY must be changed in production! "
                "Generate a new key with: openssl rand -hex 32"
            )
        if len(v) < 32:
            raise ValueError(
                f"SECRET_KEY must be at least 32 characters long, "
                f"got {len(v)} characters"
            )
        return v

    # ──────────────────────────────────────────────
    # CORS
    # ──────────────────────────────────────────────
    CORS_ORIGINS: str = "http://localhost:3000,http://localhost:5173"

    @property
    def CORS_ORIGINS_LIST(self) -> list[str]:
        """Повертає список дозволених CORS-доменів."""
        return [origin.strip() for origin in self.CORS_ORIGINS.split(",") if origin.strip()]

    # ──────────────────────────────────────────────
    # Rate Limiting
    # ──────────────────────────────────────────────
    RATE_LIMIT_AUTH: str = "5/minute"

    # ──────────────────────────────────────────────
    # Redis / Cache — конфігурація кешування
    # ──────────────────────────────────────────────
    REDIS_URL: str = "redis://localhost:6379/0"
    REDIS_HOST: str = "localhost"
    REDIS_PORT: int = 6379
    REDIS_DB: int = 0
    REDIS_PASSWORD: str | None = None

    @property
    def REDIS_ACTUAL_URL(self) -> str:
        """Повертає фактичний Redis URL (з REDIS_URL або з компонентів)."""
        if self.REDIS_URL:
            return self.REDIS_URL
        if self.REDIS_PASSWORD:
            return f"redis://:{self.REDIS_PASSWORD}@{self.REDIS_HOST}:{self.REDIS_PORT}/{self.REDIS_DB}"
        return f"redis://{self.REDIS_HOST}:{self.REDIS_PORT}/{self.REDIS_DB}"

    # ─── TTL для кешу (секунди) ──────────────────────────────────────────────
    CACHE_TTL_DEFAULT: int = 300          # 5 хвилин (загальний)
    CACHE_TTL_PRODUCTS: int = 60          # 1 хвилина (списки продуктів)
    CACHE_TTL_PRODUCT_DETAIL: int = 300   # 5 хвилин (деталі продукту)
    CACHE_TTL_CATEGORIES: int = 300       # 5 хвилин (категорії)
    CACHE_TTL_INVOICES: int = 600         # 10 хвилин (накладні)

    # ──────────────────────────────────────────────
    # Налаштування застосунку
    # ──────────────────────────────────────────────
    APP_NAME: str = "Kasa POS"
    DEBUG: bool = False

    model_config = SettingsConfigDict(
        env_file=Path(__file__).resolve().parent / ".env",
        env_file_encoding="utf-8",
        case_sensitive=True,
    )


# Глобальний екземпляр налаштувань
settings = Settings()
