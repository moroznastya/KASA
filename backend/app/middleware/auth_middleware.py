"""
Middleware для авторизації запитів.

Забезпечує:
  - Перевірку Bearer токена в заголовках
  - Додавання інформації про користувача в request.state
  - Обробку публічних ендпоінтів (логін, реєстрація)
  - Пропуск CORS preflight запитів (OPTIONS) без авторизації
"""

from fastapi import HTTPException, status
from fastapi.responses import JSONResponse

from app.domain.services.auth_service import AuthService


# Список публічних шляхів, які не потребують авторизації
PUBLIC_PATHS = {
    "/api/v1/auth/login",
    "/api/v1/auth/login-pin",
    "/api/v1/auth/refresh",
    "/api/v1/auth/users-list",
    "/api/v1/auth/verify",
    "/docs",
    "/redoc",
    "/openapi.json",
    "/health",
    "/uploads",
    "/",
}


class AuthMiddleware:
    """
    Middleware для перевірки авторизації запитів.

    Перевіряє наявність та валідність JWT токена в заголовку Authorization.
    Для публічних шляхів пропускає запит без перевірки.
    CORS preflight-запити (OPTIONS) пропускаються без авторизації,
    оскільки вони не мають Authorization-заголовка.
    """

    def __init__(self, app):
        """
        Ініціалізація middleware.

        Args:
            app: Екземпляр FastAPI застосунку.
        """
        self.app = app

    async def __call__(self, scope, receive, send):
        """
        Обробка вхідного запиту.

        Args:
            scope: ASGI scope.
            receive: Функція отримання даних.
            send: Функція відправки даних.
        """
        # Не-HTTP scope (lifespan, websocket) пропускаємо без обробки —
        # інакше middleware відповідає 401 на lifespan-старт і DI-контейнер
        # ніколи не ініціалізується.
        if scope.get("type") != "http":
            await self.app(scope, receive, send)
            return

        # Отримуємо метод та шлях запиту
        path = scope.get("path", "")
        method = scope.get("method", "")

        # CORS preflight запити (OPTIONS) не мають Authorization-заголовка.
        # Пропускаємо їх без авторизації, щоб CORSMiddleware міг додати
        # необхідні CORS-заголовки до відповіді.
        if method == "OPTIONS":
            await self.app(scope, receive, send)
            return

        # Перевіряємо чи шлях публічний
        if self._is_public_path(path):
            await self.app(scope, receive, send)
            return

        # Отримуємо заголовок Authorization
        headers = dict(scope.get("headers", []))
        auth_header = None

        for key, value in headers.items():
            if key == b"authorization":
                auth_header = value.decode("utf-8")
                break

        if not auth_header:
            response = JSONResponse(
                status_code=status.HTTP_401_UNAUTHORIZED,
                content={"detail": "Відсутній заголовок авторизації"},
            )
            await response(scope, receive, send)
            return

        # Перевіряємо формат токена
        if not auth_header.startswith("Bearer "):
            response = JSONResponse(
                status_code=status.HTTP_401_UNAUTHORIZED,
                content={"detail": "Невірний формат токена. Використовуйте Bearer"},
            )
            await response(scope, receive, send)
            return

        token = auth_header[7:]  # Видаляємо "Bearer "

        try:
            # Декодуємо токен
            payload = AuthService.decode_access_token(token)
            user_id = payload.get("sub")

            if not user_id:
                response = JSONResponse(
                    status_code=status.HTTP_401_UNAUTHORIZED,
                    content={"detail": "Недійсний токен"},
                )
                await response(scope, receive, send)
                return

            # Додаємо інформацію про користувача в scope
            scope["user_id"] = user_id
            scope["user_role"] = payload.get("role")

        except HTTPException as e:
            response = JSONResponse(
                status_code=e.status_code,
                content={"detail": e.detail},
            )
            await response(scope, receive, send)
            return

        # Продовжуємо обробку запиту
        await self.app(scope, receive, send)

    @staticmethod
    def _is_public_path(path: str) -> bool:
        """
        Перевіряє чи є шлях публічним (не потребує авторизації).

        Args:
            path: Шлях запиту.

        Returns:
            True якщо шлях публічний.
        """
        # Точний збіг
        if path in PUBLIC_PATHS:
            return True

        # Шляхи документації
        if path.startswith("/docs") or path.startswith("/redoc"):
            return True

        # Шлях OpenAPI
        if path.startswith("/openapi.json"):
            return True

        # Шлях логіну (може мати різні варіації)
        if "/auth/login" in path:
            return True

        # Статичні файли (зображення товарів)
        if path.startswith("/uploads/"):
            return True

        # Шляхи друку (аутентифікація на рівні ендпоінта через get_current_user_optional)
        if "/print" in path:
            return True

        return False
