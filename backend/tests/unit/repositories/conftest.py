"""Фікстури для тестування репозиторіїв.

Використовує SQLite in-memory базу для швидких тестів.
Підміняє PostgreSQL-специфічні типи (JSONB) на JSON для сумісності.
"""

from __future__ import annotations

import asyncio
from typing import AsyncGenerator

import pytest
import pytest_asyncio
from sqlalchemy import event
from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)
from sqlalchemy.pool import StaticPool

# ─── JSONB сумісність з SQLite ───────────────────────────────────────────────
# PostgreSQL JSONB тип не підтримується SQLite. Реєструємо компілятор.
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.ext.compiler import compiles


@compiles(JSONB, "sqlite")
def _compile_jsonb_sqlite(type_, compiler, **kw):
    """Замінити JSONB на JSON при роботі з SQLite."""
    return "JSON"


# ─── Імпорти моделей та репозиторіїв ─────────────────────────────────────────

from app.infrastructure.persistence.models import Base
from app.infrastructure.persistence.repositories import (
    SQLAlchemyProductRepository,
    SQLAlchemyInvoiceRepository,
    SQLAlchemyReceiptRepository,
    SQLAlchemyCategoryRepository,
    SQLAlchemySupplierRepository,
    SQLAlchemyUserRepository,
)


@pytest.fixture(scope="session")
def event_loop():
    """Створює event loop для async тестів."""
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


@pytest_asyncio.fixture
async def engine():
    """
    Створює SQLite in-memory engine для тестування.
    """
    engine = create_async_engine(
        "sqlite+aiosqlite://",
        echo=False,
        poolclass=StaticPool,
        connect_args={"check_same_thread": False},
    )

    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)

    yield engine
    await engine.dispose()


@pytest_asyncio.fixture
async def session(engine) -> AsyncGenerator[AsyncSession, None]:
    """Створює асинхронну сесію для тестування."""
    async_session = async_sessionmaker(engine, expire_on_commit=False)
    async with async_session() as session:
        yield session


@pytest_asyncio.fixture
async def product_repo(session: AsyncSession) -> SQLAlchemyProductRepository:
    return SQLAlchemyProductRepository(session)


@pytest_asyncio.fixture
async def invoice_repo(session: AsyncSession) -> SQLAlchemyInvoiceRepository:
    return SQLAlchemyInvoiceRepository(session)


@pytest_asyncio.fixture
async def receipt_repo(session: AsyncSession) -> SQLAlchemyReceiptRepository:
    return SQLAlchemyReceiptRepository(session)


@pytest_asyncio.fixture
async def category_repo(session: AsyncSession) -> SQLAlchemyCategoryRepository:
    return SQLAlchemyCategoryRepository(session)


@pytest_asyncio.fixture
async def supplier_repo(session: AsyncSession) -> SQLAlchemySupplierRepository:
    return SQLAlchemySupplierRepository(session)


@pytest_asyncio.fixture
async def user_repo(session: AsyncSession) -> SQLAlchemyUserRepository:
    return SQLAlchemyUserRepository(session)
