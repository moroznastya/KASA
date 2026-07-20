"""
Інтеграційні тести авторизації та рольового доступу.

Сценарій 8: Авторизація та ролі

Перевіряє:
  - Логін за паролем (успіх/невдача)
  - Логін за PIN-кодом
  - Доступ без токена (401)
  - Доступ cashier до admin-ендпоінтів (403)
  - Доступ admin до admin-ендпоінтів (200)
"""

import pytest
from httpx import AsyncClient


pytestmark = [
    pytest.mark.integration,
    pytest.mark.auth,
]


# ─── Логін за паролем ────────────────────────────────────────────────────────

class TestLoginByPassword:
    """Тести логіну за паролем."""

    async def test_login_success_admin(self, client: AsyncClient, admin_user):
        """Успішний логін адміністратора."""
        response = await client.post(
            "/api/v1/auth/login",
            json={"login": "admin", "password": "admin123"},
        )
        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["token_type"] == "bearer"
        assert data["user"]["login"] == "admin"
        assert data["user"]["role"] == "admin"

    async def test_login_success_cashier(self, client: AsyncClient, cashier_user):
        """Успішний логін касира."""
        response = await client.post(
            "/api/v1/auth/login",
            json={"login": "cashier", "password": "cashier123"},
        )
        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["user"]["login"] == "cashier"
        assert data["user"]["role"] == "cashier"

    async def test_login_wrong_password(self, client: AsyncClient, admin_user):
        """Логін з невірним паролем."""
        response = await client.post(
            "/api/v1/auth/login",
            json={"login": "admin", "password": "wrong_password"},
        )
        assert response.status_code == 401
        data = response.json()
        assert "detail" in data

    async def test_login_wrong_login(self, client: AsyncClient):
        """Логін з неіснуючим логіном."""
        response = await client.post(
            "/api/v1/auth/login",
            json={"login": "nonexistent", "password": "test123"},
        )
        assert response.status_code == 401

    async def test_login_empty_password(self, client: AsyncClient, admin_user):
        """Логін з порожнім паролем."""
        response = await client.post(
            "/api/v1/auth/login",
            json={"login": "admin", "password": ""},
        )
        assert response.status_code == 401


# ─── Логін за PIN-кодом ──────────────────────────────────────────────────────

class TestLoginByPin:
    """Тести логіну за PIN-кодом."""

    async def test_login_pin_success_admin(self, client: AsyncClient, admin_user):
        """Успішний логін адміністратора за PIN."""
        response = await client.post(
            "/api/v1/auth/login-pin",
            json={"login": "admin", "pin_code": "1111"},
        )
        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["user"]["login"] == "admin"

    async def test_login_pin_success_cashier(self, client: AsyncClient, cashier_user):
        """Успішний логін касира за PIN."""
        response = await client.post(
            "/api/v1/auth/login-pin",
            json={"login": "cashier", "pin_code": "2222"},
        )
        assert response.status_code == 200
        data = response.json()
        assert "access_token" in data
        assert data["user"]["login"] == "cashier"

    async def test_login_pin_wrong_pin(self, client: AsyncClient, admin_user):
        """Логін з невірним PIN-кодом."""
        response = await client.post(
            "/api/v1/auth/login-pin",
            json={"login": "admin", "pin_code": "0000"},
        )
        assert response.status_code == 401

    async def test_login_pin_user_without_pin(self, client: AsyncClient, session):
        """Логін користувача без PIN-коду."""
        from app.models.user import User, UserRole
        from uuid import uuid4
        from app.services.auth_service import AuthService

        user = User(
            id=uuid4(),
            name="Без PIN",
            login="no_pin_user",
            password_hash=AuthService.hash_password("test123"),
            pin_code=None,
            role=UserRole.CASHIER,
            is_active=True,
        )
        session.add(user)
        await session.flush()

        response = await client.post(
            "/api/v1/auth/login-pin",
            json={"login": "no_pin_user", "pin_code": "1111"},
        )
        assert response.status_code == 401


