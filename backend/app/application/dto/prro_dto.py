"""
DTO для ПРРО (програмний РРО): налаштування, зміни, фіскалізація, статус.

Використовуються для передачі даних між Application та Presentation шарами
(API v2). Pydantic v2 BaseModel — можуть бути використані як response_model
FastAPI без додаткових конвертацій.

ВАЖЛИВО:
  - Пароль ключа ПРРО НІКОЛИ не передається у відповідях — тільки маска
    PrroSettingsDTO.key_password_masked = "••••".
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from uuid import UUID

from pydantic import BaseModel, Field


class PrroSettingsDTO(BaseModel):
    """Налаштування ПРРО для відображення в UI/API (пароль замасковано)."""

    key_file: str | None = Field(
        default=None,
        description="Шлях до файлу ключа КЕП (pfx/p12/jks/pem/dat)",
    )
    key_password_masked: str | None = Field(
        default=None,
        description="Маска пароля ключа ('••••' — якщо пароль збережено)",
    )
    key_format: str | None = Field(
        default=None,
        description="Формат ключа: pfx/p12/jks/pem/dat",
    )
    prro_fn: str | None = Field(
        default=None,
        description="Фіскальний номер ПРРО (ФН)",
    )
    prro_tn: str | None = Field(
        default=None,
        description="Податковий номер платника ПДВ (ТН)",
    )
    prro_zn: str | None = Field(
        default=None,
        description="Заводський номер ПРРО (ЗН)",
    )
    mode: str = Field(
        default="test",
        description="Режим роботи: test / prod",
    )
    url: str | None = Field(
        default=None,
        description="Адреса фіскального сервера (залежить від mode)",
    )
    shift_open: bool = Field(
        default=False,
        description="Чи відкрита поточна зміна ПРРО",
    )
    online: bool = Field(
        default=False,
        description="ПРРО онлайн (за даними statusRro)",
    )
    auto_fiscalize: bool = Field(
        default=False,
        description=(
            "Автоматична фіскалізація чеків після створення продажу/повернення "
            "(за замовчуванням false на період розробки)"
        ),
    )


class PrroShiftDTO(BaseModel):
    """Зміна ПРРО (касова зміна / Z-звіт)."""

    id: UUID
    shift_number: int
    opened_at: datetime
    closed_at: datetime | None = None
    signer_name: str | None = None
    status: str = "open"
    receipt_count: int = 0
    total_amount: Decimal = Decimal("0")
    zreport_number: str | None = None

    model_config = {"from_attributes": True}


class FiscalizeRequestDTO(BaseModel):
    """Запит на фіскалізацію чеку."""

    receipt_id: UUID | None = Field(
        default=None,
        description="ID чеку (якщо не вказано — береться з URL)",
    )
    manual: bool = Field(
        default=False,
        description="Ручна фіскалізація (true) / автоматична (false)",
    )


class FiscalizeResponseDTO(BaseModel):
    """Результат фіскалізації чеку."""

    receipt_id: UUID
    fiscal_status: str = Field(
        description="Статус: none / sent / failed / pending",
    )
    fiscal_number: str | None = Field(
        default=None,
        description="Фіскальний номер чеку, присвоєний податковою",
    )
    fiscal_serial: str | None = Field(
        default=None,
        description="Фіскальний серійний номер (id_sign з CheckResponse)",
    )
    fiscal_sent_at: datetime | None = Field(
        default=None,
        description="Дата/час успішної відправки у податкову",
    )
    error: str | None = Field(
        default=None,
        description="Текст помилки при відправці",
    )
    split_receipt_id: UUID | None = Field(
        default=None,
        description="ID пов'язаного чеку при розділенні фіскальних/нефіскальних позицій",
    )
    fiscal_check_url: str | None = Field(
        default=None,
        description="URL перевірки фіскального чеку (для QR-коду на друку)",
    )
    warning: str | None = Field(
        default=None,
        description="Попередження (наприклад, часткова фіскалізація)",
    )


class PrroStatusDTO(BaseModel):
    """Статус ПРРО (statusRro/infoRro + локальний стан)."""

    open_shift: bool = Field(default=False, description="Зміна відкрита")
    online: bool = Field(default=False, description="ПРРО онлайн")
    last_signer: str | None = Field(
        default=None,
        description="Останній підписант (серійний номер ключа)",
    )
    name: str | None = Field(default=None, description="Назва ПРРО")
    addr: str | None = Field(default=None, description="Адреса ТО")
    fn: str | None = Field(default=None, description="Фіскальний номер ПРРО")


class OpenShiftRequestDTO(BaseModel):
    """Запит на відкриття зміни ПРРО."""

    comment: str | None = Field(
        default=None,
        description="Коментар (наприклад, ПІБ касира)",
    )


class CloseShiftRequestDTO(BaseModel):
    """Запит на закриття зміни ПРРО (Z-звіт)."""

    comment: str | None = Field(
        default=None,
        description="Коментар (наприклад, хто закриває зміну)",
    )


class PrroQueueItemDTO(BaseModel):
    """Запис журналу офлайн-черги ПРРО."""

    id: UUID
    receipt_id: UUID | None = None
    shift_id: UUID | None = None
    local_number: int = 0
    check_type: str = "CHK"
    status: str = "pending"
    error: str | None = None
    created_at: datetime | None = None
    sent_at: datetime | None = None

    model_config = {"from_attributes": True}
