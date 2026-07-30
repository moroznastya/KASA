"""
Глобальні фікстури для тестування Kasa POS — SQLite in-memory.

Надає:
  - engine — create_async_engine (SQLite in-memory, per-test)
  - session — ізольована сесія для кожного тесту
  - client — AsyncClient з FastAPI app + override залежностей
  - admin_token — JWT токен адміністратора
  - cashier_token — JWT токен касира
  - admin_user / cashier_user — тестові користувачі
  - auth_headers / cashier_headers — заголовки авторизації

Підхід:
  - Кожен тест отримує ЧИСТУ in-memory SQLite БД (engine scope="function")
  - Тестові дані створюються через фікстури з session.commit()
  - HTTP клієнт використовує ту саму сесію через dependency override
  - Rate limiter вимкнено (slowapi.Limiter.limit -> no-op)
  - PostgreSQL UUID підмінено на універсальний UUID для SQLite
  - SlowAPIMiddleware видалено з app
"""

from typing import AsyncGenerator
from uuid import uuid4

import pytest
import pytest_asyncio
from httpx import AsyncClient, ASGITransport
from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
    AsyncEngine,
)

# ─── Підміна PostgreSQL UUID на універсальний UUID для SQLite ──────────────
# Це дозволяє моделям, які імпортують UUID з postgresql, працювати з SQLite
import sqlalchemy.dialects.postgresql
from sqlalchemy import types
sqlalchemy.dialects.postgresql.UUID = types.UUID

# ─── Вимкнення slowapi rate limiter ──────────────────────────────────────────
import slowapi
slowapi.Limiter.limit = lambda self, *a, **kw: lambda f: f

from app.main import app
from slowapi.middleware import SlowAPIMiddleware
app.user_middleware = [
    m for m in app.user_middleware if m.cls is not SlowAPIMiddleware
]
app.middleware_stack = None

from app.database import Base, get_session
from app.infrastructure.persistence.models.user import User, UserRole
from app.domain.services.auth_service import AuthService


# ─── Тестова БД ──────────────────────────────────────────────────────────────
TEST_DB_URL = "sqlite+aiosqlite:///:memory:"


# ─── Фікстури сесії ──────────────────────────────────────────────────────────

@pytest_asyncio.fixture(scope="function")
async def engine() -> AsyncEngine:
    """Створює нову in-memory SQLite БД для кожного тесту."""
    engine = create_async_engine(TEST_DB_URL, echo=False)
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
    yield engine
    await engine.dispose()


@pytest_asyncio.fixture
async def session(engine: AsyncEngine) -> AsyncGenerator[AsyncSession, None]:
    """Створює ізольовану сесію для кожного тесту."""
    session_maker = async_sessionmaker(
        bind=engine, class_=AsyncSession, expire_on_commit=False,
    )
    async with session_maker() as session:
        yield session


# ─── Фікстура HTTP клієнта ───────────────────────────────────────────────────

@pytest_asyncio.fixture
async def client(session: AsyncSession) -> AsyncGenerator[AsyncClient, None]:
    """AsyncClient з override get_session."""

    async def override_get_session():
        yield session

    app.dependency_overrides[get_session] = override_get_session
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test", follow_redirects=True) as ac:
        yield ac
    app.dependency_overrides.clear()


# ─── Фікстури користувачів ───────────────────────────────────────────────────

async def _create_user(session: AsyncSession, **kwargs) -> User:
    user = User(
        id=uuid4(),
        name=kwargs["name"],
        login=kwargs["login"],
        password_hash=AuthService.hash_password(kwargs["password"]),
        pin_code=AuthService.hash_password(kwargs.get("pin", "0000")),
        role=kwargs["role"],
        is_active=kwargs.get("is_active", True),
    )
    session.add(user)
    await session.commit()
    return user


@pytest_asyncio.fixture
async def admin_user(session: AsyncSession) -> User:
    return await _create_user(
        session=session, name="Тестовий Адміністратор",
        login="admin", password="admin123", pin="1111",
        role=UserRole.ADMIN,
    )


@pytest_asyncio.fixture
async def cashier_user(session: AsyncSession) -> User:
    return await _create_user(
        session=session, name="Тестовий Касир",
        login="cashier", password="cashier123", pin="2222",
        role=UserRole.CASHIER,
    )


@pytest_asyncio.fixture
async def inactive_user(session: AsyncSession) -> User:
    return await _create_user(
        session=session, name="Неактивний Користувач",
        login="inactive", password="inactive123", pin="3333",
        role=UserRole.CASHIER, is_active=False,
    )


# ─── Фікстури токенів ────────────────────────────────────────────────────────

@pytest_asyncio.fixture
async def admin_token(client: AsyncClient, admin_user: User) -> str:
    response = await client.post(
        "/api/v1/auth/login",
        json={"login": "admin", "password": "admin123"},
    )
    assert response.status_code == 200, (
        f"Логін admin не вдався: {response.status_code} - {response.text}"
    )
    return response.json()["access_token"]


@pytest_asyncio.fixture
async def cashier_token(client: AsyncClient, cashier_user: User) -> str:
    response = await client.post(
        "/api/v1/auth/login",
        json={"login": "cashier", "password": "cashier123"},
    )
    assert response.status_code == 200, (
        f"Логін cashier не вдався: {response.status_code} - {response.text}"
    )
    return response.json()["access_token"]


# ─── Фікстури заголовків авторизації ─────────────────────────────────────────

@pytest_asyncio.fixture
async def auth_headers(admin_token: str) -> dict:
    return {"Authorization": f"Bearer {admin_token}"}


@pytest_asyncio.fixture
async def cashier_headers(cashier_token: str) -> dict:
    return {"Authorization": f"Bearer {cashier_token}"}
