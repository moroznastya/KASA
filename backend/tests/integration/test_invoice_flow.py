"""
Інтеграційні тести прибуткової накладної.

Сценарій 2: Прибуткова накладна → оприбуткування → продаж

Перевіряє:
  - Створення прибуткової накладної
  - Підтвердження накладної → збільшення stock
  - Скасування накладної → відкат stock
  - Спроба підтвердити повторно (400)
  - Спроба редагувати підтверджену (400)
  - Створення запису в SupplierLedger
  - Повний цикл: накладна → продаж
"""

from datetime import datetime

import pytest
from httpx import AsyncClient

pytestmark = [
    pytest.mark.integration,
    pytest.mark.invoice,
]


class TestInvoiceFlow:
    """Тести прибуткової накладної."""

    async def _create_supplier(
        self, client: AsyncClient, auth_headers: dict
    ) -> dict:
        """Допоміжний метод: створює постачальника."""
        response = await client.post(
            "/api/v1/suppliers",
            headers=auth_headers,
            json={
                "name": 'ТОВ "Тестовий Постачальник"',
                "edrpou": "12345678",
                "phone": "+380501112233",
            },
        )
        assert response.status_code == 201
        return response.json()

    async def _create_product(
        self, client: AsyncClient, auth_headers: dict, supplier_id: str | None = None
    ) -> dict:
        """Допоміжний метод: створює товар."""
        data = {
            "barcode": f"482000{datetime.utcnow().timestamp():.0f}",
            "sku": f"SKU-{datetime.utcnow().timestamp():.0f}",
            "title": "Тестовий товар",
            "price": 100.00,
            "cost_price": 70.00,
            "stock": 0.000,
            "unit": "шт",
        }
        if supplier_id:
            data["supplier_id"] = supplier_id
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json=data,
        )
        assert response.status_code == 201
        return response.json()

    async def test_create_and_confirm_invoice(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Створення та підтвердження прибуткової накладної.
        Після підтвердження stock має збільшитись.
        """
        # Створюємо постачальника та товар
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # Створюємо накладну
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-TEST-001",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 700.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 70.00,
                        "total": 700.00,
                    }
                ],
            },
        )
        assert response.status_code == 201
        invoice = response.json()
        assert invoice["status"] == "draft"
        invoice_id = invoice["id"]

        # Перевіряємо, що stock = 0 (товар ще не оприбутковано)
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert float(response.json()["stock"]) == 0.0

        # Підтверджуємо накладну
        response = await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )
        assert response.status_code == 200
        assert response.json()["status"] == "confirmed"

        # Перевіряємо, що stock збільшився (0 + 10 = 10)
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert float(response.json()["stock"]) == 10.0

    async def test_cancel_invoice_restores_stock(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Скасування накладної повертає stock до попереднього значення.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # Створюємо та підтверджуємо накладну
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-TEST-002",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 1400.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 20.000,
                        "price": 70.00,
                        "total": 1400.00,
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

        # Перевіряємо stock = 20
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert float(response.json()["stock"]) == 20.0

        # Скасовуємо накладну
        response = await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "cancelled"},
        )
        assert response.status_code == 200
        assert response.json()["status"] == "cancelled"

        # Перевіряємо, що stock повернувся до 0
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert float(response.json()["stock"]) == 0.0

    async def test_cannot_confirm_twice(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Не можна підтвердити вже підтверджену накладну.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # Створюємо та підтверджуємо накладну
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-TEST-003",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 700.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 70.00,
                        "total": 700.00,
                    }
                ],
            },
        )
        invoice_id = response.json()["id"]

        # Перше підтвердження — успіх
        response = await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )
        assert response.status_code == 200

        # Друге підтвердження — помилка
        response = await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )
        assert response.status_code == 400

    async def test_cannot_edit_confirmed_invoice(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Не можна редагувати підтверджену накладну.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # Створюємо та підтверджуємо накладну
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-TEST-004",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 700.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 70.00,
                        "total": 700.00,
                    }
                ],
            },
        )
        invoice_id = response.json()["id"]

        # Підтверджуємо
        await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Спроба редагувати
        response = await client.put(
            f"/api/v1/invoices/{invoice_id}",
            headers=auth_headers,
            json={"notes": "Спроба редагування"},
        )
        assert response.status_code == 400

    async def test_cannot_delete_confirmed_invoice(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Не можна видалити підтверджену накладну.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # Створюємо та підтверджуємо накладну
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-TEST-005",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 700.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 70.00,
                        "total": 700.00,
                    }
                ],
            },
        )
        invoice_id = response.json()["id"]

        # Підтверджуємо
        await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Спроба видалити
        response = await client.delete(
            f"/api/v1/invoices/{invoice_id}",
            headers=auth_headers,
        )
        assert response.status_code == 400

    async def test_full_cycle_invoice_to_sale(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
        cashier_headers: dict,
        cashier_user,
    ):
        """
        Повний цикл: накладна → оприбуткування → продаж.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # 1. Створюємо накладну на 50 шт
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-FULL-CYCLE",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 3500.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 50.000,
                        "price": 70.00,
                        "total": 3500.00,
                    }
                ],
            },
        )
        invoice_id = response.json()["id"]

        # 2. Підтверджуємо накладну
        await client.post(
            f"/api/v1/invoices/{invoice_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Перевіряємо stock = 50
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert float(response.json()["stock"]) == 50.0

        # 3. Продаємо 10 шт
        response = await client.post(
            "/api/v1/receipts",
            headers=cashier_headers,
            json={
                "receipt_number": "SALE-AFTER-INVOICE",
                "receipt_type": "sale",
                "cashier_id": str(cashier_user.id),
                "total_amount": 1000.00,
                "is_return": False,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 10.000,
                        "price": 100.00,
                        "total": 1000.00,
                    }
                ],
            },
        )
        assert response.status_code == 201

        # Перевіряємо stock = 40 (50 - 10)
        response = await client.get(
            f"/api/v1/products/{product['id']}",
            headers=auth_headers,
        )
        assert float(response.json()["stock"]) == 40.0

    async def test_invoice_creates_ledger_entry(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Підтвердження накладної створює запис у SupplierLedger.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers, supplier["id"])

        # Створюємо та підтверджуємо накладну
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-LEDGER-TEST",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 7000.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 100.000,
                        "price": 70.00,
                        "total": 7000.00,
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

        # Перевіряємо баланс постачальника
        response = await client.get(
            f"/api/v1/ledger/balance/{supplier['id']}",
            headers=auth_headers,
        )
        assert response.status_code == 200
        balance_data = response.json()
        assert float(balance_data["current_balance"]) == 7000.00
        assert balance_data["supplier_name"] == supplier["name"]
