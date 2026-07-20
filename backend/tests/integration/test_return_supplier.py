"""
Інтеграційні тести повернення товару постачальнику.

Сценарій 4: Повернення постачальнику

Перевіряє:
  - Створення повернення постачальнику
  - Підтвердження → зменшення stock
  - Скасування → відкат stock
  - Створення запису в SupplierLedger (зменшення боргу)
  - Спроба підтвердити повторно (400)
"""

from decimal import Decimal
from datetime import datetime

import pytest
from httpx import AsyncClient


pytestmark = [
    pytest.mark.integration,
    pytest.mark.invoice,
]


class TestReturnSupplier:
    """Тести повернення товару постачальнику."""

    async def _create_supplier(
        self, client: AsyncClient, auth_headers: dict
    ) -> dict:
        """Допоміжний метод: створює постачальника."""
        response = await client.post(
            "/api/v1/suppliers",
            headers=auth_headers,
            json={
                "name": 'ТОВ "Постачальник для повернення"',
                "edrpou": "87654321",
                "phone": "+380509998877",
            },
        )
        assert response.status_code == 201
        return response.json()

    async def _create_product(
        self, client: AsyncClient, auth_headers: dict, supplier_id: str = None
    ) -> dict:
        """Допоміжний метод: створює товар."""
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": f"482000{datetime.utcnow().timestamp():.0f}",
                "sku": f"SKU-RET-{datetime.utcnow().timestamp():.0f}",
                "title": "Товар для повернення постачальнику",
                "price": 200.00,
                "cost_price": 150.00,
                "stock": 100.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        return response.json()

    async def test_confirm_return_reduces_stock(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Підтвердження повернення постачальнику зменшує stock.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Створюємо повернення постачальнику
        response = await client.post(
            "/api/v1/return-invoices",
            headers=auth_headers,
            json={
                "number": "RET-SUP-001",
                "supplier_id": supplier["id"],
                "return_date": datetime.utcnow().isoformat(),
                "total_amount": 1500.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 150.00,
                        "total": 1500.00,
                    }
                ],
            },
        )
        assert response.status_code == 201
        return_id = response.json()["id"]
        assert response.json()["status"] == "draft"

        # Stock = 100 (не змінився, бо документ ще не підтверджено)
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert response.json()["stock"] == 100.000

        # Підтверджуємо повернення
        response = await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )
        assert response.status_code == 200
        assert response.json()["status"] == "confirmed"

        # Stock = 90 (100 - 10)
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert response.json()["stock"] == 90.000

    async def test_cancel_return_restores_stock(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Скасування повернення постачальнику повертає stock.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Створюємо та підтверджуємо повернення
        response = await client.post(
            "/api/v1/return-invoices",
            headers=auth_headers,
            json={
                "number": "RET-SUP-CANCEL",
                "supplier_id": supplier["id"],
                "return_date": datetime.utcnow().isoformat(),
                "total_amount": 750.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 5.000,
                        "price": 150.00,
                        "total": 750.00,
                    }
                ],
            },
        )
        return_id = response.json()["id"]

        await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Stock = 95
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert response.json()["stock"] == 95.000

        # Скасовуємо повернення
        response = await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "cancelled"},
        )
        assert response.status_code == 200
        assert response.json()["status"] == "cancelled"

        # Stock = 100 (95 + 5)
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert response.json()["stock"] == 100.000

    async def test_return_creates_ledger_entry(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Підтвердження повернення створює запис у SupplierLedger
        з від'ємною сумою (зменшення боргу).
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Спочатку створимо накладну, щоб був борг
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-BEFORE-RET",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 15000.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 100.000,
                        "price": 150.00,
                        "total": 15000.00,
                    }
                ],
            },
        )
        invoice_id = response.json()["id"]

        await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Баланс = 15000
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 15000.00

        # Створюємо та підтверджуємо повернення на 1500 грн
        response = await client.post(
            "/api/v1/return-invoices",
            headers=auth_headers,
            json={
                "number": "RET-SUP-LEDGER",
                "supplier_id": supplier["id"],
                "return_date": datetime.utcnow().isoformat(),
                "total_amount": 1500.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 150.00,
                        "total": 1500.00,
                    }
                ],
            },
        )
        return_id = response.json()["id"]

        await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Баланс = 13500 (15000 - 1500)
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 13500.00

    async def test_cannot_confirm_return_twice(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Не можна підтвердити вже підтверджене повернення.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        response = await client.post(
            "/api/v1/return-invoices",
            headers=auth_headers,
            json={
                "number": "RET-SUP-DOUBLE",
                "supplier_id": supplier["id"],
                "return_date": datetime.utcnow().isoformat(),
                "total_amount": 1500.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 150.00,
                        "total": 1500.00,
                    }
                ],
            },
        )
        return_id = response.json()["id"]

        # Перше підтвердження
        response = await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )
        assert response.status_code == 200

        # Друге підтвердження — помилка
        response = await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )
        assert response.status_code == 400
