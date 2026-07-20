"""
Головний файл FastAPI застосунку Kasa POS.

Підключає:
  - Всі API роутери v1
  - CORS middleware
  - Swagger документацію
  - Middleware авторизації
  - Обробники помилок
"""

from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from app.config import settings
from app.api.v1 import api_v1_router
from app.middleware.auth_middleware import AuthMiddleware


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

    При старті: ініціалізація (якщо потрібно).
    При завершенні: закриття з'єднань.
    """
    # Старт
    print(f"🚀 {settings.APP_NAME} запускається...")
    yield
    # Завершення
    print(f"👋 {settings.APP_NAME} завершує роботу.")


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


# ─── CORS Middleware ──────────────────────────────────────────────────────────
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],  # В продакшені замінити на конкретні домени
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


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
    """
    # В майбутньому тут можна додати логування в файл/Sentry
    print(f"❌ Непередбачена помилка: {exc}")
    return JSONResponse(
        status_code=500,
        content={
            "detail": "Внутрішня помилка сервера",
            "type": type(exc).__name__,
        },
    )
