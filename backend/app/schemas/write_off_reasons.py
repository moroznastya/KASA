"""
Pydantic схеми для моделі WriteOffReason (довідник причин списання).
"""

from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict


class WriteOffReasonCreate(BaseModel):
    """Схема створення нової причини списання."""
    name: str = Field(
        ...,
        min_length=2,
        max_length=100,
        description="Назва причини (унікальна, від 2 символів)",
    )


class WriteOffReasonResponse(BaseModel):
    """Схема відповіді з даними причини списання."""
    id: UUID
    name: str
    is_active: bool
    created_at: datetime

    model_config = ConfigDict(from_attributes=True)
