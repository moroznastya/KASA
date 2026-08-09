"""
Pydantic схеми для моделі PrintTemplate (шаблон друку чека).
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict


class PrintTemplateCreate(BaseModel):
    """Схема для створення нового шаблону друку."""
    name: str = Field(..., min_length=1, max_length=255, description="Назва шаблону")
    type: str = Field(..., description="Тип шаблону: receipt_58mm, receipt_80mm, return_receipt_58mm, fiscal, custom")
    content: str = Field(..., description="HTML-вміст шаблону з {{змінними}}")
    variables: Optional[list[dict]] = Field(
        None,
        description="Список змінних: [{\"key\": \"shop_name\", \"label\": \"Назва магазину\", \"default\": \"Мій магазин\"}]",
    )
    is_default: bool = Field(False, description="Чи є шаблоном за замовчуванням для свого типу")


class PrintTemplateUpdate(BaseModel):
    """Схема для оновлення шаблону друку. Всі поля опціональні."""
    name: Optional[str] = Field(None, min_length=1, max_length=255)
    content: Optional[str] = Field(None, description="HTML-вміст шаблону")
    variables: Optional[list[dict]] = None
    is_default: Optional[bool] = None
    is_active: Optional[bool] = None


class PrintTemplateResponse(BaseModel):
    """Відповідь з даними шаблону друку."""
    id: UUID
    name: str
    type: str
    content: str
    variables: Optional[list[dict]] = None
    is_default: bool
    is_active: bool
    created_at: datetime
    updated_at: datetime

    model_config = ConfigDict(from_attributes=True)


class TemplateRenderRequest(BaseModel):
    """Запит на рендер шаблону: словник змінних для підстановки."""
    data: dict[str, str] = Field(..., description="Змінні для підстановки: {\"shop_name\": \"Torgashka\", \"total\": \"100.00\", ...}")


class TemplateRenderResponse(BaseModel):
    """Відповідь після рендеру шаблону: готовий HTML."""
    html: str = Field(..., description="Згенерований HTML-вміст")
