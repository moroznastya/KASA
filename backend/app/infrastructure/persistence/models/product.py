"""
Модель Product (Товар) — Super-Product Model.

Це центральна модель системи, яка об'єднує всі характеристики товару:
  - Штрих-коди (окрема таблиця barcodes)
  - Категорія (деревоподібна структура)
  - Постачальник
  - Ціни, податки, акциз
  - Зображення (окрема таблиця product_images)
  - Одиниці виміру та ваговий режим
  - Фіскальний облік (товари з фіскальних накладних)
"""

import uuid
from datetime import datetime

from sqlalchemy import (
    ForeignKey,
    String,
    Numeric,
    Boolean,
    Text,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.dialects.postgresql import UUID

from app.database import Base


class Product(Base):
    """Товар (номенклатурна одиниця)."""

    __tablename__ = "products"

    # ── Поля ────────────────────────────────────
    id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        primary_key=True,
        default=uuid.uuid4,
        comment="Унікальний ідентифікатор товару",
    )
    barcode: Mapped[str | None] = mapped_column(
        String(50),
        unique=True,
        nullable=True,
        index=True,
        comment="Основний штрих-код товару (EAN-13)",
    )
    sku: Mapped[str | None] = mapped_column(
        String(100),
        unique=True,
        nullable=True,
        index=True,
        comment="Артикул товару (внутрішній код)",
    )
    title: Mapped[str] = mapped_column(
        String(255),
        nullable=False,
        index=True,
        comment="Назва товару",
    )
    description: Mapped[str | None] = mapped_column(
        Text,
        nullable=True,
        comment="Опис товару",
    )

    # ── Ціни та фінанси ─────────────────────────
    price: Mapped[float | None] = mapped_column(
        Numeric(10, 2),
        default=0.00,
        comment="Роздрібна ціна (грн)",
    )
    cost_price: Mapped[float | None] = mapped_column(
        Numeric(10, 2),
        default=0.00,
        comment="Собівартість / закупівельна ціна (грн)",
    )
    markup: Mapped[float | None] = mapped_column(
        Numeric(5, 2),
        default=0.00,
        comment="Націнка (%)",
    )

    # ── Облік залишків ──────────────────────────
    stock: Mapped[float | None] = mapped_column(
        Numeric(10, 3),
        default=0.000,
        comment="Поточний залишок на складі (в одиницях виміру товару)",
    )
    recommended_qty: Mapped[float | None] = mapped_column(
        Numeric(10, 3),
        default=0.000,
        nullable=True,
        comment="Рекомендований залишок (мінімальний залишок для замовлення)",
    )

    # ── Фіскальний облік ────────────────────────
    is_fiscal: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        nullable=False,
        comment="Ознака: товар надходив з фіскальної накладної",
    )
    fiscal_stock: Mapped[float] = mapped_column(
        Numeric(10, 3),
        default=0.000,
        nullable=False,
        comment="Кількість у поточному залишку, що надійшла з фіскальних накладних",
    )

    # ── Податки та акциз ────────────────────────
    uktzed: Mapped[str | None] = mapped_column(
        String(10),
        nullable=True,
        comment="Код УКТЗЕД (для митного оформлення та звітності)",
    )
    scan_excise: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        comment="Чи потрібно сканувати акцизну марку при продажу",
    )
    tax_rate: Mapped[float | None] = mapped_column(
        Numeric(5, 2),
        default=0.00,
        comment="Ставка ПДВ (%)",
    )
    tax_group: Mapped[str | None] = mapped_column(
        String(2),
        default="А",
        comment="Група оподаткування (А, Б, В тощо)",
    )

    # ── Фізичні характеристики ──────────────────
    is_weight: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        comment="Чи є товар ваговим (продається за вагою, а не за штуками)",
    )
    unit: Mapped[str | None] = mapped_column(
        String(10),
        default="шт",
        comment="Одиниця виміру (шт, кг, л, м тощо)",
    )

    # ── Зовнішні ключі ──────────────────────────
    category_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("categories.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="Ідентифікатор категорії товару",
    )
    supplier_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True),
        ForeignKey("suppliers.id", ondelete="SET NULL"),
        nullable=True,
        index=True,
        comment="Ідентифікатор постачальника",
    )

    # ── Timestamps ──────────────────────────────
    created_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        comment="Дата створення",
    )
    updated_at: Mapped[datetime] = mapped_column(
        default=datetime.utcnow,
        onupdate=datetime.utcnow,
        comment="Дата останнього оновлення",
    )

    # ── Зв'язки (relationships) ─────────────────
    category: Mapped["Category | None"] = relationship(
        "Category",
        back_populates="products",
    )
    supplier: Mapped["Supplier | None"] = relationship(
        "Supplier",
        back_populates="products",
    )
    barcodes: Mapped[list["Barcode"]] = relationship(
        "Barcode",
        back_populates="product",
        cascade="all, delete-orphan",
    )
    images: Mapped[list["ProductImage"]] = relationship(
        "ProductImage",
        back_populates="product",
        cascade="all, delete-orphan",
        order_by="ProductImage.sort_order",
    )

    # ── Зв'язки з документами ───────────────────
    invoice_items: Mapped[list["InvoiceItem"]] = relationship(
        "InvoiceItem",
        back_populates="product",
    )
    transfer_items: Mapped[list["TransferItem"]] = relationship(
        "TransferItem",
        back_populates="product",
    )
    write_off_items: Mapped[list["WriteOffItem"]] = relationship(
        "WriteOffItem",
        back_populates="product",
    )
    return_items: Mapped[list["ReturnInvoiceItem"]] = relationship(
        "ReturnInvoiceItem",
        back_populates="product",
    )
    receipt_items: Mapped[list["ReceiptItem"]] = relationship(
        "ReceiptItem",
        back_populates="product",
    )

    inventory_items: Mapped[list["InventoryItem"]] = relationship(
        "InventoryItem",
        back_populates="product",
    )

    def __repr__(self) -> str:
        return f"<Product {self.title}>"
