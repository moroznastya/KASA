"""
Інтеграційні тести авторизації API v2 (end-to-end: HTTP → AuthUseCases → БД).

Покриває флоу:
  - Логін через POST /api/v2/auth/login → отримання JWT-токена
  - Доступ до /api/v2/products З токеном → 200
  - Доступ до /api/v2/products БЕЗ токена → 401
  - Логін з невірним паролем → 401
  - Логін за PIN-кодом (v2)
"""

import pytest
from httpx import AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.auth,
    pytest.mark.v2,
]


class TestV2AuthFlow:
    """Авторизація API v2: логін → токен → захищені ендпоінти."""

    async def test_login_returns_token_and_user(
        self, client: AsyncClient, admin_user
    ):
        """Логін адміністратора через v2: токен + профіль користувача."""
        response = await client.post(
            "/api/v2/auth/login",
            json={"login": "admin", "password": "admin123"},
        )
        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["token_type"] == "bearer"
        assert data["user"]["login"] == "admin"
        assert data["user"]["role"] == "admin"

    async def test_products_accessible_with_token(
        self, client: AsyncClient, admin_user
    ):
        """Доступ до /api/v2/products з валідним токеном → 200."""
        login = await client.post(
            "/api/v2/auth/login",
            json={"login": "admin", "password": "admin123"},
        )
        token = login.json()["access_token"]
        headers = {"Authorization": f"Bearer {token}"}

        response = await client.get("/api/v2/products", headers=headers)
        assert response.status_code == 200
        data = response.json()
        assert "items" in data
        assert "total" in data

    async def test_products_denied_without_token(self, client: AsyncClient):
        """Доступ до /api/v2/products без токена → 401."""
        response = await client.get("/api/v2/products")
        assert response.status_code == 401

    async def test_login_wrong_password_returns_401(
        self, client: AsyncClient, admin_user
    ):
        """Логін з невірним паролем через v2 → 401."""
        response = await client.post(
            "/api/v2/auth/login",
            json={"login": "admin", "password": "wrong_password"},
        )
        assert response.status_code == 401

    async def test_login_pin_returns_token(
        self, client: AsyncClient, admin_user
    ):
        """Логін за PIN-кодом через v2 → токен."""
        response = await client.post(
            "/api/v2/auth/login-pin",
            json={"login": "admin", "pin_code": "1111"},
        )
        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["user"]["login"] == "admin"

    async def test_login_pin_wrong_pin_returns_401(
        self, client: AsyncClient, admin_user
    ):
        """Логін з невірним PIN через v2 → 401."""
        response = await client.post(
            "/api/v2/auth/login-pin",
            json={"login": "admin", "pin_code": "0000"},
        )
        assert response.status_code == 401
