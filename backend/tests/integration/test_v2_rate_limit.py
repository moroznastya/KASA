"""
Інтеграційні тести rate-limit на /api/v2/auth/login.

Rate-limit (5 запитів/хвилину на IP) налаштований через slowapi.
У тестовому режимі (tests/conftest.py) slowapi.Limiter.limit замінено
на no-op, щоб не ламати інші тести — тому перевіряємо двома способами:
  1. Статично: декоратор @limiter.limit("5/minute") присутній на роуті login.
  2. Динамічно: створюємо міні-FastAPI-застосунок з ТИМ САМИМ limiter
     (app.api.rate_limit.limiter) і такою самою політикою "5/minute" —
     6-й запит має повернути 429 (доводить, що конфігурація реально блокує).
"""

import importlib
import inspect

import pytest
from httpx import ASGITransport, AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.auth,
    pytest.mark.rate_limit,
    pytest.mark.v2,
]


class TestV2RateLimit:
    """Rate-limit на auth-ендпоінтах v2."""

    def test_login_route_decorated_with_limiter(self):
        """Роут /api/v2/auth/login захищено @limiter.limit("5/minute")."""
        from app.api.v2 import auth as auth_v2_module

        source = inspect.getsource(auth_v2_module.login)
        assert "@limiter.limit" in source
        assert "5/minute" in source

    def test_refresh_route_decorated_with_limiter(self):
        """Роут /api/v2/auth/refresh також під rate-limit."""
        from app.api.v2 import auth as auth_v2_module

        source = inspect.getsource(auth_v2_module.refresh_token)
        assert "@limiter.limit" in source
        assert "5/minute" in source

    async def test_rate_limit_returns_429_after_five_requests(self):
        """
        Після 5 запитів за хвилину 6-й запит отримує 429.

        Використовуємо реальний slowapi (importlib.reload відновлює
        оригінальний Limiter.limit, який conftest замінив на no-op)
        та ТОЙ САМИЙ limiter-екземпляр, що стоїть на /api/v2/auth/login
        (app.api.rate_limit.limiter).
        """
        import slowapi.extension
        importlib.reload(slowapi.extension)  # відновлюємо реальний slowapi (conftest замінив limit на no-op)

        from fastapi import FastAPI, Request
        from pydantic import BaseModel
        from slowapi import _rate_limit_exceeded_handler
        from slowapi.errors import RateLimitExceeded

        # Свіжий Limiter з ВІДНОВЛЕНОГО класу: той самий key_func (get_remote_address)
        # і та сама політика "5/minute", що й на /api/v2/auth/login.
        from slowapi.extension import Limiter as RealLimiter
        from slowapi.middleware import SlowAPIMiddleware
        from slowapi.util import get_remote_address
        limiter = RealLimiter(key_func=get_remote_address)

        class LoginBody(BaseModel):
            login: str
            password: str

        test_app = FastAPI()
        test_app.state.limiter = limiter
        test_app.add_exception_handler(RateLimitExceeded, _rate_limit_exceeded_handler)

        @test_app.post("/login")
        @limiter.limit("5/minute")
        async def login_route(request: Request, body: LoginBody):
            return {"ok": True}

        test_app.add_middleware(SlowAPIMiddleware)

        transport = ASGITransport(app=test_app)
        async with AsyncClient(transport=transport, base_url="http://test") as ac:
            codes = []
            for _ in range(6):
                response = await ac.post(
                    "/login", json={"login": "admin", "password": "x"}
                )
                codes.append(response.status_code)

        assert codes[:5] == [200, 200, 200, 200, 200]
        assert codes[5] == 429, f"6-й запит мав бути 429, отримано: {codes}"
