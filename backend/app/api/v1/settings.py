"""
API роутер для системних налаштувань Torgashka POS.

GET  /api/v1/settings          — отримати всі налаштування, згруповані за модулями
GET  /api/v1/settings/:module  — отримати налаштування конкретного модуля
PUT  /api/v1/settings          — масове оновлення налаштувань
PUT  /api/v1/settings/:key     — оновити конкретне налаштування (upsert)

БЕЗПЕКА:
  Значення, що зберігаються через PUT, проходять валідацію за ключем
  (SettingsValueValidator): числові діапазони, булеві значення, whitelist
  (barcode_type). Невірне значення → HTTP 422 з описом українською.
"""
from __future__ import annotations

import json

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.application.services.settings_value_validator import (
    validate_and_normalize_setting_value,
)
from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.infrastructure.persistence.models.user import User
from app.schemas.settings import (
    SettingsModuleResponse,
    SystemSettingBatchUpdate,
    SystemSettingRead,
    SystemSettingUpdate,
)

router = APIRouter(prefix="/settings", tags=["Settings"])


# ─── Відомий список ключів друку ─────────────────────────────────────────────
# Використовується для автоматичного визначення module='printing'
# при створенні нового налаштування (upsert).
PRINTING_KEYS = {
    "printer_name",
    "print_font_family",
    "default_template_type",
    "print_copies",
    "auto_cut_paper",
    "show_logo",
    "return_receipt_template_type",
    "receipt_print_copies",
    "report_print_copies",
    "price_tag_fields",
    "price_tag_width",
    "price_tag_height",
    "label_fields",
    "label_width",
    "label_height",
    "price_tag_gap",
    "label_gap",
    "price_tag_margin",
    "barcode_type",
    "price_tag_template_id",
    "label_template_id",
}


# ─── Допоміжні функції для upsert ────────────────────────────────────────────

def _determine_module(key: str) -> str:
    """
    Визначає модуль налаштування автоматично за ключем.

    Якщо ключ починається з 'price_tag_', 'label_', 'print_'
    або входить у відомий список ключів друку — модуль 'printing',
    інакше — 'general'.

    Args:
        key: ключ налаштування.

    Returns:
        Назва модуля ('printing' або 'general').
    """
    if (
        key.startswith("price_tag_")
        or key.startswith("label_")
        or key.startswith("print_")
        or key in PRINTING_KEYS
    ):
        return "printing"
    return "general"


def _determine_value_type(value: str | None) -> str:
    """
    Визначає тип значення налаштування автоматично за значенням.

    - true/false (case-insensitive) → 'boolean'
    - ціле число                      → 'number'
    - JSON-масив (list)               → 'string' (зберігається як JSON-текст)
    - інакше                          → 'string'

    Args:
        value: значення налаштування (як текст).

    Returns:
        Тип значення, сумісний з моделлю SystemSetting.value_type.
    """
    if value is None:
        return "string"

    value_stripped = value.strip()

    # Boolean: "true" / "false" (без урахування регістру)
    if value_stripped.lower() in ("true", "false"):
        return "boolean"

    # Number: ціле число (може бути з мінусом)
    if value_stripped.lstrip("-").isdigit():
        return "number"

    # JSON list: значення, що починається з '[' і є валідним JSON
    if value_stripped.startswith("["):
        try:
            parsed = json.loads(value_stripped)
            if isinstance(parsed, list):
                return "string"  # JSON-масив зберігається як текст
        except (json.JSONDecodeError, TypeError):
            pass

    # Все інше — рядок
    return "string"


def _humanize_key(key: str) -> str:
    """
    Перетворює ключ налаштування на людино-зрозумілу назву.

    Приклад: 'price_tag_width' → 'Price tag width'.

    Args:
        key: ключ налаштування.

    Returns:
        Людино-зрозуміла назва (для поля label).
    """
    return key.replace("_", " ").capitalize()


def _validate_setting_value_or_422(key: str, value: str | None) -> str | None:
    """
    Валідує значення налаштування; при помилці піднімає HTTP 422.

    Args:
        key: ключ налаштування.
        value: значення налаштування (як рядок).

    Returns:
        Нормалізоване значення для зберігання.

    Raises:
        HTTPException 422: якщо значення не проходить валідацію.
    """
    try:
        return validate_and_normalize_setting_value(key, value)
    except ValueError as exc:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            detail=str(exc),
        ) from exc


# ─── ЕНДПОІНТИ ───────────────────────────────────────────────────────────────