# ─── Доступ без авторизації ──────────────────────────────────────────────────

class TestAccessWithoutAuth:
    """Тести доступу до ендпоінтів без токена."""

    async def test_products_without_token(self, client: AsyncClient):
        """Доступ до /products без токена."""
        response = await client.get("/api/v1/products")
        assert response.status_code == 401

    async def test_users_without_token(self, client: AsyncClient):
        """Доступ до /users без токена."""
        response = await client.get("/api/v1/users")
        assert response.status_code == 401

    async def test_invoices_without_token(self, client: AsyncClient):
        """Доступ до /invoices без токена."""
        response = await client.get("/api/v1/invoices")
        assert response.status_code == 401

    async def test_receipts_without_token(self, client: AsyncClient):
        """Доступ до /receipts без токена."""
        response = await client.get("/api/v1/receipts")
        assert response.status_code == 401

    async def test_health_without_token(self, client: AsyncClient):
        """Доступ до /health без токена (публічний ендпоінт)."""
        response = await client.get("/health")
        assert response.status_code == 200


# ─── Рольовий доступ ─────────────────────────────────────────────────────────

class TestRoleBasedAccess:
    """Тести рольового доступу (admin vs cashier)."""

    async def test_cashier_cannot_list_users(
        self, client: AsyncClient, cashier_headers: dict
    ):
        """Касир не може отримати список користувачів."""
        response = await client.get(
            "/api/v1/users",
            headers=cashier_headers,
        )
        assert response.status_code == 403

    async def test_admin_can_list_users(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Адмін може отримати список користувачів."""
        response = await client.get(
            "/api/v1/users",
            headers=auth_headers,
        )
        assert response.status_code == 200
        data = response.json()
        assert isinstance(data, list)

    async def test_cashier_cannot_create_user(
        self, client: AsyncClient, cashier_headers: dict
    ):
        """Касир не може створити користувача."""
        response = await client.post(
            "/api/v1/users",
            headers=cashier_headers,
            json={
                "name": "Новий",
                "login": "new_user",
                "password": "test123",
                "role": "cashier",
            },
        )
        assert response.status_code == 403

    async def test_admin_can_create_user(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Адмін може створити користувача."""
        response = await client.post(
            "/api/v1/users",
            headers=auth_headers,
            json={
                "name": "Новий Користувач",
                "login": "new_user",
                "password": "test123",
                "role": "cashier",
            },
        )
        assert response.status_code == 201
        data = response.json()
        assert data["login"] == "new_user"

    async def test_cashier_can_access_products(
        self, client: AsyncClient, cashier_headers: dict
    ):
        """Касир може отримати список товарів."""
        response = await client.get(
            "/api/v1/products",
            headers=cashier_headers,
        )
        assert response.status_code == 200

    async def test_cashier_can_access_receipts(
        self, client: AsyncClient, cashier_headers: dict
    ):
        """Касир може створити чек."""
        response = await client.get(
            "/api/v1/receipts",
            headers=cashier_headers,
        )
        assert response.status_code == 200


# ─── Неактивний користувач ───────────────────────────────────────────────────

class TestInactiveUser:
    """Тести для неактивного користувача."""

    async def test_inactive_user_cannot_login(self, client: AsyncClient, session):
        """Неактивний користувач не може увійти."""
        from app.models.user import User, UserRole
        from uuid import uuid4
        from app.services.auth_service import AuthService

        user = User(
            id=uuid4(),
            name="Неактивний",
            login="inactive_user",
            password_hash=AuthService.hash_password("test123"),
            role=UserRole.CASHIER,
            is_active=False,
        )
        session.add(user)
        await session.flush()

        response = await client.post(
            "/api/v1/auth/login",
            json={"login": "inactive_user", "password": "test123"},
        )
        assert response.status_code == 403
