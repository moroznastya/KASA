"""
Інтеграційні тести повного циклу продажу.

Сценарій 1: Повний цикл продажу

Перевіряє:
  - Створення товару → пошук на POS → додавання в кошик → оплата → створення чеку
  - Зменшення залишку після продажу
  - Помилка при недостатній кількості товару
  - Повернення товару (type=RETURN) → збільшення залишку
"""

from decimal import Decimal
from uuid import uuid4

import pytest
from httpx import AsyncClient


pytestmark = [
    pytest.mark.integration,
    pytest.mark.sale,
]


class TestSaleFlow:
    """Тести повного циклу продажу."""

    async def test_full_sale_flow(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Повний цикл продажу:
        1. Створюємо товар (admin)
        2. Шукаємо товар за штрих-кодом (cashier)
        3. Створюємо чек продажу (cashier)
        4. Перевіряємо, що stock зменшився
        """
        # 1. Створюємо товар
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000000001",
                "sku": "TEST-SALE-001",
                "title": "Тестовий товар для продажу",
                "price": 150.00,
                "cost_price": 100.00,
                "stock": 50.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        product = response.json()
        product_id = product["id"]
        assert product["stock"] == 50.000

        # 2. Шукаємо товар за штрих-кодом
        response = await client.get(
            f"/api/v1/products/barcode/4820000000001",
            headers=cashier_headers,
        )
        assert response.status_code == 200
        assert response.json()["id"] == product_id

        # 3. Створюємо чек продажу (продаємо 3 шт)
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-001",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 450.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 3.000,
                        "price": 150.00,
                        "total": 450.00,
                    }
                ],
            },
        )
        assert response.status_code == 201
        receipt = response.json()
        assert receipt["receipt_number"] == "SALE-001"
        assert receipt["receipt_type"] == "sale"
        assert receipt["total_amount"] == 450.00

        # 4. Перевіряємо, що stock зменшився (50 - 3 = 47)
        response = await client.get(
            f"/api/v1/products/{product_id}",
            headers=cashier_headers,
        )
        assert response.status_code == 200
        assert response.json()["stock"] == 47.000

    async def test_sale_insufficient_stock(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Продаж з недостатньою кількістю товару.
        """
        # Створюємо товар з малим залишком
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000000002",
                "sku": "TEST-LOW-STOCK",
                "title": "Товар з малим залишком",
                "price": 100.00,
                "stock": 2.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        product_id = response.json()["id"]

        # Спроба продати 5 шт (є тільки 2)
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-INSUFFICIENT",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 500.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 5.000,
                        "price": 100.00,
                        "total": 500.00,
                    }
                ],
            },
        )
        assert response.status_code == 400
        data = response.json()
        assert "detail" in data
        assert "Недостатньо" in data["detail"]

    async def test_sale_and_return(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Продаж товару, а потім повернення.
        Після повернення stock має збільшитись.
        """
        # 1. Створюємо товар
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000000003",
                "sku": "TEST-RETURN-001",
                "title": "Товар для повернення",
                "price": 200.00,
                "stock": 100.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        product_id = response.json()["id"]

        # 2. Продаємо 10 шт
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-FOR-RETURN",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 2000.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 10.000,
                        "price": 200.00,
                        "total": 2000.00,
                    }
                ],
            },
        )
        assert response.status_code == 201

        # Перевіряємо stock після продажу (100 - 10 = 90)
        response = await client.get(
            f"/api/v1/products/{product_id}",
            headers=cashier_headers,
        )
        assert response.json()["stock"] == 90.000

        # 3. Повертаємо 3 шт
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "RET-001",
                "receipt_type": "return",
                "cashier_id": str(cashier_user.id),
                "total_amount": 600.00,
                "is_return": True,
                "items": [
                    {
                        "product_id": product_id,
                        "quantity": 3.000,
                        "price": 200.00,
                        "total": 600.00,
                    }
                ],
            },
        )
        assert response.status_code == 201

        # Перевіряємо stock після повернення (90 + 3 = 93)
        response = await client.get(
            f"/api/v1/products/{product_id}",
            headers=cashier_headers,
        )
        assert response.json()["stock"] == 93.000

    async def test_sale_multiple_items(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Продаж декількох товарів в одному чеку.
        """
        # Створюємо два товари
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000000010",
                "sku": "MULTI-001",
                "title": "Товар A",
                "price": 50.00,
                "stock": 100.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        prod_a_id = response.json()["id"]

        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": "4820000000011",
                "sku": "MULTI-002",
                "title": "Товар B",
                "price": 30.00,
                "stock": 200.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        prod_b_id = response.json()["id"]

        # Продаємо обидва товари
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-MULTI",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 190.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": prod_a_id,
                        "quantity": 2.000,
                        "price": 50.00,
                        "total": 100.00,
                    },
                    {
                        "product_id": prod_b_id,
                        "quantity": 3.000,
                        "price": 30.00,
                        "total": 90.00,
                    },
                ],
            },
        )
        assert response.status_code == 201

        # Перевіряємо stock обох товарів
        response = await client.get(
            f"/api/v1/products/{prod_a_id}",
            headers=cashier_headers,
        )
        assert response.json()["stock"] == 98.000  # 100 - 2

        response = await client.get(
            f"/api/v1/products/{prod_b_id}",
            headers=cashier_headers,
        )
        assert response.json()["stock"] == 197.000  # 200 - 3