@router.get("", response_model=SettingsModuleResponse)
async def get_all_settings(
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Отримати всі активні налаштування, згруповані за модулями.
    """
    result = await session.execute(
        select(SystemSetting)
        .where(SystemSetting.is_active)
        .order_by(SystemSetting.module, SystemSetting.key)
    )
    settings = result.scalars().all()

    modules: dict[str, list[SystemSettingRead]] = {}
    for setting in settings:
        if setting.module not in modules:
            modules[setting.module] = []
        modules[setting.module].append(SystemSettingRead.model_validate(setting))

    return SettingsModuleResponse(modules=modules)


@router.get("/{module}", response_model=list[SystemSettingRead])
async def get_settings_by_module(
    module: str,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Отримати налаштування конкретного модуля (general, pos, тощо).
    """
    result = await session.execute(
        select(SystemSetting)
        .where(SystemSetting.module == module, SystemSetting.is_active)
        .order_by(SystemSetting.key)
    )
    settings = result.scalars().all()
    if not settings:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Модуль '{module}' не знайдено або він порожній",
        )
    return [SystemSettingRead.model_validate(s) for s in settings]


@router.put("", response_model=SettingsModuleResponse)
async def batch_update_settings(
    data: SystemSettingBatchUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Масове оновлення налаштувань.

    Очікує словник {key: value} для оновлення декількох налаштувань одночасно.
    Кожне значення проходить валідацію за ключем — при помилці повертається
    HTTP 422 з описом українською.
    """
    if current_user.role != "admin":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Тільки адміністратор може змінювати налаштування",
        )

    updated_keys = []
    for key, value in data.settings.items():
        # Валідуємо та нормалізуємо значення перед збереженням
        normalized_value = _validate_setting_value_or_422(key, value)

        result = await session.execute(
            select(SystemSetting).where(SystemSetting.key == key)
        )
        setting = result.scalar_one_or_none()
        if setting:
            setting.value = normalized_value
            updated_keys.append(key)

    if updated_keys:
        await session.commit()

    # Повертаємо оновлені налаштування
    result = await session.execute(
        select(SystemSetting)
        .where(SystemSetting.is_active)
        .order_by(SystemSetting.module, SystemSetting.key)
    )
    settings = result.scalars().all()
    modules: dict[str, list[SystemSettingRead]] = {}
    for setting in settings:
        if setting.module not in modules:
            modules[setting.module] = []
        modules[setting.module].append(SystemSettingRead.model_validate(setting))

    return SettingsModuleResponse(modules=modules)


@router.put("/{key}", response_model=SystemSettingRead)
async def update_setting(
    key: str,
    data: SystemSettingUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Оновити конкретне налаштування за ключем (upsert).

    Значення проходить валідацію за ключем:
      - price_tag_width/height, label_width/height → int 10..200
      - price_tag_gap, label_gap                   → int 0..20
      - price_tag_margin                           → int 0..50
      - print_copies                               → int 1..100
      - barcode_type                               → whitelist ["code128", "qr"]
      - auto_cut_paper, show_logo                  → bool ("true"/"false"/"1"/"0")
    Невірне значення → HTTP 422 з описом українською.

    Якщо налаштування з таким ключем не існує — воно СТВОРЮЄТЬСЯ автоматично:
    - module визначається автоматично за ключем (printing / general)
    - value_type визначається автоматично за значенням (boolean / number / string)
    - label генерується з ключа

    Якщо налаштування існує — оновлюється тільки значення (value).
    """
    if current_user.role != "admin":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Тільки адміністратор може змінювати налаштування",
        )

    # ─── Валідація значення за ключем (HTTP 422 при помилці) ────────────────
    normalized_value = _validate_setting_value_or_422(key, data.value)

    # Шукаємо налаштування за ключем
    result = await session.execute(
        select(SystemSetting).where(SystemSetting.key == key)
    )
    setting = result.scalar_one_or_none()

    if not setting:
        # ─── Upsert: створюємо нове налаштування ──────────────────────────
        setting = SystemSetting(
            key=key,
            value=normalized_value,
            module=_determine_module(key),
            value_type=_determine_value_type(normalized_value),
            label=_humanize_key(key),
            description=None,
            options=None,
            is_active=True,
        )
        session.add(setting)
    else:
        # ─── Оновлюємо існуюче налаштування ───────────────────────────────
        setting.value = normalized_value

    await session.commit()
    await session.refresh(setting)

    return SystemSettingRead.model_validate(setting)
