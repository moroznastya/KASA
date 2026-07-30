"""Unit tests: Domain Events."""

from __future__ import annotations

from uuid import UUID, uuid4
from datetime import datetime, timezone
from decimal import Decimal

import pytest

from app.domain.events import (
    BaseDomainEvent,
    ProductCreated,
    ProductUpdated,
    ProductDeleted,
    StockChanged,
    InvoiceCreated,
    InvoiceUpdated,
    InvoiceDeleted,
    InvoiceApproved,
    ReceiptCreated,
    ReceiptRefunded,
    LedgerEntryCreated,
    UserLoggedIn,
    UserCreated,
)


class TestBaseDomainEvent:

    def test_event_id_auto_generated(self):
        """Event ID генерується автоматично."""
        event = ProductCreated(product_id=uuid4(), name="Test", barcode="123")
        assert isinstance(event.event_id, UUID)
        assert event.event_id is not None

    def test_occurred_at_auto_set(self):
        """Час події встановлюється автоматично."""
        event = ProductCreated(product_id=uuid4(), name="Test", barcode="123")
        assert isinstance(event.occurred_at, datetime)
        assert event.occurred_at.tzinfo is not None

    def test_event_name_auto_set(self):
        """Ім'я події встановлюється з назви класу."""
        event = ProductCreated(product_id=uuid4(), name="Test", barcode="123")
        assert event.event_name == "ProductCreated"

        event2 = InvoiceCreated(
            invoice_id=uuid4(), supplier_id=uuid4(),
            total_amount=Decimal("100.00"), status="pending"
        )
        assert event2.event_name == "InvoiceCreated"


class TestProductEvents:

    def test_product_created(self):
        """Створення події ProductCreated."""
        product_id = uuid4()
        event = ProductCreated(
            product_id=product_id,
            name="Тестовий товар",
            barcode="1234567890",
            category_id=uuid4(),
            supplier_id=uuid4(),
        )
        assert event.product_id == product_id
        assert event.name == "Тестовий товар"
        assert event.barcode == "1234567890"
        assert event.category_id is not None
        assert event.supplier_id is not None

    def test_product_created_minimal(self):
        """ProductCreated з мінімальними полями."""
        event = ProductCreated(
            product_id=uuid4(), name="Мін", barcode="1"
        )
        assert event.category_id is None
        assert event.supplier_id is None

    def test_product_updated(self):
        """Створення події ProductUpdated."""
        changes = {"price": (100.0, 150.0), "title": ("Стара", "Нова")}
        event = ProductUpdated(product_id=uuid4(), changes=changes)
        assert event.changes["price"] == (100.0, 150.0)
        assert event.changes["title"][1] == "Нова"

    def test_product_deleted(self):
        """Створення події ProductDeleted."""
        product_id = uuid4()
        event = ProductDeleted(product_id=product_id)
        assert event.product_id == product_id

    def test_stock_changed(self):
        """Створення події StockChanged."""
        product_id = uuid4()
        event = StockChanged(
            product_id=product_id,
            old_quantity=10.0,
            new_quantity=5.0,
            reason="sale",
            reference_type="receipt",
            reference_id=uuid4(),
        )
        assert event.product_id == product_id
        assert event.old_quantity == 10.0
        assert event.new_quantity == 5.0
        assert event.reason == "sale"
        assert event.reference_type == "receipt"


class TestInvoiceEvents:

    def test_invoice_created(self):
        """Створення події InvoiceCreated."""
        invoice_id = uuid4()
        event = InvoiceCreated(
            invoice_id=invoice_id,
            supplier_id=uuid4(),
            total_amount=Decimal("1500.50"),
            status="pending",
        )
        assert event.invoice_id == invoice_id
        assert event.total_amount == Decimal("1500.50")
        assert event.status == "pending"

    def test_invoice_approved(self):
        """Створення події InvoiceApproved."""
        event = InvoiceApproved(
            invoice_id=uuid4(),
            items_count=5,
        )
        assert event.items_count == 5


class TestReceiptEvents:

    def test_receipt_created(self):
        """Створення події ReceiptCreated."""
        receipt_id = uuid4()
        event = ReceiptCreated(
            receipt_id=receipt_id,
            cashier_id=uuid4(),
            total_amount=Decimal("250.00"),
            payment_method="cash",
        )
        assert event.receipt_id == receipt_id
        assert event.total_amount == Decimal("250.00")

    def test_receipt_refunded(self):
        """Створення події ReceiptRefunded."""
        original_id = uuid4()
        event = ReceiptRefunded(
            receipt_id=uuid4(),
            original_receipt_id=original_id,
            refund_amount=Decimal("100.00"),
        )
        assert event.original_receipt_id == original_id
        assert event.refund_amount == Decimal("100.00")


class TestLedgerEvents:

    def test_ledger_entry_created(self):
        """Створення події LedgerEntryCreated."""
        entry_id = uuid4()
        event = LedgerEntryCreated(
            entry_id=entry_id,
            supplier_id=uuid4(),
            amount=Decimal("500.00"),
            entry_type="debit",
            reference_type="invoice",
            reference_id=uuid4(),
        )
        assert event.entry_id == entry_id
        assert event.entry_type == "debit"


class TestUserEvents:

    def test_user_logged_in(self):
        """Створення події UserLoggedIn."""
        event = UserLoggedIn(
            user_id=uuid4(),
            login_method="password",
        )
        assert event.login_method == "password"

    def test_user_created(self):
        """Створення події UserCreated."""
        event = UserCreated(
            user_id=uuid4(),
            login="new_user",
            role="cashier",
        )
        assert event.login == "new_user"
        assert event.role == "cashier"


class TestEventHandlers:

    def test_all_events_importable(self):
        """Всі події імпортуються."""
        from app.domain.events import (
            BaseDomainEvent,
            ProductCreated,
            ProductUpdated,
            ProductDeleted,
            StockChanged,
            InvoiceCreated,
            InvoiceUpdated,
            InvoiceDeleted,
            InvoiceApproved,
            ReceiptCreated,
            ReceiptRefunded,
            LedgerEntryCreated,
            UserLoggedIn,
            UserCreated,
        )
        assert BaseDomainEvent is not None
        assert ProductCreated is not None
        assert UserCreated is not None
