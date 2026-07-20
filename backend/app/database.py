"""
Асинхронне підключення до PostgreSQL через SQLAlchemy 2.0 + asyncpg.

Використання:
    async with async_session() as session:
        result = await session.execute(select(Product))
"""

from sqlalchemy.ext.asyncio import (
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)
from sqlalchemy.orm import DeclarativeBase

from app.config import settings

# ──────────────────────────────────────────────
# Асинхронний двигун (engine)
# ──────────────────────────────────────────────
engine = create_async_engine(
    url=settings.DATABASE_URL,
    echo=settings.DEBUG,          # Логувати SQL-запити в режимі DEBUG
    pool_size=10,                 # Розмір пулу з'єднань
    max_overflow=20,              # Додаткові з'єднання при піковому навантаженні
    pool_pre_ping=True,           # Перевіряти з'єднання перед використанням
)

# ──────────────────────────────────────────────
# Фабрика асинхронних сесій
# ──────────────────────────────────────────────
async_session = async_sessionmaker(
    bind=engine,
    class_=AsyncSession,
    expire_on_commit=False,       # Не застарівати об'єкти після коміту
)


# ──────────────────────────────────────────────
# Базовий клас для всіх моделей (Declarative Base)
# ──────────────────────────────────────────────
class Base(DeclarativeBase):
    """Базовий клас для всіх ORM-моделей Kasa."""
    pass


async def get_session() -> AsyncSession:
    """
    Генератор асинхронної сесії для FastAPI Depends.

    Використання:
        @router.get("/products")
        async def list_products(session: AsyncSession = Depends(get_session)):
            ...
    """
    async with async_session() as session:
        try:
            yield session
            await session.commit()
        except Exception:
            await session.rollback()
            raise
        finally:
            await session.close()
