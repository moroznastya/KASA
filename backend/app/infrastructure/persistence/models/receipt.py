"""
Моделі Receipt та ReceiptItem (Чек продажу / повернення).

Фіксує кожну транзакцію на касі: продаж або повернення товару.
"""

import uuid
from datetime import datetime
from enum import StrEnum
from typing import TYPE_CHECKING

import sqlalchemy as sa
from sqlalchemy import (
    Boolean,
    DateTime,
    Enum,
    ForeignKey,
    Numeric,
    String,
    Text,
)
from sqlalchemy.dialects.postgresql import UUID
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.debtor import Debtor
    from app.infrastructure.persistence.models.product import Product
    from app.infrastructure.persistence.models.user import User


class ReceiptType(StrEnum):
    """Тип чеку."""
    SALE = "sale"           # Продаж
    RETURN = "return"       # Повернення


class ReceiptPaymentMethod(StrEnum):
    """Спосіб оплати в чеку."""
    CASH = "cash"        # Готівка
    CARD = "card"        # Картка
    MIXED = "mixed"      # Готівка + картка


class FiscalStatus(StrEnum):
    """Статус відправки фіскального чеку у податкову."""
    NONE = "none"        # Не фіскальний / не потребує відправки
    PENDING = "pending"  # Очікує відправки у податкову
    SENT = "sent"        # Успішно відправлено у податкову
    FAILED = "failed"    # Помилка при відправці у податкову
    FISCALIZED = "fiscalized"  # Тимчасова заглушка: чек позначено фіскалізованим (ПРРО не підключений)


class Receipt(Base):
    """Чек продажу або повернення."""

    __tablename__ = "receipts"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор чеку",
    )

    # ── Ідемпотентність push (offline-first sync, дизайн 8.2) ────────────
    client_uuid: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        nullable=True,
        comment="UUID транзакції з каси — ключ ідемпотентного прийому push",
    )

    __table_args__ = (
        sa.Index(
            "uq_receipts_client_uuid",
            "client_uuid",
            unique=True,
            postgresql_where=sa.text("client_uuid IS NOT NULL"),
        ),
    )
    receipt_number: Mapped[str] = mapped_column(
        String(50),
        nullable=False,
        index=True,
        comment="Номер чеку (фіскальний або внутрішній)",
    )
    receipt_type: Mapped[ReceiptType] = mapped_column(
        Enum(ReceiptType, name="receipt_type", create_constraint=True, values_callable=lambda x: [e.value for e in x]),
        default=ReceiptType.SALE,
        nullable=False,
        comment="Тип чеку: продаж або повернення",
    )
    cashier_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("users.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор касира, який пробив чек",
    )
    total_amount: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        comment="Загальна сума чеку (грн)",
    )
    paid_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        nullable=True,
        comment="Фактично сплачена сума (грн). Якщо менша за total_amount — різниця в борг",
    )
    change_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        nullable=True,
        default=0.00,
        comment="Сума здачі (грн). Якщо paid_amount > total_amount",
    )
    debtor_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("debtors.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="ID боржника (якщо покупка в борг)",
    )
    is_return: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        comment="Чи є цей чек поверненням",
    )
    notes: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Додаткові нотатки до чеку",
    )
    payment_method: Mapped[ReceiptPaymentMethod | None] = mapped_column(
        Enum(
            ReceiptPaymentMethod,
            name="receipt_payment_method",
            create_constraint=True,
            values_callable=lambda x: [e.value for e in x],
        ),
        nullable=True,
        default=None,
        comment="Спосіб оплати: cash, card, mixed",
    )
    cash_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        nullable=True,
        comment="Сума оплати готівкою (грн). Заповнюється для cash/mixed чеків",
    )
    card_amount: Mapped[float | None] = mapped_column(
        Numeric(12, 2),
        nullable=True,
        comment="Сума оплати карткою (грн). Заповнюється для card/mixed чеків",
    )

    # ── Дані банківської транзакції карткового терміналу (ПриватБанк) ──
    terminal_rrn: Mapped[str | None] = mapped_column(
        String(32),
        nullable=True,
        comment="RRN транзакції терміналу (унікальний номер транзакції банку)",
    )
    terminal_approval_code: Mapped[str | None] = mapped_column(
        String(16),
        nullable=True,
        comment="Код авторизації терміналу",
    )
    terminal_invoice_number: Mapped[str | None] = mapped_column(
        String(32),
        nullable=True,
        comment="Номер чека терміналу (може перевищувати 32-bit, зберігається як рядок)",
    )
    terminal_transaction_id: Mapped[str | None] = mapped_column(
        String(64),
        nullable=True,
        comment="Ідентифікатор транзакції в банку-емітенті (rrnExt / rid)",
    )
    terminal_response_code: Mapped[str | None] = mapped_column(
        String(8),
        nullable=True,
        comment='ResponseCode відповіді терміналу ("0000" — успіх тощо)',
    )
    terminal_status: Mapped[str | None] = mapped_column(
        String(16),
        nullable=True,
        comment='Статус транзакції (trnStatus: "1" — успіх; або наш статус approved/declined/partial/cancelled)',
    )
    terminal_receipt: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Повний текст чека терміналу (для друку)",
    )
    terminal_card_pan: Mapped[str | None] = mapped_column(
        String(32),
        nullable=True,
        comment="Маскований номер картки (pan)",
    )
    terminal_payment_system: Mapped[str | None] = mapped_column(
        String(16),
        nullable=True,
        comment="Міжнародна платіжна система (VISA/MasterCard)",
    )
    terminal_merchant: Mapped[str | None] = mapped_column(
        String(32),
        nullable=True,
        comment="Номер мерчанта",
    )
    terminal_created_at: Mapped[datetime | None] = mapped_column(
        DateTime,
        nullable=True,
        comment="Дата/час транзакції від терміналу",
    )
    original_receipt_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("receipts.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="ID оригінального чеку (для повернень)",
    )
    return_reason: Mapped[str | None] = mapped_column(
        String(255),
        nullable=True,
        comment="Причина повернення",
    )

    # ── Фіскальні дані (відправка у податкову) ──
    is_fiscal: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        nullable=False,
        comment="Чек є фіскальним (містить лише товари з фіскальних накладних)",
    )
    fiscal_status: Mapped[FiscalStatus] = mapped_column(
        Enum(
            FiscalStatus,
            name="fiscal_status",
            create_constraint=True,
            values_callable=lambda x: [e.value for e in x],
        ),
        default=FiscalStatus.NONE,
        nullable=False,
        comment="Статус відправки фіскального чеку у податкову",
    )
    fiscal_number: Mapped[str | None] = mapped_column(
        String(50),
        nullable=True,
        comment="Фіскальний номер чеку, присвоєний податковою",
    )
    fiscal_serial: Mapped[str | None] = mapped_column(
        String(50),
        nullable=True,
        comment="Фіскальний серійний номер",
    )
    fiscal_sent_at: Mapped[datetime | None] = mapped_column(
        DateTime,
        nullable=True,
        comment="Дата/час успішної відправки у податкову",
    )
    fiscal_error: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Текст помилки при відправці у податкову",
    )
    split_group_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("receipts.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="ID пов'язаного чеку при розділенні фіскальних/нефіскальних позицій (обидва чеки однієї продажі)",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата та час продажу",
    )

    # ── Зв'язки ─────────────────────────────────
    cashier: Mapped["User"] = relationship(
        "User",
        back_populates="receipts",
    )
    debtor: Mapped["Debtor | None"] = relationship(
        "Debtor",
        back_populates="receipts",
    )
    items: Mapped[list["ReceiptItem"]] = relationship(
        "ReceiptItem",
        back_populates="receipt",
        cascade="all, delete-orphan",
    )

    def __repr__(self) -> str:
        return f"<Receipt {self.receipt_number} ({self.receipt_type.value})>"


