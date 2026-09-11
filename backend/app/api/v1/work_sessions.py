"""
API роутер для робочих сесій (WorkSession).

Ендпоінти:
  - GET /work-sessions/my            — сесії поточного користувача за місяць
  - GET /work-sessions/report        — звіт по всіх касирах (admin only)
  - GET /work-sessions/user/{id}     — drill-down: сесії конкретного користувача (admin only)
"""

from datetime import datetime
from typing import Optional
from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.database import get_session
from app.domain.services.auth_service import AuthService
from app.infrastructure.persistence.models.user import User
from app.infrastructure.persistence.models.work_session import WorkSession
from app.schemas.work_session import (
    UserHoursSummary,
    WorkSessionReportResponse,
    WorkSessionResponse,
)

router = APIRouter(
    prefix="/work-sessions",
    tags=["Work Sessions"],
)

# Максимальна «враховувана» тривалість активної сесії в годинах.
# Якщо сесія активна (logout_time IS NULL) довше цього порогу — її вважаємо
# «забутою» (користувач закрив програму без logout) і обрізаємо тривалість,
# щоб одна забута сесія не спотворювала звіт.
MAX_SESSION_HOURS = 24


def _effective_duration(ws: WorkSession, now: Optional[datetime] = None) -> Optional[float]:
    """
    «Ефективна» тривалість сесії в годинах (без запису в БД).

    - Закрита сесія (logout_time IS NOT NULL) → duration_hours з БД.
    - Активна сесія (logout_time IS NULL) → «жива» тривалість:
        now - login_time, але НЕ більше MAX_SESSION_HOURS
        (забута сесія обрізається порогом).
    """
    if ws.logout_time is not None:
        return float(ws.duration_hours) if ws.duration_hours is not None else 0.0

    now = now or datetime.utcnow()
    live_hours = (now - ws.login_time).total_seconds() / 3600
    return round(min(live_hours, MAX_SESSION_HOURS), 2)


def _month_bounds(month: int, year: int) -> tuple[datetime, datetime]:
    """Початок та кінець (ексклюзивний) вказаного місяця."""
    month_start = datetime(year, month, 1)
    month_end = datetime(year + 1, 1, 1) if month == 12 else datetime(year, month + 1, 1)
    return month_start, month_end


