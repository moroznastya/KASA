"""
Інтеграційні тести повернення товару від клієнта.

Сценарій 3: Повернення від клієнта

Перевіряє:
  - Створення чеку повернення (type=RETURN)
  - Збільшення залишку після повернення
  - Статистика за день враховує повернення окремо
"""


import pytest
from httpx import AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.sale,
]


class TestReturnFlow:
    """Тести повернення товару від клієнта."""

    async def test_return_increases_stock(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Повернення товару збільшує залишок на складі.
        """
        # 1. Створюємо товар
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000100001",
                "sku": "RETURN-STOCK-001",
                "title": "Товар для повернення клієнтом",
                "price": 250.00,
                "stock": 50.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        product_id = response.json()["id"]

        # 2. Продаємо 5 шт
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-BEFORE-RETURN",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 1250.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 5.000,
                        "price": 250.00,
                        "total": 1250.00,
                    }
                ],
            },
        )
        assert response.status_code == 201

        # Stock = 45
        response = await client.get(
            f"/api/v1/products/{product_id}",
            headers=cashier_headers,
        )
        assert float(response.json()["stock"]) == 45.0

        # 3. Повертаємо 2 шт
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "RET-CLIENT-001",
                "receipt_type": "return",
                "cashier_id": str(cashier_user.id),
                "total_amount": 500.00,
                "is_return": True,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 2.000,
                        "price": 250.00,
                        "total": 500.00,
                    }
                ],
            },
        )
        assert response.status_code == 201
        receipt = response.json()
        assert receipt["receipt_type"] == "return"
        assert receipt["is_return"] is True

        # Stock = 47 (45 + 2)
        response = await client.get(
            f"/api/v1/products/{product_id}",
            headers=cashier_headers,
        )
        assert float(response.json()["stock"]) == 47.0

    async def test_return_receipt_has_correct_type(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Чек повернення має правильний тип та суму.
        """
        # Створюємо товар
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000200001",
                "sku": "RETURN-TYPE-001",
                "title": "Товар для перевірки типу",
                "price": 100.00,
                "stock": 30.000,
                "unit": "шт",
            },
        )
        product_id = response.json()["id"]

        # Продаємо
        await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-TYPE-TEST",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 300.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 3.000,
                        "price": 100.00,
                        "total": 300.00,
                    }
                ],
            },
        )

        # Повертаємо 1 шт
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "RET-TYPE-001",
                "receipt_type": "return",
                "cashier_id": str(cashier_user.id),
                "total_amount": 100.00,
                "is_return": True,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 1.000,
                        "price": 100.00,
                        "total": 100.00,
                    }
                ],
            },
        )
        assert response.status_code == 201
        data = response.json()
        assert data["receipt_type"] == "return"
        assert data["is_return"] is True
        assert float(data["total_amount"]) == 100.0

    async def test_today_stats_reflects_returns(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Статистика за день враховує повернення окремо.
        """
        # Створюємо товар
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000300001",
                "sku": "RETURN-STATS-001",
                "title": "Товар для статистики",
                "price": 500.00,
                "stock": 100.000,
                "unit": "шт",
            },
        )
        product_id = response.json()["id"]

        # Продаємо на 1000 грн
        await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-STATS-001",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 1000.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 2.000,
                        "price": 500.00,
                        "total": 1000.00,
                    }
                ],
            },
        )

        # Повертаємо на 500 грн
        await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "RET-STATS-001",
                "receipt_type": "return",
                "cashier_id": str(cashier_user.id),
                "total_amount": 500.00,
                "is_return": True,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 1.000,
                        "price": 500.00,
                        "total": 500.00,
                    }
                ],
            },
        )

        # Перевіряємо статистику
        response = await client.get(
            "/api/v1/receipts/stats/today",
            headers=auth_headers,
        )
        assert response.status_code == 200
        stats = response.json()
        assert float(stats["total_sales"]) == 1000.0
        assert float(stats["total_returns"]) == 500.0
        assert stats["receipts_count"] >= 2
