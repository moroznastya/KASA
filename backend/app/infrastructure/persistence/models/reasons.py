"""
Модель WriteOffReason (Персистентний список причин списання).

Користувач може обрати причину зі списку або додати нову,
яка зберігається в БД і доступна в наступних накладних.
"""

import uuid
from datetime import datetime

from sqlalchemy import Boolean, String
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column

from app.database import Base


class WriteOffReason(Base):
    """Причина списання товару (персистентний довідник)."""

    __tablename__ = "write_off_reasons"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор причини",
    )
    name: Mapped[str] = mapped_column(
        String(100),
        unique=True,
        nullable=False,
        comment="Назва причини (унікальна)",
    )
    is_active: Mapped[bool] = mapped_column(
        Boolean,
        default=True,
        nullable=False,
        comment="Чи активна причина (показується у формі списання)",
    )
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )

    def __repr__(self) -> str:
        return f"<WriteOffReason {self.name}>"
