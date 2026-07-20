"""
Фабрики для створення тестових товарів (Product).

Використовує factory_boy для генерації даних.
"""

from uuid import uuid4
from decimal import Decimal
from datetime import datetime
from typing import Optional

import factory

from app.models.product import Product


class ProductFactory(factory.Factory):
    """Фабрика для створення тестових товарів."""

    class Meta:
        model = Product

    id = factory.LazyFunction(uuid4)
    barcode = factory.Sequence(lambda n: f"482000{n:07d}")
    sku = factory.Sequence(lambda n: f"SKU-{n:05d}")
    title = factory.Sequence(lambda n: f"Тестовий товар #{n:03d}")
    description = factory.Faker("sentence", locale="uk_UA")
    price = Decimal("100.00")
    cost_price = Decimal("70.00")
    stock = Decimal("100.000")
    uktzed = "48200000"
    scan_excise = False
    tax_rate = Decimal("20.00")
    tax_group = "А"
    is_weight = False
    unit = "шт"
    category_id = None
    supplier_id = None
    created_at = factory.LazyFunction(datetime.utcnow)
    updated_at = factory.LazyFunction(datetime.utcnow)

    @classmethod
    def weight_product(cls, **kwargs):
        """Створює ваговий товар."""
        return cls(
            is_weight=True,
            unit="кг",
            stock=Decimal("50.000"),
            **kwargs,
        )

    @classmethod
    def with_zero_stock(cls, **kwargs):
        """Створює товар з нульовим залишком."""
        return cls(stock=Decimal("0.000"), **kwargs)

    @classmethod
    def with_low_stock(cls, **kwargs):
        """Створює товар з малим залишком."""
        return cls(stock=Decimal("5.000"), **kwargs)
