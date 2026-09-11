"""
Pydantic схеми для системних налаштувань.
"""
from __future__ import annotations

import uuid
from datetime import datetime

from pydantic import BaseModel, Field


class SystemSettingRead(BaseModel):
    """Схема для читання налаштування."""
    id: uuid.UUID
    module: str
    key: str
    value: str | None = None
    value_type: str = "string"
    label: str
    description: str | None = None
    options: str | None = None  # JSON-рядок для select
    is_active: bool = True
    created_at: datetime
    updated_at: datetime

    model_config = {"from_attributes": True}


class SystemSettingUpdate(BaseModel):
    """Схема для оновлення значення налаштування."""
    value: str | None = None


class SettingsModuleResponse(BaseModel):
    """Відповідь з налаштуваннями, згрупованими за модулями."""
    modules: dict[str, list[SystemSettingRead]]


class SystemSettingBatchUpdate(BaseModel):
    """Масове оновлення налаштувань."""
    settings: dict[str, str | None] = Field(
        ...,
        description="Словник key -> value для оновлення",
    )
