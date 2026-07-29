"""
Фабрики для створення тестових категорій (Category).

Використовує factory_boy для генерації даних.
"""

from uuid import uuid4
from datetime import datetime
from typing import Optional

import factory

from app.infrastructure.persistence.models.category import Category


class CategoryFactory(factory.Factory):
    """Фабрика для створення тестових категорій."""

    class Meta:
        model = Category

    id = factory.LazyFunction(uuid4)
    name = factory.Sequence(lambda n: f"Категорія {n:03d}")
    description = factory.Faker("sentence", locale="uk_UA")
    parent_id = None
    created_at = factory.LazyFunction(datetime.utcnow)
    updated_at = factory.LazyFunction(datetime.utcnow)

    @classmethod
    def child_of(cls, parent: Category, **kwargs):
        """Створює дочірню категорію."""
        return cls(parent_id=parent.id, **kwargs)

    @classmethod
    def root(cls, **kwargs):
        """Створює кореневу категорію (без батька)."""
        return cls(parent_id=None, **kwargs)
