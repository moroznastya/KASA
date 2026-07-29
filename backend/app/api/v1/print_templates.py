"""
API роутер для управління шаблонами чеків друку (PrintTemplate).

Ендпоінти:
  - GET    /print-templates          — список активних шаблонів
  - GET    /print-templates/all      — всі шаблони (admin only)
  - GET    /print-templates/default  — шаблон за замовчуванням для типу
  - GET    /print-templates/{id}     — деталі шаблону
  - POST   /print-templates          — створити новий шаблон (admin only)
  - PUT    /print-templates/{id}     — оновити шаблон (admin only)
  - DELETE /print-templates/{id}     — видалити шаблон (soft delete, admin only)
  - POST   /print-templates/{id}/set-default — встановити як основний (admin only)
  - POST   /print-templates/{id}/render     — рендер шаблону з даними

ВАЖЛИВО: Статичні шляхи (/default, /all) визначені ДО динамічних (/{id}),
щоб уникнути конфлікту маршрутизації FastAPI.
"""

from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select, update, func
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.infrastructure.persistence.models.user import User
from app.infrastructure.persistence.models.print_template import PrintTemplate
from app.schemas.print_template import (
    PrintTemplateCreate,
    PrintTemplateUpdate,
    PrintTemplateResponse,
    TemplateRenderRequest,
    TemplateRenderResponse,
)
from app.services.auth_service import AuthService
from app.services.print_template_service import PrintTemplateService

router = APIRouter(
    prefix="/print-templates",
    tags=["Print Templates"],
)


# ─── CRUD: Список активних шаблонів ──────────────────────────────────────────

@router.get("")
async def list_active_templates(
    page: int = Query(1, ge=1, description="Номер сторінки"),
    size: int = Query(50, ge=1, le=1000, description="Кількість записів на сторінці"),
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Повертає список активних шаблонів друку (is_active == True) з пагінацією.

    Доступно будь-якому аутентифікованому користувачу.
    """
    offset = (page - 1) * size
    stmt = select(PrintTemplate).where(PrintTemplate.is_active == True).order_by(PrintTemplate.type, PrintTemplate.name).offset(offset).limit(size)
    result = await session.execute(stmt)
    templates = result.scalars().all()

    # total count
    count_stmt = select(func.count(PrintTemplate.id)).where(PrintTemplate.is_active == True)
    count_result = await session.execute(count_stmt)
    total = count_result.scalar()

    return {
        "items": [PrintTemplateResponse.model_validate(t) for t in templates],
        "total": total,
        "page": page,
        "page_size": size,
        "pages": max(1, (total + size - 1) // size),
    }


# ─── CRUD: Всі шаблони (admin only) ──────────────────────────────────────────

@router.get("/all", response_model=list[PrintTemplateResponse])
async def list_all_templates(
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Повертає всі шаблони друку, включаючи неактивні (тільки admin).
    """
    result = await session.execute(
        select(PrintTemplate).order_by(PrintTemplate.type, PrintTemplate.name)
    )
    templates = result.scalars().all()
    return [PrintTemplateResponse.model_validate(t) for t in templates]


# ─── Спеціальний: Отримати шаблон за замовчуванням для типу ──────────────────
# ВАЖЛИВО: Цей статичний маршрут визначено ДО /{template_id},
#          щоб уникнути конфлікту маршрутизації FastAPI.

@router.get("/default", response_model=PrintTemplateResponse)
async def get_default_template(
    type: str = Query(..., description="Тип шаблону: receipt_58mm, receipt_80mm, return_receipt_58mm, fiscal, custom"),
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Повертає шаблон за замовчуванням для вказаного типу.

    Спочатку шукає шаблон з is_default=True,
    якщо такого немає — повертає перший активний шаблон цього типу.
    """
    service = PrintTemplateService(session)
    template = await service.get_default_for_type(type)

    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Не знайдено жодного активного шаблону для типу '{type}'",
        )

    return PrintTemplateResponse.model_validate(template)


# ─── CRUD: Деталі шаблону ────────────────────────────────────────────────────

@router.get("/{template_id}", response_model=PrintTemplateResponse)
async def get_template(
    template_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Повертає деталі шаблону друку за ID.

    Доступно будь-якому аутентифікованому користувачу.
    """
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == template_id)
    )
    template = result.scalar_one_or_none()
    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{template_id}' не знайдено",
        )
    return PrintTemplateResponse.model_validate(template)


