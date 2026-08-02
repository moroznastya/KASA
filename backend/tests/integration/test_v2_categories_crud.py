"""
Інтеграційні тести CRUD категорій через API v2.

Покриває флоу (HTTP → CategoryRepository → БД):
  - Створення категорії (POST /api/v2/categories) → 201
  - Отримання категорії за ID (GET /api/v2/categories/{id}) → 200
  - Список категорій (GET /api/v2/categories) → 200
  - Дерево категорій (GET /api/v2/categories/tree) → 200
  - Оновлення категорії (PUT /api/v2/categories/{id}) → 200
  - Видалення категорії (DELETE /api/v2/categories/{id}) → 204 → 404
  - Дублікат назви → 400
"""

import pytest
from httpx import AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.categories,
    pytest.mark.v2,
]


class TestV2CategoriesCRUD:
    """Повний CRUD-цикл категорії через API v2."""

    async def test_create_category(self, client: AsyncClient, auth_headers: dict):
        """Створення категорії → 201, дані збережено в БД."""
        response = await client.post(
            "/api/v2/categories",
            headers=auth_headers,
            json={"name": "Напої", "description": "Всі напої"},
        )
        assert response.status_code == 201
        data = response.json()
        assert data["name"] == "Напої"
        assert data["description"] == "Всі напої"
        assert data["id"]

    async def test_get_category_by_id(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Отримання категорії за ID → 200."""
        created = await client.post(
            "/api/v2/categories",
            headers=auth_headers,
            json={"name": "Молочні продукти"},
        )
        category_id = created.json()["id"]

        response = await client.get(
            f"/api/v2/categories/{category_id}", headers=auth_headers
        )
        assert response.status_code == 200
        assert response.json()["name"] == "Молочні продукти"

    async def test_list_categories(self, client: AsyncClient, auth_headers: dict):
        """Список категорій → 200, містить створену."""
        await client.post(
            "/api/v2/categories", headers=auth_headers, json={"name": "Випічка"}
        )
        response = await client.get("/api/v2/categories", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["total"] >= 1
        assert any(c["name"] == "Випічка" for c in data["items"])

    async def test_category_tree(self, client: AsyncClient, auth_headers: dict):
        """Дерево категорій → 200, вкладена структура."""
        parent = await client.post(
            "/api/v2/categories", headers=auth_headers, json={"name": "Батьківська"}
        )
        parent_id = parent.json()["id"]
        await client.post(
            "/api/v2/categories",
            headers=auth_headers,
            json={"name": "Дочірня", "parent_id": parent_id},
        )
        response = await client.get("/api/v2/categories/tree", headers=auth_headers)
        assert response.status_code == 200
        tree = response.json()
        assert isinstance(tree, list)
        parent_node = next((n for n in tree if n["name"] == "Батьківська"), None)
        assert parent_node is not None
        assert any(c["name"] == "Дочірня" for c in parent_node["children"])

    async def test_update_category(self, client: AsyncClient, auth_headers: dict):
        """Оновлення назви категорії → 200, зміни в БД."""
        created = await client.post(
            "/api/v2/categories", headers=auth_headers, json={"name": "Стара назва"}
        )
        category_id = created.json()["id"]

        response = await client.put(
            f"/api/v2/categories/{category_id}",
            headers=auth_headers,
            json={"name": "Нова назва", "description": "Оновлено"},
        )
        assert response.status_code == 200
        assert response.json()["name"] == "Нова назва"

        # Зміни збереглися в БД
        check = await client.get(
            f"/api/v2/categories/{category_id}", headers=auth_headers
        )
        assert check.json()["name"] == "Нова назва"

    async def test_delete_category(self, client: AsyncClient, auth_headers: dict):
        """Видалення категорії → 204 → 404 після."""
        created = await client.post(
            "/api/v2/categories", headers=auth_headers, json={"name": "Для видалення"}
        )
        category_id = created.json()["id"]

        response = await client.delete(
            f"/api/v2/categories/{category_id}", headers=auth_headers
        )
        assert response.status_code == 204

        check = await client.get(
            f"/api/v2/categories/{category_id}", headers=auth_headers
        )
        assert check.status_code == 404

    async def test_create_duplicate_category_name(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Дублікат назви категорії → 400."""
        await client.post(
            "/api/v2/categories", headers=auth_headers, json={"name": "Унікальна"}
        )
        response = await client.post(
            "/api/v2/categories", headers=auth_headers, json={"name": "Унікальна"}
        )
        assert response.status_code == 400

    async def test_get_missing_category_returns_404(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Неіснуюча категорія → 404."""
        from uuid import uuid4

        response = await client.get(
            f"/api/v2/categories/{uuid4()}", headers=auth_headers
        )
        assert response.status_code == 404
