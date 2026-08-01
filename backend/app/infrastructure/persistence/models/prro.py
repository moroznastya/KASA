"""
Моделі ПРРО: налаштування, зміни (касові зміни) та офлайн-черга.

Зберігає:
  - PrroSetting   — ключ-значення налаштувань ПРРО (ключі, ФН, режим роботи тощо)
  - PrroShift     — зміна ПРРО (аналог касової зміни) з обліком чеків та Z-звітів
  - PrroQueueItem — офлайн-черга фіскальних документів, що ще не передані у податкову
"""

from __future__ import annotations

import uuid
from datetime import datetime
from enum import Enum as PyEnum

from decimal import Decimal

from sqlalchemy import (
    ForeignKey,
    String,
    Text,
    Integer,
    Numeric,
    Enum,
    DateTime,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class PrroShiftStatus(str, PyEnum):
    """Статус зміни ПРРО."""
    OPEN = "open"        # Зміна відкрита
    CLOSED = "closed"    # Зміна закрита (Z-звіт)


class PrroQueueStatus(str, PyEnum):
    """Статус передачі фіскального документа у податкову."""
    PENDING = "pending"  # Очікує передачі
    SENT = "sent"        # Успішно передано
    FAILED = "failed"    # Помилка передачі


class PrroSetting(Base):
    """Налаштування ПРРО (ключ-значення)."""

    __tablename__ = "prro_settings"

    # ── Поля ────────────────────────────────────
    id: Mapped[int] = mapped_column(
        Integer,
        primary_key=True,
        autoincrement=True,
        comment="Унікальний ідентифікатор налаштування",
    )
    key_name: Mapped[str] = mapped_column(
        String(100),
        unique=True,
        nullable=False,
        index=True,
        comment=(
            "Ключ налаштування (key_file, key_password_encrypted, key_format, "
            "prro_fn, prro_tn, prro_zn, mode, url, last_shift_number, ...)"
        ),
    )
    value: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Значення налаштування (зберігається як текст)",
    )

    # ── Timestamps ──────────────────────────────
    updated_at: Mapped[datetime] = mapped_column(
        DateTime,
        default=datetime.utcnow,
        onupdate=datetime.utcnow,
        comment="Дата останнього оновлення",
    )

    def __repr__(self) -> str:
        return f"<PrroSetting {self.key_name}>"


class PrroShift(Base):
    """Зміна ПРРО (аналог касової зміни)."""

    __tablename__ = "prro_shifts"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор зміни",
    )
    shift_number: Mapped[int] = mapped_column(
        Integer,
        nullable=False,
        index=True,
        comment="Номер зміни",
    )
    opened_at: Mapped[datetime] = mapped_column(
        DateTime,
        nullable=False,
        comment="Дата/час відкриття зміни",
    )
    closed_at: Mapped[datetime | None] = mapped_column(
        DateTime,
        nullable=True,
        comment="Дата/час закриття зміни",
    )
    signer_serial: Mapped[str | None] = mapped_column(
        String(255),
        nullable=True,
        comment="Серійний номер КЕП підписанта",
    )
    signer_name: Mapped[str | None] = mapped_column(
        String(255),
        nullable=True,
        comment="ПІБ підписанта",
    )
    closed_by: Mapped[str | None] = mapped_column(
        String(255),
        nullable=True,
        comment="Хто закрив зміну (касир/старший касир)",
    )
    zreport_number: Mapped[str | None] = mapped_column(
        String(50),
        nullable=True,
        comment="Номер Z-звіту",
    )
    status: Mapped[PrroShiftStatus] = mapped_column(
        Enum(
            PrroShiftStatus,
            name="prro_shift_status",
            create_constraint=True,
            values_callable=lambda x: [e.value for e in x],
        ),
        default=PrroShiftStatus.OPEN,
        nullable=False,
        comment="Статус зміни: open / closed",
    )
    receipt_count: Mapped[int] = mapped_column(
        Integer,
        default=0,
        nullable=False,
        comment="Кількість фіскальних чеків за зміну",
    )
    total_amount: Mapped[Decimal] = mapped_column(
        Numeric(12, 2),
        default=0,
        nullable=False,
        comment="Обіг за зміну (грн)",
    )
    last_local_number: Mapped[int] = mapped_column(
        Integer,
        default=0,
        nullable=False,
        comment="Останній локальний номер чеку (для контролю послідовності)",
    )
    last_mac: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="MAC/хеш останнього переданого <DAT> (для хеш-ланцюжка)",
    )

    # ── Зв'язки ─────────────────────────────────
    queue_items: Mapped[list[PrroQueueItem]] = relationship(
        "PrroQueueItem",
        back_populates="shift",
    )

    def __repr__(self) -> str:
        return f"<PrroShift #{self.shift_number} ({self.status.value})>"


class PrroQueueItem(Base):
    """Офлайн-черга фіскальних документів, що не передані у податкову."""

    __tablename__ = "prro_queue_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор запису черги",
    )
    receipt_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("receipts.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="Зв'язок з чеком",
    )
    shift_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("prro_shifts.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="Зміна ПРРО, до якої належить документ",
    )
    local_number: Mapped[int] = mapped_column(
        Integer,
        nullable=False,
        comment="Локальний номер документа в межах зміни",
    )
    check_type: Mapped[str] = mapped_column(
        String(10),
        nullable=False,
        comment="Тип фіскального документа: CHK / ZREPORT / SERVICECHK",
    )
    xml_body: Mapped[str] = mapped_column(
        Text,
        nullable=False,
        comment="Канонічний XML <DAT> (підписаний check_sign)",
    )
    mac: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Значення MAC (хеш-ланцюжок)",
    )
    status: Mapped[PrroQueueStatus] = mapped_column(
        Enum(
            PrroQueueStatus,
            name="prro_queue_status",
            create_constraint=True,
            values_callable=lambda x: [e.value for e in x],
        ),
        default=PrroQueueStatus.PENDING,
        nullable=False,
        index=True,
        comment="Статус передачі: pending / sent / failed",
    )
    error: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Текст помилки при передачі",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        DateTime,
        default=datetime.utcnow,
        comment="Дата створення запису",
    )
    sent_at: Mapped[datetime | None] = mapped_column(
        DateTime,
        nullable=True,
        comment="Дата/час успішної передачі",
    )

    # ── Зв'язки ─────────────────────────────────
    receipt: Mapped["Receipt | None"] = relationship("Receipt")
    shift: Mapped["PrroShift | None"] = relationship(
        "PrroShift",
        back_populates="queue_items",
    )

    def __repr__(self) -> str:
        return f"<PrroQueueItem {self.check_type} #{self.local_number} ({self.status.value})>"
