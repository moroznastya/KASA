"""
Конфігурація застосунку Kasa POS.

Використовує Pydantic Settings для читання змінних оточення
або .env файлу (в корені backend/ — рівень вище від app/).

🔐 Безпека (оновлено 2026-08-01):
    - DB_PASSWORD: більше НЕ має хардкод-дефолту!
      Значення обов'язково читається з .env або середовища.
    - SECRET_KEY: більше НЕ має хардкод-дефолту!
      Значення обов'язково читається з .env або середовища.
    - Якщо змінні відсутні — Settings підніме помилку при старті.

🌐 Підключення до БД (оновлено 2026-08-01, контейнеризація):
    Підтримуються ДВА способи налаштування:
    1. DATABASE_URL — прямий DSN (override), напр. для Docker Compose:
       postgresql+asyncpg://user:pass@db:5432/pos_system
    2. DB_HOST/DB_PORT/DB_USER/DB_PASSWORD/DB_NAME — компоненти,
       з яких DSN будується автоматично (локальний запуск).
    Якщо задано DATABASE_URL — він має пріоритет.
"""

from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict
from pathlib import Path


class Settings(BaseSettings):
    # ──────────────────────────────────────────────
    # База даних
    # ──────────────────────────────────────────────
    # Прямий DSN (override). Якщо заданий — використовується замість DB_*.
    # Приклад: postgresql+asyncpg://postgres:pass@db:5432/pos_system
    database_url_override: str | None = Field(default=None, alias="DATABASE_URL")

    DB_HOST: str = "localhost"
    DB_PORT: int = 5432
    DB_USER: str = "postgres"
    # 🔐 Обов'язкова змінна — читається з .env або середовища.
    # Хардкод-дефолт ПРИБРАНО з міркувань безпеки (секрет був у git).
    DB_PASSWORD: str = ""
    DB_NAME: str = "pos_system"

    @field_validator("DB_PASSWORD")
    @classmethod
    def validate_db_password(cls, v: str) -> str:
        """Валідація DB_PASSWORD: не може бути порожньою."""
        if not v:
            raise ValueError(
                "DB_PASSWORD is required! "
                "Set it in .env file: DB_PASSWORD=<your_password>"
            )
        return v

    @property
    def DATABASE_URL(self) -> str:
        """
        Асинхронний DSN для SQLAlchemy + asyncpg.

        Пріоритет: DATABASE_URL (прямий DSN) → DB_* компоненти.
        """
        if self.database_url_override:
            return self.database_url_override
        return (
            f"postgresql+asyncpg://{self.DB_USER}:{self.DB_PASSWORD}"
            f"@{self.DB_HOST}:{self.DB_PORT}/{self.DB_NAME}"
        )

    @property
    def DATABASE_URL_SYNC(self) -> str:
        """
        Синхронний DSN (для Alembic).

        Якщо задано DATABASE_URL — конвертуємо драйвер asyncpg → psycopg2.
        """
        if self.database_url_override:
            return self.database_url_override.replace("+asyncpg", "+psycopg2")
        return (
            f"postgresql://{self.DB_USER}:{self.DB_PASSWORD}"
            f"@{self.DB_HOST}:{self.DB_PORT}/{self.DB_NAME}"
        )

    # ──────────────────────────────────────────────
    # Безпека
    # ──────────────────────────────────────────────
    # 🔐 Обов'язкова змінна — читається з .env або середовища.
    # Хардкод-дефолт ПРИБРАНО з міркувань безпеки (секрет був у git).
    SECRET_KEY: str = ""
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 480  # 8 годин
    REFRESH_TOKEN_EXPIRE_MINUTES: int = 10080  # 7 днів

    @field_validator("SECRET_KEY")
    @classmethod
    def validate_secret_key(cls, v: str) -> str:
        """Валідація SECRET_KEY: не порожній та має довжину >= 32."""
        if not v:
            raise ValueError(
                "SECRET_KEY is required! "
                "Set it in .env file. Generate with: openssl rand -hex 32"
            )
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
    CACHE_TTL_PRODUCTS: int = 30          # 30с (списки продуктів — гарячий read)
    CACHE_TTL_PRODUCT_DETAIL: int = 60    # 60с (деталі продукту)
    CACHE_TTL_BARCODE: int = 30           # 30с (пошук за штрих-кодом)
    CACHE_TTL_CATEGORIES: int = 60        # 60с (категорії)
    CACHE_TTL_LEDGER: int = 30            # 30с (ledger: історія та баланси)
    CACHE_TTL_INVOICES: int = 30          # 30с (списки/деталі накладних)
    CACHE_TTL_INVOICE_PRICE: int = 60     # 60с (зміни цін у накладній)
    CACHE_TTL_RECEIPTS: int = 15          # 15с (списки чеків — швидко міняються)
    CACHE_TTL_RECEIPT_STATS: int = 10     # 10с (статистика за сьогодні)
    CACHE_TTL_RECEIPT_DETAIL: int = 60    # 60с (чек за ID, позиції, повернення)

    # ──────────────────────────────────────────────
    # ПРРО (програмний РРО) — фіскалізація чеків через ДПС України
    # ──────────────────────────────────────────────
    PRRO_TEST_URL: str = "cabinet.tax.gov.ua:9443"   # Тестове API (чеки НЕ фіскальні)
    PRRO_PROD_URL: str = "prro.tax.gov.ua:443"        # Бойове API
    PRRO_MODE: str = "test"                           # "test" або "prod"
    PRRO_USE_SSL: bool = True                           # TLS-з'єднання
    PRRO_TIMEOUT_SECONDS: float = 30.0                  # Таймаут gRPC-виклику
    PRRO_MAX_RETRIES: int = 3                           # Кількість спроб з ретраями

    @property
    def PRRO_URL(self) -> str:
        """Повертає URL фіскального сервера залежно від PRRO_MODE."""
        return self.PRRO_PROD_URL if self.PRRO_MODE == "prod" else self.PRRO_TEST_URL

    # ──────────────────────────────────────────────
    # Налаштування застосунку
    # ──────────────────────────────────────────────
    APP_NAME: str = "Kasa POS"
    DEBUG: bool = False

    model_config = SettingsConfigDict(
        # 🔧 .env знаходиться в корені backend/ (рівень вище від app/)
        env_file=Path(__file__).resolve().parent.parent / ".env",
        env_file_encoding="utf-8",
        case_sensitive=True,
        populate_by_name=True,
    )


# Глобальний екземпляр налаштувань
# ⚠️ При відсутності DB_PASSWORD або SECRET_KEY у .env — підніметься помилка при імпорті!
settings = Settings()
