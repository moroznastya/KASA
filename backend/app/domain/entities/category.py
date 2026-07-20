"""
Domain Entity: Category (Категорія товарів).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional
from uuid import UUID, uuid4


@dataclass
class Category:
    """
    Категорія товарів (ієрархічне дерево).

    Відповідає за:
    - Групування товарів за категоріями
    - Ієрархічну структуру (батьківська категорія)
    """

    id: UUID = field(default_factory=uuid4)
    name: str = ""
    parent_id: Optional[UUID] = None
    description: str = ""
    sort_order: int = 0
    is_active: bool = True

    @property
    def is_root(self) -> bool:
        """Чи є категорія кореневою (без батька)."""
        return self.parent_id is None

    @property
    def has_parent(self) -> bool:
        """Чи має категорія батьківську категорію."""
        return self.parent_id is not None

    def change_parent(self, parent_id: Optional[UUID]) -> None:
        """
        Змінює батьківську категорію.

        Args:
            parent_id: ID нової батьківської категорії (None — коренева).
        """
        self.parent_id = parent_id

    def deactivate(self) -> None:
        """Деактивує категорію."""
        self.is_active = False

    def activate(self) -> None:
        """Активує категорію."""
        self.is_active = True

    def __str__(self) -> str:
        return f"Category(id={self.id}, name='{self.name}')"

    def __repr__(self) -> str:
        return (
            f"Category(id={self.id}, name='{self.name}', "
            f"parent_id={self.parent_id})"
        )
