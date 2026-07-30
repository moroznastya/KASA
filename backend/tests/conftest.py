"""
Глобальні фікстури для тестування Kasa POS.

Надає:
  - test_db_url — тестова БД (kasa_test)
  - engine — create_async_engine + create_all + drop_all
  - session — ізольована сесія для кожного тесту
  - client — AsyncClient з FastAPI app + override залежностей
  - admin_token — JWT токен адміністратора
  - cashier_token — JWT токен касира
"""

import asyncio
from typing import AsyncGenerator
from uuid import uuid4

import pytest
import pytest_asyncio
from httpx import AsyncClient, ASGITransport
from sqlalchemy import text
from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
    AsyncEngine,
)

from app.database import Base, get_session
from app.main import app
from app.infrastructure.persistence.models.user import User, UserRole
from app.domain.services.auth_service import AuthService

# ─── Тестова БД ──────────────────────────────────────────────────────────────
TEST_DB_URL = "postgresql+asyncpg://kasa_user:kasa_pass@localhost:5432/kasa_test"


# ─── Фікстури сесії ──────────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def event_loop():
    """Створює єдиний event loop для всієї тестової сесії."""
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


@pytest_asyncio.fixture(scope="session")
async def engine() -> AsyncEngine:
    """
    Створює асинхронний двигун для тестової БД.

    При старті: створює всі таблиці (create_all)
    При завершенні: видаляє всі таблиці (drop_all)
    """
    engine = create_async_engine(TEST_DB_URL, echo=False)

    # Створюємо всі таблиці
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)

    yield engine

    # Видаляємо всі таблиці після завершення тестів
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.drop_all)

    await engine.dispose()


@pytest_asyncio.fixture
async def session(engine: AsyncEngine) -> AsyncGenerator[AsyncSession, None]:
    """
    Створює ізольовану сесію для кожного тесту.

    Після тесту відкочує всі зміни (rollback),
    щоб кожен тест починав з чистого стану.
    """
    session_maker = async_sessionmaker(
        bind=engine,
        class_=AsyncSession,
        expire_on_commit=False,
    )

    async with session_maker() as session:
        # Починаємо транзакцію
        await session.begin()
        try:
            yield session
        finally:
            # Відкочуємо всі зміни після тесту
            await session.rollback()


# ─── Фікстура HTTP клієнта ───────────────────────────────────────────────────

@pytest_asyncio.fixture
async def client(session: AsyncSession) -> AsyncGenerator[AsyncClient, None]:
    """
    Створює AsyncClient для тестування FastAPI ендпоінтів.

    Підміняє dependency get_session на тестову сесію,
    щоб всі запити використовували тестову БД.
    """

    # Функція-замінник для get_session
    async def override_get_session():
        yield session

    # Підміняємо залежність
    app.dependency_overrides[get_session] = override_get_session

    # Створюємо клієнт
    transport = ASGITransport(app=app)
    async with AsyncClient(
        transport=transport,
        base_url="http://test",
    ) as ac:
        yield ac

    # Очищаємо підміну залежностей
    app.dependency_overrides.clear()


# ─── Фікстури користувачів та токенів ───────────────────────────────────────

@pytest_asyncio.fixture
async def admin_user(session: AsyncSession) -> User:
    """
    Створює тестового адміністратора.

    Login: admin
    Password: admin123
    PIN: 1111
    """
    user = User(
        id=uuid4(),
        name="Тестовий Адміністратор",
        login="admin",
        password_hash=AuthService.hash_password("admin123"),
        pin_code=AuthService.hash_password("1111"),
        role=UserRole.ADMIN,
        is_active=True,
    )
    session.add(user)
    await session.flush()
    return user


@pytest_asyncio.fixture
async def cashier_user(session: AsyncSession) -> User:
    """
    Створює тестового касира.

    Login: cashier
    Password: cashier123
    PIN: 2222
    """
    user = User(
        id=uuid4(),
        name="Тестовий Касир",
        login="cashier",
        password_hash=AuthService.hash_password("cashier123"),
        pin_code=AuthService.hash_password("2222"),
        role=UserRole.CASHIER,
        is_active=True,
    )
    session.add(user)
    await session.flush()
    return user


@pytest_asyncio.fixture
async def admin_token(
    client: AsyncClient,
    admin_user: User,
) -> str:
    """
    Отримує JWT токен адміністратора через API логіну.

    Використовує реальний ендпоінт /api/v1/auth/login
    для отримання токена, щоб тестувати також і авторизацію.
    """
    response = await client.post(
        "/api/v1/auth/login",
        json={"login": "admin", "password": "admin123"},
    )
    assert response.status_code == 200, f"Логін admin не вдався: {response.text}"
    return response.json()["access_token"]


@pytest_asyncio.fixture
async def cashier_token(
    client: AsyncClient,
    cashier_user: User,
) -> str:
    """
    Отримує JWT токен касира через API логіну.
    """
    response = await client.post(
        "/api/v1/auth/login",
        json={"login": "cashier", "password": "cashier123"},
    )
    assert response.status_code == 200, f"Логін cashier не вдався: {response.text}"
    return response.json()["access_token"]


@pytest_asyncio.fixture
async def auth_headers(admin_token: str) -> dict:
    """Заголовки авторизації для admin."""
    return {"Authorization": f"Bearer {admin_token}"}


@pytest_asyncio.fixture
async def cashier_headers(cashier_token: str) -> dict:
    """Заголовки авторизації для cashier."""
    return {"Authorization": f"Bearer {cashier_token}"}
