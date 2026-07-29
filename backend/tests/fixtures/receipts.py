"""
Фабрики для створення тестових чеків (Receipt, ReceiptItem).

Використовує factory_boy для генерації даних.
"""

from uuid import uuid4
from decimal import Decimal
from datetime import datetime
from typing import Optional

import factory

from app.infrastructure.persistence.models.receipt import Receipt, ReceiptItem, ReceiptType


class ReceiptItemFactory(factory.Factory):
    """Фабрика для створення позицій чеку."""

    class Meta:
        model = ReceiptItem

    id = factory.LazyFunction(uuid4)
    receipt_id = None  # Має бути передано при створенні
    product_id = None  # Має бути передано при створенні
    quantity = Decimal("1.000")
    price = Decimal("100.00")
    total = Decimal("100.00")
    created_at = factory.LazyFunction(datetime.utcnow)


class ReceiptFactory(factory.Factory):
    """Фабрика для створення тестових чеків."""

    class Meta:
        model = Receipt

    id = factory.LazyFunction(uuid4)
    receipt_number = factory.Sequence(lambda n: f"REC-{n:05d}")
    receipt_type = ReceiptType.SALE
    cashier_id = None  # Має бути передано при створенні
    total_amount = Decimal("100.00")
    is_return = False
    notes = None
    created_at = factory.LazyFunction(datetime.utcnow)

    @classmethod
    def sale(cls, **kwargs):
        """Створює чек продажу."""
        return cls(
            receipt_type=ReceiptType.SALE,
            is_return=False,
            **kwargs,
        )

    @classmethod
    def return_receipt(cls, **kwargs):
        """Створює чек повернення."""
        return cls(
            receipt_type=ReceiptType.RETURN,
            is_return=True,
            **kwargs,
        )
