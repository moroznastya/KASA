"""
Глобальні фікстури для тестування Torgashka POS — SQLite in-memory.

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

from collections.abc import AsyncGenerator
from uuid import uuid4

import pytest
import pytest_asyncio

# ─── Підміна PostgreSQL UUID на універсальний UUID для SQLite ──────────────
# Це дозволяє моделям, які імпортують UUID з postgresql, працювати з SQLite
import sqlalchemy.dialects.postgresql
from httpx import ASGITransport, AsyncClient
from sqlalchemy import types
from sqlalchemy.ext.asyncio import (
    AsyncEngine,
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)

sqlalchemy.dialects.postgresql.UUID = types.UUID

# ─── Вимкнення slowapi rate limiter ──────────────────────────────────────────
import slowapi

slowapi.Limiter.limit = lambda self, *a, **kw: lambda f: f

from slowapi.middleware import SlowAPIMiddleware

from app.main import app

app.user_middleware = [
    m for m in app.user_middleware if m.cls is not SlowAPIMiddleware
]
app.middleware_stack = None

from app.api.v2.deps import get_cache_service
from app.database import Base, get_session
from app.domain.services.auth_service import AuthService
from app.infrastructure.cache.memory_cache import MemoryCacheService
from app.infrastructure.persistence.models.user import User, UserRole

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


# ─── DI-контейнер для v2 ендпоінтів ──────────────────────────────────────────

@pytest.fixture(scope="session")
def di_container():
    """
    DI-контейнер для v2 ендпоінтів (app.state.di_container).

    V2 deps (app/api/v2/deps.py) резолвлять event_bus/cache_service
    через request.app.state.di_container, який у продакшні створюється
    у lifespan. Тут створюємо його явно (без запуску lifespan).
    """
    from app.infrastructure.di.container import DIContainer
    from app.infrastructure.di.service_registry import register_all_services

    container = DIContainer()
    register_all_services(container)
    return container


# ─── Фікстура HTTP клієнта ───────────────────────────────────────────────────

@pytest_asyncio.fixture
async def client(session: AsyncSession, di_container) -> AsyncGenerator[AsyncClient, None]:
    """AsyncClient з override get_session + DI-контейнер для v2."""

    async def override_get_session():
        yield session

    app.dependency_overrides[get_session] = override_get_session

    # V2 deps вимагають app.state.di_container (у продакшні — з lifespan)
    app.state.di_container = di_container

    # Кешування: in-memory MemoryCacheService — ізоляція від Redis у тестах
    test_cache = MemoryCacheService(default_ttl=60)

    async def override_get_cache_service():
        return test_cache

    app.dependency_overrides[get_cache_service] = override_get_cache_service

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
