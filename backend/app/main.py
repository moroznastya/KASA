"""
Головний файл FastAPI застосунку Kasa POS.

Підключає:
  - Всі API роутери v1 та v2
  - CORS middleware (через settings) — зовнішній шар
  - Swagger документацію
  - Middleware авторизації — внутрішній шар
  - Rate Limiting (slowapi)
  - DI Container та Event Bus (ініціалізація в lifespan)
  - Обробники помилок
  - Graceful shutdown для Redis cache
"""

from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from slowapi import _rate_limit_exceeded_handler
from slowapi.errors import RateLimitExceeded
from slowapi.middleware import SlowAPIMiddleware
from fastapi.staticfiles import StaticFiles

from app.config import settings
from app.api.v1 import api_v1_router
from app.api.v2 import router as v2_router
from app.middleware.auth_middleware import AuthMiddleware
from app.api.v1.users import limiter

# ─── Інфраструктурні компоненти ─────────────────────────────────────────────
import logging
logging.basicConfig(level=logging.WARNING, format='%(asctime)s [%(levelname)s] %(name)s: %(message)s')

# Діагностичні логи рендеру цінників/етикеток (INFO)
logging.getLogger("app.infrastructure.services.price_tag_print_service").setLevel(logging.INFO)
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
    - Реєстрація всіх сервісів (включно з Redis cache)

    При завершенні:
    - Graceful shutdown Redis cache connection
    - Очищення глобальних посилань
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

    # ─── 3. Зберігаємо контейнер в стані застосунку ─────────────────────────
    app.state.di_container = container

    print(f"   ✅ Інфраструктура готова")

    yield

    # ─── Graceful Shutdown ──────────────────────────────────────────────────
    print(f"👋 {settings.APP_NAME} завершує роботу...")

    # Закриваємо з'єднання з Redis (якщо є)
    try:
        if container and container.has("cache_service"):
            cache_service = container.resolve("cache_service")
            await cache_service.close()
            print("   ✅ Redis cache connection closed")
    except Exception as e:
        print(f"   ⚠️  Помилка при закритті Redis: {e}")

    # Закриваємо gRPC-канали ПРРО
    try:
        from app.infrastructure.di.prro import close_prro_service_factory
        await close_prro_service_factory()
        print("   ✅ ПРРО gRPC-канали закрито")
    except Exception as e:
        print(f"   ⚠️  Помилка при закритті ПРРО каналів: {e}")

    # Очищаємо глобальні посилання
    container = None
    event_bus = None
    print(f"   ✅ Cleanup завершено")


# ─── Створення застосунку ────────────────────────────────────────────────────
app = FastAPI(
    redirect_slashes=False,
    title=settings.APP_NAME,
    description=APP_DESCRIPTION,
    version="2.0.0",
    docs_url="/docs",
    redoc_url="/redoc",
    openapi_url="/openapi.json",
    lifespan=lifespan,
)


# ─── Rate Limiting ───────────────────────────────────────────────────────────
app.state.limiter = limiter
app.add_exception_handler(RateLimitExceeded, _rate_limit_exceeded_handler)


# ─── Auth Middleware ──────────────────────────────────────────────────────────
# Додається першим, щоб бути внутрішнім шаром.
# CORS preflight-запити (OPTIONS) пропускаються без авторизації.
app.add_middleware(AuthMiddleware)


# ─── SlowAPI Middleware (для rate limiting) ───────────────────────────────────
app.add_middleware(SlowAPIMiddleware)


# ─── CORS Middleware (зовнішній шар — додається останнім) ─────────────────────
# CORSMiddleware має бути зовнішнім (доданий останнім), щоб обробляти
# CORS preflight-запити ДО того, як вони дійдуть до AuthMiddleware.
cors_origins = settings.CORS_ORIGINS.split(",") if settings.CORS_ORIGINS else ["*"]

app.add_middleware(
    CORSMiddleware,
    allow_origins=cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


# ─── Static Files (завантажені зображення) ────────────────────────────
app.mount("/uploads", StaticFiles(directory="uploads"), name="uploads")


# ─── Підключення роутерів ────────────────────────────────────────────────────
app.include_router(api_v1_router)
app.include_router(v2_router)


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
        "version": "2.0.0",
    }


@app.get("/", tags=["Система"])
async def root():
    """Кореневий ендпоінт з інформацією про API."""
    return {
        "app": settings.APP_NAME,
        "version": "2.0.0",
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