# ─── CRUD: Створити шаблон (admin only) ──────────────────────────────────────

@router.post("", response_model=PrintTemplateResponse, status_code=status.HTTP_201_CREATED)
async def create_template(
    data: PrintTemplateCreate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Створює новий шаблон друку (тільки admin).

    Якщо is_default == True, то з інших шаблонів того самого типу
    прапорець is_default буде знято.
    """
    service = PrintTemplateService(session)

    # Якщо новий шаблон має бути основним — знімаємо is_default з інших
    if data.is_default:
        await session.execute(
            update(PrintTemplate)
            .where(PrintTemplate.type == data.type)
            .where(PrintTemplate.is_default == True)
            .values(is_default=False)
        )

    template = PrintTemplate(
        name=data.name,
        type=data.type,
        content=data.content,
        variables=data.variables,
        is_default=data.is_default,
    )
    session.add(template)
    await session.flush()

    return PrintTemplateResponse.model_validate(template)


# ─── CRUD: Оновити шаблон (admin only) ───────────────────────────────────────

@router.put("/{template_id}", response_model=PrintTemplateResponse)
async def update_template(
    template_id: UUID,
    data: PrintTemplateUpdate,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Оновлює шаблон друку (тільки admin).

    Якщо is_default встановлюється в True, то з інших шаблонів
    того самого типу прапорець is_default буде знято.
    """
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == template_id)
    )
    template = result.scalar_one_or_none()
    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{template_id}' не знайдено",
        )

    update_data = data.model_dump(exclude_unset=True)

    # Якщо встановлюємо is_default — знімаємо з інших шаблонів цього типу
    if update_data.get("is_default") is True:
        await session.execute(
            update(PrintTemplate)
            .where(PrintTemplate.type == template.type)
            .where(PrintTemplate.is_default == True)
            .where(PrintTemplate.id != template_id)
            .values(is_default=False)
        )

    for field, value in update_data.items():
        setattr(template, field, value)

    await session.flush()
    return PrintTemplateResponse.model_validate(template)


# ─── CRUD: Видалити шаблон (soft delete, admin only) ─────────────────────────

@router.delete("/{template_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_template(
    template_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Видаляє шаблон друку (soft delete) — встановлює is_active=False (тільки admin).

    Якщо шаблон не знайдено — повертає 404.
    """
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == template_id)
    )
    template = result.scalar_one_or_none()
    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{template_id}' не знайдено",
        )

    template.is_active = False
    await session.flush()


# ─── Спеціальний: Встановити як основний (admin only) ────────────────────────

@router.post("/{template_id}/set-default", response_model=PrintTemplateResponse)
async def set_template_default(
    template_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Встановлює шаблон як основний для свого типу (тільки admin).

    Знімає прапорець is_default з усіх інших шаблонів того самого типу.
    """
    service = PrintTemplateService(session)
    template = await service.set_as_default(template_id)

    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{template_id}' не знайдено",
        )

    return PrintTemplateResponse.model_validate(template)


# ─── Спеціальний: Рендер шаблону ─────────────────────────────────────────────

@router.post("/{template_id}/render", response_model=TemplateRenderResponse)
async def render_template(
    template_id: UUID,
    data: TemplateRenderRequest,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Рендер шаблону з переданими даними.

    Замінює всі {{variable}} у вмісті шаблону на відповідні значення.
    Повертає готовий HTML-рядок.

    Тіло запиту: { "data": { "shop_name": "Kasa", "total": "100.00", ... } }
    """
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == template_id)
    )
    template = result.scalar_one_or_none()
    if not template:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Шаблон з ID '{template_id}' не знайдено",
        )

    html = PrintTemplateService.render_template(template.content, data.data)

    return TemplateRenderResponse(html=html)
