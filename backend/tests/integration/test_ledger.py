"""
Інтеграційні тести взаєморозрахунків з постачальниками.

Сценарій 5: Взаєморозрахунки

Перевіряє:
  - Створення запису в ledger через накладну
  - Створення запису оплати через POST /ledger
  - Розрахунок балансу після декількох операцій
  - Історія операцій з пагінацією
  - Коригування балансу
"""

from decimal import Decimal
from datetime import datetime

import pytest
from httpx import AsyncClient


pytestmark = [
    pytest.mark.integration,
    pytest.mark.ledger,
]


class TestLedger:
    """Тести взаєморозрахунків з постачальниками."""

    async def _create_supplier(
        self, client: AsyncClient, auth_headers: dict
    ) -> dict:
        """Допоміжний метод: створює постачальника."""
        response = await client.post(
            "/api/v1/suppliers",
            headers=auth_headers,
            json={
                "name": 'ТОВ "Для взаєморозрахунків"',
                "edrpou": "11112222",
                "phone": "+380501234567",
            },
        )
        assert response.status_code == 201
        return response.json()

    async def _create_product(
        self, client: AsyncClient, auth_headers: dict
    ) -> dict:
        """Допоміжний метод: створює товар."""
        response = await client.post(
            "/api/v1/products",
            headers=auth_headers,
            json={
                "barcode": f"482000{datetime.utcnow().timestamp():.0f}",
                "sku": f"SKU-LEDGER-{datetime.utcnow().timestamp():.0f}",
                "title": "Товар для ledger",
                "price": 100.00,
                "cost_price": 70.00,
                "stock": 500.000,
                "unit": "шт",
            },
        )
        assert response.status_code == 201
        return response.json()

    async def test_balance_after_invoice(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Після підтвердження накладної баланс = сумі накладної.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Створюємо та підтверджуємо накладну на 7000 грн
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-LEDGER-001",
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

        # Перевіряємо баланс
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.status_code == 200
        assert response.json()["current_balance"] == 7000.00

    async def test_balance_after_payment(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Після оплати баланс зменшується.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Створюємо борг 7000 грн
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-LEDGER-002",
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

        # Баланс = 7000
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 7000.00

        # Оплачуємо 3000 грн
        response = await client.post(
            "/api/v1/ledger",
            headers=auth_headers,
            json={
                "supplier_id": supplier["id"],
                "operation_type": "payment",
                "amount": -3000.00,
                "balance_after": 4000.00,
                "operation_date": datetime.utcnow().isoformat(),
                "document_number": "PAY-001",
                "notes": "Оплата частинами",
            },
        )
        assert response.status_code == 201

        # Баланс = 4000 (7000 - 3000)
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 4000.00

    async def test_full_ledger_cycle(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Повний цикл: накладна → оплата → повернення → перевірка балансу.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # 1. Накладна на 10000 грн
        response = await client.post(
            "/api/v1/invoices",
            headers=auth_headers,
            json={
                "number": "INV-LEDGER-003",
                "supplier_id": supplier["id"],
                "invoice_date": datetime.utcnow().isoformat(),
                "total_amount": 10000.00,
                "items": [
                    {
                        "product_id": product["id"],
                        "quantity": 100.000,
                        "price": 100.00,
                        "total": 10000.00,
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

        # Баланс = 10000
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 10000.00

        # 2. Оплата 4000 грн
        await client.post(
            "/api/v1/ledger",
            headers=auth_headers,
            json={
                "supplier_id": supplier["id"],
                "operation_type": "payment",
                "amount": -4000.00,
                "balance_after": 6000.00,
                "operation_date": datetime.utcnow().isoformat(),
                "document_number": "PAY-002",
            },
        )

        # Баланс = 6000
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 6000.00

        # 3. Повернення товару на 1000 грн
        response = await client.post(
            "/api/v1/return-invoices",
            headers=auth_headers,
            json={
                "number": "RET-SUP-LEDGER-002",
                "supplier_id": supplier["id"],
                "return_date": datetime.utcnow().isoformat(),
                "total_amount": 1000.00,
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
        return_id = response.json()["id"]
        await client.post(
            f"/api/v1/return-invoices/{return_id}/confirm",
            headers=auth_headers,
            json={"status": "confirmed"},
        )

        # Баланс = 5000 (6000 - 1000)
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 5000.00

    async def test_ledger_history(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Історія операцій містить всі записи.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Створюємо 2 накладні
        for i in range(2):
            response = await client.post(
                "/api/v1/invoices",
                headers=auth_headers,
                json={
                    "number": f"INV-HIST-{i:03d}",
                    "supplier_id": supplier["id"],
                    "invoice_date": datetime.utcnow().isoformat(),
                    "total_amount": 1000.00,
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
            invoice_id = response.json()["id"]
            await client.post(
                f"/api/v1/invoices/{invoice_id}/confirm",
                headers=auth_headers,
                json={"status": "confirmed"},
            )

        # Отримуємо історію
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}",
            headers=auth_headers,
        )
        assert response.status_code == 200
        data = response.json()
        assert data["total"] >= 2
        assert len(data["items"]) >= 2

        # Перевіряємо, що всі записи мають правильну структуру
        for entry in data["items"]:
            assert "id" in entry
            assert "operation_type" in entry
            assert "amount" in entry
            assert "balance_after" in entry
            assert "operation_date" in entry

    async def test_ledger_history_pagination(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Пагінація історії операцій працює коректно.
        """
        supplier = await self._create_supplier(client, auth_headers)
        product = await self._create_product(client, auth_headers)

        # Створюємо 3 накладні
        for i in range(3):
            response = await client.post(
                "/api/v1/invoices",
                headers=auth_headers,
                json={
                    "number": f"INV-PAG-{i:03d}",
                    "supplier_id": supplier["id"],
                    "invoice_date": datetime.utcnow().isoformat(),
                    "total_amount": 1000.00,
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
            invoice_id = response.json()["id"]
            await client.post(
                f"/api/v1/invoices/{invoice_id}/confirm",
                headers=auth_headers,
                json={"status": "confirmed"},
            )

        # Сторінка 1, розмір 2
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}?page=1&size=2",
            headers=auth_headers,
        )
        assert response.status_code == 200
        data = response.json()
        assert data["total"] >= 3
        assert len(data["items"]) == 2
        assert data["page"] == 1
        assert data["size"] == 2

        # Сторінка 2, розмір 2
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}?page=2&size=2",
            headers=auth_headers,
        )
        assert response.status_code == 200
        data = response.json()
        assert len(data["items"]) >= 1
        assert data["page"] == 2

    async def test_correction_entry(
        self,
        client: AsyncClient,
        session,
        auth_headers: dict,
    ):
        """
        Коригування балансу через POST /ledger.
        """
        supplier = await self._create_supplier(client, auth_headers)

        # Створюємо запис коригування
        response = await client.post(
            "/api/v1/ledger",
            headers=auth_headers,
            json={
                "supplier_id": supplier["id"],
                "operation_type": "correction",
                "amount": 500.00,
                "balance_after": 500.00,
                "operation_date": datetime.utcnow().isoformat(),
                "notes": "Коригування початкового боргу",
            },
        )
        assert response.status_code == 201
        entry = response.json()
        assert entry["operation_type"] == "correction"
        assert entry["amount"] == 500.00
        assert entry["balance_after"] == 500.00

        # Перевіряємо баланс
        response = await client.get(
            f"/api/v1/ledger/{supplier['id']}/balance",
            headers=auth_headers,
        )
        assert response.json()["current_balance"] == 500.00
