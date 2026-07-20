"""
Фабрики для створення тестових прибуткових накладних (Invoice, InvoiceItem).

Використовує factory_boy для генерації даних.
"""

from uuid import uuid4
from decimal import Decimal
from datetime import datetime
from typing import Optional

import factory

from app.models.invoice import Invoice, InvoiceItem, InvoiceStatus


class InvoiceItemFactory(factory.Factory):
    """Фабрика для створення позицій прибуткової накладної."""

    class Meta:
        model = InvoiceItem

    id = factory.LazyFunction(uuid4)
    invoice_id = None  # Має бути передано при створенні
    product_id = None  # Має бути передано при створенні
    quantity = Decimal("10.000")
    price = Decimal("70.00")
    total = Decimal("700.00")
    created_at = factory.LazyFunction(datetime.utcnow)


class InvoiceFactory(factory.Factory):
    """Фабрика для створення тестових прибуткових накладних."""

    class Meta:
        model = Invoice

    id = factory.LazyFunction(uuid4)
    number = factory.Sequence(lambda n: f"INV-{n:05d}")
    supplier_id = None  # Має бути передано при створенні
    invoice_date = factory.LazyFunction(datetime.utcnow)
    status = InvoiceStatus.DRAFT
    notes = factory.Faker("sentence", locale="uk_UA")
    total_amount = Decimal("700.00")
    created_at = factory.LazyFunction(datetime.utcnow)
    updated_at = factory.LazyFunction(datetime.utcnow)

    @classmethod
    def draft(cls, **kwargs):
        """Створює накладну в статусі чернетки."""
        return cls(status=InvoiceStatus.DRAFT, **kwargs)

    @classmethod
    def confirmed(cls, **kwargs):
        """Створює підтверджену накладну."""
        return cls(status=InvoiceStatus.CONFIRMED, **kwargs)

    @classmethod
    def cancelled(cls, **kwargs):
        """Створює скасовану накладну."""
        return cls(status=InvoiceStatus.CANCELLED, **kwargs)
