"""Фікстури для тестування репозиторіїв."""

from __future__ import annotations

import asyncio
from typing import AsyncGenerator

import pytest
import pytest_asyncio
from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)
from sqlalchemy import event
from sqlalchemy.dialects.postgresql import JSONB

from app.database import Base
from app.infrastructure.persistence.repositories import (
    SQLAlchemyProductRepository,
    SQLAlchemyInvoiceRepository,
    SQLAlchemyReceiptRepository,
    SQLAlchemyCategoryRepository,
    SQLAlchemySupplierRepository,
    SQLAlchemyUserRepository,
)


# ─── Заміна JSONB на JSON для SQLite ─────────────────────────────────────────

@event.listens_for(Base.metadata, "before_create")
def _replace_jsonb(target, connection, **kw):
    """Замінити JSONB на JSON для SQLite сумісності."""
    for table in target.tables.values():
        for column in table.columns:
            if isinstance(column.type, JSONB):
                from sqlalchemy import JSON
                column.type = JSON()


@pytest.fixture(scope="session")
def event_loop():
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


@pytest_asyncio.fixture
async def engine():
    engine = create_async_engine("sqlite+aiosqlite:///", echo=False)
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
    yield engine
    await engine.dispose()


@pytest_asyncio.fixture
async def session(engine) -> AsyncGenerator[AsyncSession, None]:
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