@router.get("/my")
async def get_my_sessions(
    month: int = Query(datetime.now().month, ge=1, le=12, description="Місяць (1-12)"),
    year: int = Query(datetime.now().year, ge=2020, description="Рік"),
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Повертає список робочих сесій поточного користувача за вказаний місяць.

    Параметри:
    - `month` (int, default=поточний): номер місяця
    - `year` (int, default=поточний): рік

    Відповідь містить:
    - `sessions`: список сесій (з полем `is_active` та «живою» тривалістю
      для активної сесії — БД не змінюється)
    - `total_hours`: загальна кількість відпрацьованих годин (з урахуванням
      живої тривалості активної сесії)
    - `hourly_rate`: ставка користувача (грн/год)
    """
    month_start, month_end = _month_bounds(month, year)
    now = datetime.utcnow()

    # Отримуємо всі сесії користувача за місяць
    result = await session.execute(
        select(WorkSession)
        .where(WorkSession.user_id == current_user.id)
        .where(WorkSession.login_time >= month_start)
        .where(WorkSession.login_time < month_end)
        .order_by(WorkSession.login_time.desc())
    )
    sessions = result.scalars().all()

    # Рахуємо тривалість кожної сесії (активна — «жива», без запису в БД)
    items: list[WorkSessionResponse] = []
    for s in sessions:
        resp = WorkSessionResponse.model_validate(s)
        resp.duration_hours = _effective_duration(s, now)
        resp.is_active = s.logout_time is None
        items.append(resp)

    total_hours = sum((i.duration_hours or 0) for i in items)

    return {
        "sessions": items,
        "total_hours": round(total_hours, 2),
        "hourly_rate": float(current_user.hourly_rate) if current_user.hourly_rate else None,
    }


@router.get("/report")
async def get_work_report(
    month: int = Query(..., ge=1, le=12, description="Місяць (1-12)"),
    year: int = Query(..., ge=2020, description="Рік"),
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Повертає звіт по всіх касирах за вказаний місяць (тільки admin).

    Для кожного касира:
    - `user_name`: ім'я
    - `total_hours`: сума відпрацьованих годин за місяць
      (активні сесії враховуються «живою» тривалістю, обрізаною MAX_SESSION_HOURS)
    - `hourly_rate`: погодинна ставка
    - `salary`: total_hours * hourly_rate
    """
    month_start, month_end = _month_bounds(month, year)
    now = datetime.utcnow()

    # Отримуємо всіх користувачів (касирів та адмінів)
    users_result = await session.execute(
        select(User).order_by(User.name)
    )
    users = users_result.scalars().all()

    items = []
    for u in users:
        # Сесії користувача за місяць — рахуємо в Python-циклі, щоб врахувати
        # «живу» тривалість активної сесії (користувачів небагато)
        sessions_result = await session.execute(
            select(WorkSession)
            .where(WorkSession.user_id == u.id)
            .where(WorkSession.login_time >= month_start)
            .where(WorkSession.login_time < month_end)
        )
        user_sessions = sessions_result.scalars().all()

        total_hours = sum(
            (_effective_duration(s, now) or 0) for s in user_sessions
        )

        hourly_rate = float(u.hourly_rate) if u.hourly_rate else 0
        salary = round(total_hours * hourly_rate, 2) if hourly_rate else 0

        items.append(UserHoursSummary(
            user_id=u.id,
            user_name=u.name,
            total_hours=round(total_hours, 2),
            hourly_rate=hourly_rate if hourly_rate else None,
            salary=salary if salary else None,
        ))

    return WorkSessionReportResponse(
        month=month,
        year=year,
        items=items,
    )


@router.get("/user/{user_id}")
async def get_user_sessions_report(
    user_id: UUID,
    month: int = Query(..., ge=1, le=12, description="Місяць (1-12)"),
    year: int = Query(..., ge=2020, description="Рік"),
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.require_admin),
):
    """
    Drill-down: сесії конкретного користувача за місяць (тільки admin).

    Параметри:
    - `user_id` (UUID): ідентифікатор користувача
    - `month` (int): номер місяця
    - `year` (int): рік

    Відповідь:
    - `user_id`, `user_name`
    - `total_hours`: сума годин за місяць (з живою тривалістю активної сесії)
    - `sessions`: список сесій, відсортованих за login_time DESC:
      `{ id, login_time, logout_time, duration_hours, is_active }`

    Помилки:
    - 404 — користувача з вказаним ID не знайдено
    """
    # 404, якщо користувача не існує
    user_result = await session.execute(
        select(User).where(User.id == user_id)
    )
    user = user_result.scalar_one_or_none()
    if not user:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Користувача з ID '{user_id}' не знайдено",
        )

    month_start, month_end = _month_bounds(month, year)
    now = datetime.utcnow()

    # Сесії користувача за місяць, відсортовані за login_time DESC
    result = await session.execute(
        select(WorkSession)
        .where(WorkSession.user_id == user_id)
        .where(WorkSession.login_time >= month_start)
        .where(WorkSession.login_time < month_end)
        .order_by(WorkSession.login_time.desc())
    )
    sessions = result.scalars().all()

    items = []
    total_hours = 0.0
    for s in sessions:
        duration = _effective_duration(s, now)
        total_hours += duration or 0
        items.append({
            "id": s.id,
            "login_time": s.login_time,
            "logout_time": s.logout_time,
            "duration_hours": duration,
            "is_active": s.logout_time is None,
        })

    return {
        "user_id": user_id,
        "user_name": user.name,
        "total_hours": round(total_hours, 2),
        "sessions": items,
    }
