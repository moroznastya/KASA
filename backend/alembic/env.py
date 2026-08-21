"""
Середовище виконання Alembic міграцій.

Підтримує асинхронний режим через asyncpg.
Використовує синхронний DSN для міграцій (Alembic працює синхронно).
"""

import asyncio
import sys
from pathlib import Path
from logging.config import fileConfig

from alembic import context
from sqlalchemy import pool
from sqlalchemy.engine import Connection
from sqlalchemy.ext.asyncio import async_engine_from_config

# ── Додаємо шлях до проєкту ────────────────────
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# ── Імпорт конфігурації застосунку ─────────────
from app.config import settings
from app.database import Base

# ── Імпорт усіх моделей (щоб Alembic бачив їх) ─
from app.infrastructure.persistence.models import (  # noqa: F401
    # Довідники
    User,
    Category,
    Supplier,
    # Товари
    Product,
    Barcode,
    ProductImage,
    # Документи
    Invoice, InvoiceItem,
    Transfer, TransferItem,
    WriteOff, WriteOffItem,
    ReturnInvoice, ReturnInvoiceItem,
    # Продажі
    Receipt, ReceiptItem,
    # Взаєморозрахунки
    SupplierLedger,
)

# ── Конфігурація Alembic ───────────────────────
config = context.config

# Встановлюємо URL бази даних з налаштувань застосунку
config.set_main_option("sqlalchemy.url", settings.DATABASE_URL_SYNC)

# Налаштування логування з alembic.ini
if config.config_file_name is not None:
    fileConfig(config.config_file_name)

# Метадані для автогенерації міграцій
target_metadata = Base.metadata


def run_migrations_offline() -> None:
    """
    Запуск міграцій в офлайн-режимі (без підключення до БД).

    Використовується для генерації SQL-скриптів.
    """
    url = config.get_main_option("sqlalchemy.url")
    context.configure(
        url=url,
        target_metadata=target_metadata,
        literal_binds=True,
        dialect_opts={"paramstyle": "named"},
        transaction_per_migration=True,
    )

    with context.begin_transaction():
        context.run_migrations()


def do_run_migrations(connection: Connection) -> None:
    """Виконання міграцій у переданому з'єднанні."""
    context.configure(
        connection=connection,
        target_metadata=target_metadata,
        transaction_per_migration=True,
    )

    with context.begin_transaction():
        context.run_migrations()


async def run_async_migrations() -> None:
    """
    Запуск міграцій в асинхронному режимі.

    Створює асинхронний двигун з конфігурації Alembic
    та виконує міграції.
    """
    configuration = config.get_section(config.config_ini_section, {})
    configuration["sqlalchemy.url"] = settings.DATABASE_URL

    connectable = async_engine_from_config(
        configuration,
        prefix="sqlalchemy.",
        poolclass=pool.NullPool,
    )

    async with connectable.connect() as connection:
        await connection.run_sync(do_run_migrations)

    await connectable.dispose()


def run_migrations_online() -> None:
    """
    Запуск міграцій в онлайн-режимі (з підключенням до БД).

    Використовує асинхронний run_async_migrations.
    """
    asyncio.run(run_async_migrations())


# Вибір режиму виконання
if context.is_offline_mode():
    run_migrations_offline()
else:
    run_migrations_online()