class ReceiptItem(Base):
    """Позиція чеку (один товар)."""

    __tablename__ = "receipt_items"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор позиції",
    )
    receipt_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("receipts.id", ondelete="CASCADE"),
        nullable=False,
        index=True,
        comment="Ідентифікатор чеку",
    )
    product_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("products.id", ondelete="RESTRICT"),
        nullable=False,
        index=True,
        comment="Ідентифікатор товару",
    )
    quantity: Mapped[float] = mapped_column(
        Numeric(10, 3),
        nullable=False,
        comment="Кількість товару",
    )
    price: Mapped[float] = mapped_column(
        Numeric(10, 2),
        nullable=False,
        comment="Ціна за одиницю (грн)",
    )
    total: Mapped[float] = mapped_column(
        Numeric(12, 2),
        nullable=False,
        comment="Загальна сума позиції (грн)",
    )
    purchase_price: Mapped[float | None] = mapped_column(
        Numeric(10, 2),
        nullable=True,
        comment="Собівартість товару на момент продажу (грн)",
    )
    fiscal_quantity: Mapped[float] = mapped_column(
        Numeric(10, 3),
        nullable=False,
        default=0,
        server_default=sa.text('0'),
        comment="Фіскалізована кількість позиції (0 = нефіскальна; 0<fiscal_quantity<quantity = часткова фіскалізація; =quantity = повністю фіскальна)",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )

    # ── Зв'язки ─────────────────────────────────
    receipt: Mapped["Receipt"] = relationship(
        "Receipt",
        back_populates="items",
    )
    product: Mapped["Product"] = relationship(
        "Product",
        back_populates="receipt_items",
    )

    def __repr__(self) -> str:
        return f"<ReceiptItem {self.product_id} x{self.quantity}>"
