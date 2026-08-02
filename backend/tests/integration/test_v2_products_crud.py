"""
Інтеграційні тести повного CRUD товарів через API v2.

Покриває флоу (HTTP → ProductUseCases → БД):
  - Створення товару (POST /api/v2/products) → 201
  - Отримання товару за ID (GET /api/v2/products/{id}) → 200
  - Пошук товару (GET /api/v2/products?search=) → 200
  - Оновлення товару (PUT /api/v2/products/{id}) → 200
  - Видалення товару (DELETE /api/v2/products/{id}) → 204 → 404
  - Дублікат штрих-коду → 400
"""

import pytest
from httpx import AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.products,
    pytest.mark.v2,
]


class TestV2ProductsCRUD:
    """Повний CRUD-цикл товару через API v2."""

    async def _create_product(
        self, client: AsyncClient, auth_headers: dict, **overrides
    ) -> dict:
        payload = {
            "name": "Кава зернова",
            "barcode": "4820000000109",
            "price": 250.0,
            "cost_price": 180.0,
            "quantity": 0.0,
            "unit": "кг",
            "sku": "SKU-V2-CRUD-001",
            "description": "Тестовий товар для CRUD",
        }
        payload.update(overrides)
        response = await client.post("/api/v2/products", headers=auth_headers, json=payload)
        assert response.status_code == 201, (
            f"Створення товару не вдалося: {response.status_code} - {response.text}"
        )
        return response.json()

    async def test_create_product(self, client: AsyncClient, auth_headers: dict):
        """Створення товару через v2 → 201, дані збережено в БД."""
        product = await self._create_product(client, auth_headers)
        assert product["name"] == "Кава зернова"
        assert product["barcode"] == "4820000000109"
        assert float(product["price"]) == 250.0
        assert float(product["quantity"]) == 0.0
        assert product["unit"] == "кг"

    async def test_get_product_by_id(self, client: AsyncClient, auth_headers: dict):
        """Отримання товару за ID → 200 з тими ж даними."""
        created = await self._create_product(client, auth_headers)
        response = await client.get(
            f"/api/v2/products/{created['id']}", headers=auth_headers
        )
        assert response.status_code == 200
        data = response.json()
        assert data["id"] == created["id"]
        assert data["name"] == "Кава зернова"

    async def test_search_products(self, client: AsyncClient, auth_headers: dict):
        """Пошук товару за назвою → 200, товар знайдено."""
        await self._create_product(client, auth_headers)
        response = await client.get(
            "/api/v2/products?search=Кава", headers=auth_headers
        )
        assert response.status_code == 200
        data = response.json()
        assert data["total"] >= 1
        assert any("Кава" in item["name"] for item in data["items"])

    async def test_search_products_empty(self, client: AsyncClient, auth_headers: dict):
        """Пошук неіснуючого товару → 200, порожній список."""
        await self._create_product(client, auth_headers)
        response = await client.get(
            "/api/v2/products?search=zzz-неіснує", headers=auth_headers
        )
        assert response.status_code == 200
        assert response.json()["total"] == 0

    async def test_update_product(self, client: AsyncClient, auth_headers: dict):
        """Оновлення ціни та назви товару → 200, зміни в БД."""
        created = await self._create_product(client, auth_headers)
        response = await client.put(
            f"/api/v2/products/{created['id']}",
            headers=auth_headers,
            json={"name": "Кава еспресо", "price": 300.0},
        )
        assert response.status_code == 200
        data = response.json()
        assert data["name"] == "Кава еспресо"
        assert float(data["price"]) == 300.0

        # Перевіряємо, що зміни реально збереглися в БД
        check = await client.get(
            f"/api/v2/products/{created['id']}", headers=auth_headers
        )
        assert check.json()["name"] == "Кава еспресо"

    async def test_delete_product(self, client: AsyncClient, auth_headers: dict):
        """Видалення товару з нульовим залишком → 204 → 404 після."""
        created = await self._create_product(client, auth_headers)
        response = await client.delete(
            f"/api/v2/products/{created['id']}", headers=auth_headers
        )
        assert response.status_code == 204

        # Після видалення товар не знаходиться
        check = await client.get(
            f"/api/v2/products/{created['id']}", headers=auth_headers
        )
        assert check.status_code == 404

    async def test_create_product_duplicate_barcode(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Дублікат штрих-коду → 400."""
        await self._create_product(client, auth_headers)
        response = await client.post(
            "/api/v2/products",
            headers=auth_headers,
            json={
                "name": "Кава інша",
                "barcode": "4820000000109",  # той самий штрих-код
                "price": 100.0,
            },
        )
        assert response.status_code == 400

    async def test_delete_product_with_stock_returns_400(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Видалення товару з ненульовим залишком → 400 (бізнес-правило)."""
        created = await self._create_product(
            client, auth_headers, barcode="4820000000208", quantity=5.0
        )
        response = await client.delete(
            f"/api/v2/products/{created['id']}", headers=auth_headers
        )
        assert response.status_code == 400
        assert "залишок" in response.json()["detail"]
