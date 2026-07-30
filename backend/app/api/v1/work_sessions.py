"""
API роутер для робочих сесій (WorkSession).

Ендпоінти:
  - GET /work-sessions/my      — сесії поточного користувача за місяць
  - GET /work-sessions/report  — звіт по всіх касирах (admin only)
"""

from datetime import datetime, date
from typing import Optional
from uuid import UUID

from fastapi import APIRouter, Depends, Query, HTTPException, status
from sqlalchemy import select, func
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload

from app.database import get_session
from app.infrastructure.persistence.models.user import User
from app.infrastructure.persistence.models.work_session import WorkSession
from app.schemas.work_session import (
    WorkSessionResponse,
    WorkSessionReportResponse,
    WorkSessionDetail,
    UserHoursSummary,
)
from app.domain.services.auth_service import AuthService

router = APIRouter(
    prefix="/work-sessions",
    tags=["Work Sessions"],
)


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
    - `sessions`: список сесій
    - `total_hours`: загальна кількість відпрацьованих годин
    - `hourly_rate`: ставка користувача (грн/год)
    """
    # Визначаємо початок та кінець місяця
    month_start = datetime(year, month, 1)
    if month == 12:
        month_end = datetime(year + 1, 1, 1)
    else:
        month_end = datetime(year, month + 1, 1)

    # Отримуємо всі сесії користувача за місяць
    result = await session.execute(
        select(WorkSession)
        .where(WorkSession.user_id == current_user.id)
        .where(WorkSession.login_time >= month_start)
        .where(WorkSession.login_time < month_end)
        .order_by(WorkSession.login_time.desc())
    )
    sessions = result.scalars().all()

    # Рахуємо загальну кількість годин
    total_hours = sum(
        (s.duration_hours or 0) for s in sessions
    )

    return {
        "sessions": [WorkSessionResponse.model_validate(s) for s in sessions],
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
    - `hourly_rate`: погодинна ставка
    - `salary`: total_hours * hourly_rate
    """
    # Визначаємо початок та кінець місяця
    month_start = datetime(year, month, 1)
    if month == 12:
        month_end = datetime(year + 1, 1, 1)
    else:
        month_end = datetime(year, month + 1, 1)

    # Отримуємо всіх активних користувачів (касирів та адмінів)
    users_result = await session.execute(
        select(User).order_by(User.name)
    )
    users = users_result.scalars().all()

    items = []
    for u in users:
        # Сума duration_hours за місяць для цього користувача
        hours_result = await session.execute(
            select(func.coalesce(func.sum(WorkSession.duration_hours), 0))
            .where(WorkSession.user_id == u.id)
            .where(WorkSession.login_time >= month_start)
            .where(WorkSession.login_time < month_end)
        )
        total_hours = float(hours_result.scalar() or 0)

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
