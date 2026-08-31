"""
Модель WorkSession (Робоча сесія касира).

Фіксує вхід/вихід касира та автоматично розраховує тривалість зміни.
Створюється при login, завершується при logout.
"""

import uuid
from datetime import datetime
from typing import TYPE_CHECKING

from sqlalchemy import DateTime, ForeignKey, Numeric
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.user import User


class WorkSession(Base):
    """Робоча сесія касира — період між login та logout."""

    __tablename__ = "work_sessions"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор робочої сесії",
    )
    user_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("users.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор користувача (касира)",
    )
    login_time: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        default=datetime.utcnow,
        comment="Час входу (початок зміни)",
    )
    logout_time: Mapped[datetime | None] = mapped_column(
        DateTime,
        nullable=True,
        default=None,
        comment="Час виходу (завершення зміни). Якщо None — сесія активна",
    )
    duration_hours: Mapped[float | None] = mapped_column(
        Numeric(5, 2),
        nullable=True,
        default=None,
        comment="Тривалість сесії в годинах (розраховується при logout)",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        DateTime,
        default=datetime.utcnow,
        comment="Дата створення запису",
    )

    # ── Зв'язки ─────────────────────────────────
    user: Mapped["User"] = relationship(
        "User",
        back_populates="work_sessions",
    )

    def __repr__(self) -> str:
        status = "active" if self.logout_time is None else "closed"
        return f"<WorkSession user={self.user_id} {status}>"
