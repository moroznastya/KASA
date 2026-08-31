"""
Pydantic схеми для моделі Category (Категорія товарів).

Підтримує ієрархічну структуру (дерево) через parent_id.
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


# ─── Create Schema ───────────────────────────────────────────────────────────
class CategoryCreate(BaseModel):
    """Схема створення нової категорії."""
    name: str = Field(..., max_length=255, description="Назва категорії")
    description: Optional[str] = Field(None, description="Опис категорії")
    parent_id: Optional[UUID] = Field(None, description="ID батьківської категорії")


class CategoryUpdate(BaseModel):
    """Схема оновлення категорії. Всі поля опціональні."""
    name: Optional[str] = Field(None, max_length=255, description="Назва категорії")
    description: Optional[str] = Field(None, description="Опис категорії")
    parent_id: Optional[UUID] = Field(None, description="ID батьківської категорії")


class CategoryResponse(BaseModel):
    """Схема відповіді з даними категорії."""
    id: UUID
    name: str
    description: Optional[str] = None
    parent_id: Optional[UUID] = None
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)


class CategoryTreeResponse(BaseModel):
    """Схема відповіді для дерева категорій з дочірніми елементами."""
    id: UUID
    name: str
    description: Optional[str] = None
    parent_id: Optional[UUID] = None
    children: list["CategoryTreeResponse"] = []
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)
