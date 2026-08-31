"""
Фабрики для створення тестових постачальників (Supplier).

Використовує factory_boy для генерації даних.
"""

from datetime import datetime
from uuid import uuid4

import factory

from app.infrastructure.persistence.models.supplier import Supplier


class SupplierFactory(factory.Factory):
    """Фабрика для створення тестових постачальників."""

    class Meta:
        model = Supplier

    id = factory.LazyFunction(uuid4)
    name = factory.Sequence(lambda n: f'ТОВ "Постачальник {n:03d}"')
    edrpou = factory.Sequence(lambda n: f"{n:08d}")
    phone = factory.Faker("phone_number", locale="uk_UA")
    email = factory.LazyAttribute(lambda o: f"info@{o.name.lower().replace(' ', '')}.com")
    address = factory.Faker("address", locale="uk_UA")
    notes = factory.Faker("sentence", locale="uk_UA")
    created_at = factory.LazyFunction(datetime.utcnow)
    updated_at = factory.LazyFunction(datetime.utcnow)
