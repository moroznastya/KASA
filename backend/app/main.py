"""
Головний файл FastAPI застосунку Kasa POS.

Підключає:
  - Всі API роутери v1
  - CORS middleware (через settings)
  - Swagger документацію
  - Middleware авторизації
  - Rate Limiting (slowapi)
  - DI Container та Event Bus (ініціалізація в lifespan)
  - Обробники помилок
"""

from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from slowapi import _rate_limit_exceeded_handler
from slowapi.errors import RateLimitExceeded
from slowapi.middleware import SlowAPIMiddleware

from app.config import settings
from app.api.v1 import api_v1_router
from app.middleware.auth_middleware import AuthMiddleware
from app.api.v1.users import limiter

# ─── Інфраструктурні компоненти ─────────────────────────────────────────────
import logging
logging.basicConfig(level=logging.WARNING, format='%(asctime)s [%(levelname)s] %(name)s: %(message)s')
from app.infrastructure.di import DIContainer, register_all_services
from app.infrastructure.event_bus import LocalEventBus


# ─── Глобальні екземпляри інфраструктури ────────────────────────────────────
"""
Глобальні екземпляри інфраструктурних компонентів.

Створюються на рівні модуля для доступу з роутерів та middleware.
Ініціалізуються в lifespan при старті застосунку.
"""
container: DIContainer | None = None
event_bus: LocalEventBus | None = None


def get_container() -> DIContainer:
    """Повертає глобальний DI Container (для Depends)."""
    if container is None:
        raise RuntimeError("DI Container not initialized. App may not be started yet.")
    return container


def get_event_bus() -> LocalEventBus:
    """Повертає глобальний Event Bus (для Depends)."""
    if event_bus is None:
        raise RuntimeError("Event Bus not initialized. App may not be started yet.")
    return event_bus


# ─── Опис застосунку для Swagger ─────────────────────────────────────────────
APP_DESCRIPTION = """
# Kasa POS — Система управління торгівлею

## Можливості API:
- **Товари**: CRUD, пошук за штрих-кодом, фільтрація
- **Категорії**: CRUD, ієрархічне дерево
- **Постачальники**: CRUD
- **Користувачі**: CRUD, авторизація (пароль/PIN)
- **Прибуткові накладні**: CRUD, підтвердження з оновленням залишків
- **Переміщення**: CRUD, підтвердження
- **Списання**: CRUD, автоматичне оновлення залишків
- **Повернення постачальнику**: CRUD, підтвердження
- **Чеки продажу**: створення, історія
- **Взаєморозрахунки**: журнал операцій, баланс постачальників

## Авторизація:
- Більшість ендпоінтів потребують Bearer токена
- Отримайте токен через `/api/v1/auth/login` або `/api/v1/auth/login-pin`
- Деякі операції (створення користувачів) доступні тільки admin
"""


# ─── Lifespan (заміна on_event) ──────────────────────────────────────────────
@asynccontextmanager
async def lifespan(app: FastAPI):
    """
    Управління життєвим циклом застосунку.

    При старті:
    - Ініціалізація DI Container
    - Ініціалізація Event Bus
    - Реєстрація всіх сервісів

    При завершенні:
    - Закриття з'єднань
    """
    global container, event_bus

    print(f"🚀 {settings.APP_NAME} запускається...")

    # ─── 1. Ініціалізація DI Container ──────────────────────────────────────
    container = DIContainer()
    register_all_services(container)
    print(f"   ✅ DI Container ініціалізовано: {len(container.registered_services)} сервісів")

    # ─── 2. Отримуємо Event Bus з контейнера ────────────────────────────────
    event_bus = container.resolve("event_bus")
    print(f"   ✅ Event Bus ініціалізовано")

    # ─── 3. Ініціалізація обробників подій (майбутнє) ──────────────────────
    # Тут будуть підписуватись обробники подій:
    # stock_handler = StockEventHandler(...)
    # event_bus.subscribe(InvoiceConfirmed, stock_handler.handle)
    # event_bus.subscribe(InvoiceConfirmed, ledger_handler.handle)

    print(f"   ✅ Інфраструктура готова")

    yield

    # ─── Завершення роботи ──────────────────────────────────────────────────
    print(f"👋 {settings.APP_NAME} завершує роботу.")

    # Очищаємо глобальні посилання
    container = None
    event_bus = None


# ─── Створення застосунку ────────────────────────────────────────────────────
app = FastAPI(
    title=settings.APP_NAME,
    description=APP_DESCRIPTION,
    version="1.0.0",
    docs_url="/docs",
    redoc_url="/redoc",
    openapi_url="/openapi.json",
    lifespan=lifespan,
)


# ─── Rate Limiting ───────────────────────────────────────────────────────────
app.state.limiter = limiter
app.add_exception_handler(RateLimitExceeded, _rate_limit_exceeded_handler)


# ─── CORS Middleware ──────────────────────────────────────────────────────────
# Використовуємо налаштування з config.py
cors_origins = settings.CORS_ORIGINS.split(",") if settings.CORS_ORIGINS else ["*"]

app.add_middleware(
    CORSMiddleware,
    allow_origins=cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


# ─── Auth Middleware ──────────────────────────────────────────────────────────
app.add_middleware(AuthMiddleware)


# ─── SlowAPI Middleware (для rate limiting) ───────────────────────────────────
app.add_middleware(SlowAPIMiddleware)


# ─── Підключення роутерів ────────────────────────────────────────────────────
app.include_router(api_v1_router)


# ─── Health Check ────────────────────────────────────────────────────────────
@app.get("/health", tags=["Система"])
async def health_check():
    """
    Перевірка стану сервера.

    Повертає статус OK, якщо сервер працює.
    """
    return {
        "status": "ok",
        "app": settings.APP_NAME,
        "version": "1.0.0",
    }


@app.get("/", tags=["Система"])
async def root():
    """Кореневий ендпоінт з інформацією про API."""
    return {
        "app": settings.APP_NAME,
        "version": "1.0.0",
        "docs": "/docs",
        "redoc": "/redoc",
    }


# ─── Глобальний обробник помилок ─────────────────────────────────────────────
@app.exception_handler(Exception)
async def global_exception_handler(request: Request, exc: Exception):
    """
    Глобальний обробник непередбачених помилок.

    Логує помилку та повертає 500.
    В DEBUG режимі додає тип помилки для зручності розробки.
    """
    # В майбутньому тут можна додати логування в файл/Sentry
    print(f"❌ Непередбачена помилка: {exc}")

    content = {
        "detail": "Внутрішня помилка сервера",
    }

    # Додаємо тип помилки тільки в DEBUG режимі
    if settings.DEBUG:
        content["type"] = type(exc).__name__

    return JSONResponse(
        status_code=500,
        content=content,
    )
