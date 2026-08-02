"""
Інтеграційні тести чеків через API v2.

Покриває флоу (HTTP → ReceiptUseCases → БД):
  - Створення чека продажу з позиціями (POST /api/v2/receipts/sale) → 201
  - Зменшення залишку товару після продажу
  - Список чеків (GET /api/v2/receipts) → 200
  - Деталі чека (GET /api/v2/receipts/{id}) → 200 з позиціями
  - Позиції чека (GET /api/v2/receipts/{id}/items) → 200
  - Продаж з недостатнім залишком → 400
  - Продаж неіснуючого товару → 400
"""

import pytest
from httpx import AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.receipts,
    pytest.mark.v2,
]


class TestV2ReceiptsFlow:
    """Створення та перегляд чеків через API v2."""

    async def _create_product(
        self, client: AsyncClient, auth_headers: dict, barcode: str, stock: float
    ) -> dict:
        response = await client.post(
            "/api/v2/products",
            headers=auth_headers,
            json={
                "name": f"Товар {barcode}",
                "barcode": barcode,
                "price": 100.0,
                "cost_price": 60.0,
                "quantity": stock,
                "unit": "шт",
            },
        )
        assert response.status_code == 201, response.text
        return response.json()

    async def test_create_sale_receipt_decreases_stock(
        self, client: AsyncClient, auth_headers: dict, admin_user
    ):
        """
        Повний флоу: товар → чек продажу (3 шт) → залишок зменшився (10 → 7).
        """
        product = await self._create_product(
            client, auth_headers, "4820000000307", stock=10.0
        )

        response = await client.post(
            "/api/v2/receipts/sale",
            headers=auth_headers,
            json={
                "items": [
                    {
                        "product_id": product["id"],
                        "name": product["name"],
                        "quantity": 3,
                        "price": 100.0,
                    }
                ],
                "payment_method": "cash",
                "cash_amount": 300.0,
                "notes": "Тестовий чек v2",
            },
        )
        assert response.status_code == 201, response.text
        receipt = response.json()
        assert receipt["id"]
        assert receipt["number"]
        assert len(receipt["items"]) == 1
        assert float(receipt["items"][0]["quantity"]) == 3.0
        assert float(receipt["total"]) == 300.0

        # Залишок зменшився: 10 - 3 = 7
        check = await client.get(
            f"/api/v2/products/{product['id']}", headers=auth_headers
        )
        assert check.status_code == 200
        assert float(check.json()["quantity"]) == 7.0

    async def test_list_receipts(self, client: AsyncClient, auth_headers: dict):
        """Список чеків → 200, містить створений чек."""
        product = await self._create_product(
            client, auth_headers, "4820000000314", stock=10.0
        )
        created = await client.post(
            "/api/v2/receipts/sale",
            headers=auth_headers,
            json={
                "items": [{"product_id": product["id"], "quantity": 1, "price": 100.0}],
                "payment_method": "cash",
            },
        )
        receipt_id = created.json()["id"]

        response = await client.get("/api/v2/receipts", headers=auth_headers)
        assert response.status_code == 200
        data = response.json()
        assert data["total"] >= 1
        assert any(r["id"] == receipt_id for r in data["items"])

    async def test_get_receipt_detail(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Деталі чека → 200 з позиціями."""
        product = await self._create_product(
            client, auth_headers, "4820000000321", stock=10.0
        )
        created = await client.post(
            "/api/v2/receipts/sale",
            headers=auth_headers,
            json={
                "items": [
                    {
                        "product_id": product["id"],
                        "name": product["name"],
                        "quantity": 2,
                        "price": 100.0,
                    }
                ],
                "payment_method": "card",
                "card_amount": 200.0,
            },
        )
        receipt_id = created.json()["id"]

        response = await client.get(
            f"/api/v2/receipts/{receipt_id}", headers=auth_headers
        )
        assert response.status_code == 200
        data = response.json()
        assert data["id"] == receipt_id
        assert len(data["items"]) == 1
        assert data["items"][0]["product_id"] == product["id"]
        assert float(data["total"]) == 200.0

    async def test_get_receipt_items(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Позиції чека → 200."""
        product = await self._create_product(
            client, auth_headers, "4820000000338", stock=10.0
        )
        created = await client.post(
            "/api/v2/receipts/sale",
            headers=auth_headers,
            json={
                "items": [{"product_id": product["id"], "quantity": 4, "price": 50.0}],
                "payment_method": "cash",
            },
        )
        receipt_id = created.json()["id"]

        response = await client.get(
            f"/api/v2/receipts/{receipt_id}/items", headers=auth_headers
        )
        assert response.status_code == 200
        items = response.json()
        assert isinstance(items, list)
        assert len(items) == 1
        assert items[0]["product_id"] == product["id"]
        assert float(items[0]["quantity"]) == 4.0

    async def test_sale_insufficient_stock_returns_400(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Продаж більше ніж є на складі → 400."""
        product = await self._create_product(
            client, auth_headers, "4820000000345", stock=2.0
        )
        response = await client.post(
            "/api/v2/receipts/sale",
            headers=auth_headers,
            json={
                "items": [{"product_id": product["id"], "quantity": 5, "price": 100.0}],
                "payment_method": "cash",
            },
        )
        assert response.status_code == 400
        assert "Недостатньо" in response.json()["detail"]

    async def test_sale_missing_product_returns_400(
        self, client: AsyncClient, auth_headers: dict
    ):
        """Чек з неіснуючим товаром → 400."""
        from uuid import uuid4

        response = await client.post(
            "/api/v2/receipts/sale",
            headers=auth_headers,
            json={
                "items": [{"product_id": str(uuid4()), "quantity": 1, "price": 100.0}],
                "payment_method": "cash",
            },
        )
        assert response.status_code == 400
        assert "не знайдено" in response.json()["detail"]
