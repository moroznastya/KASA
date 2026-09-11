"""
Domain Entity: Product (Товар).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4

from ..value_objects.barcode import Barcode
from ..value_objects.money import Money
from ..value_objects.quantity import Quantity
from ..value_objects.tax_rate import TaxRate


@dataclass
class Product:
    """
    Товар (Product aggregate root).

    Відповідає за:
    - Ідентифікацію товару (ID, назва, штрих-код)
    - Ціноутворення (ціна, собівартість, ПДВ)
    - Складський облік (залишок)
    - Відстеження фіскальних надходжень (is_fiscal, fiscal_stock)
    - Категоризацію (категорія, постачальник)
    """

    id: UUID = field(default_factory=uuid4)
    name: str = ""
    barcode: Optional[Barcode] = None
    price: Optional[Money] = None
    cost_price: Optional[Money] = None
    stock: Optional[Quantity] = None
    category_id: Optional[UUID] = None
    supplier_id: Optional[UUID] = None
    tax_rate: TaxRate = field(default_factory=TaxRate.standard)
    sku: str = ""
    unit: str = "шт"
    is_active: bool = True
    description: str = ""
    # ── Фіскальне відстеження ──────────────────────────────────────────────
    is_fiscal: bool = False
    """Ознака: товар хоча б раз надходив з фіскальної накладної."""
    fiscal_stock: Optional[Quantity] = None
    """Кількість у поточному залишку, що надійшла з фіскальних накладних."""

    def update_stock(self, quantity: Quantity) -> None:
        """
        Оновлює залишок товару.

        Args:
            quantity: Нова кількість (може бути від'ємною для зменшення).

        Raises:
            ValueError: Якщо одиниці виміру не співпадають.
        """
        if self.stock is None:
            self.stock = quantity
        else:
            if self.stock.unit != quantity.unit:
                raise ValueError(
                    f"Unit mismatch: {self.stock.unit} vs {quantity.unit}"
                )
            self.stock = self.stock + quantity

    def update_fiscal_stock(self, quantity: Quantity) -> None:
        """
        Оновлює фіскальний залишок товару (кількість, що надійшла
        з фіскальних накладних).

        Args:
            quantity: Зміна кількості (додатна — надходження,
                      від'ємна — списання при продажу/поверненні постачальнику).

        Raises:
            ValueError: Якщо одиниці виміру не співпадають або фіскальний
                        залишок стає від'ємним.
        """
        if quantity.value < 0 and self.fiscal_stock is None:
            raise ValueError(
                f"Cannot decrease fiscal stock of product '{self.name}': "
                "fiscal stock is empty"
            )
        if self.fiscal_stock is None:
            self.fiscal_stock = quantity
        else:
            if self.fiscal_stock.unit != quantity.unit:
                raise ValueError(
                    f"Unit mismatch: {self.fiscal_stock.unit} vs {quantity.unit}"
                )
            new_value = self.fiscal_stock.value + quantity.value
            if new_value < 0:
                raise ValueError(
                    f"Cannot decrease fiscal stock of product '{self.name}' "
                    f"below zero: available {self.fiscal_stock.value}, "
                    f"requested change {quantity.value}"
                )
            self.fiscal_stock = Quantity(new_value, self.fiscal_stock.unit)

    def mark_as_fiscal(self) -> None:
        """
        Позначає товар як такий, що надходив з фіскальної накладної.
        """
        self.is_fiscal = True

    def change_price(self, new_price: Money) -> None:
        """
        Змінює ціну товару.

        Args:
            new_price: Нова ціна.
        """
        self.price = new_price

    def change_cost_price(self, new_cost_price: Money) -> None:
        """
        Змінює собівартість товару.

        Args:
            new_cost_price: Нова собівартість.
        """
        self.cost_price = new_cost_price

    def is_low_stock(self, threshold: Optional[Quantity] = None) -> bool:
        """
        Перевіряє, чи залишок нижче порогу.

        Args:
            threshold: Поріг мінімального залишку.

        Returns:
            True якщо залишок нижче порогу або відсутній.
        """
        if self.stock is None:
            return True
        if threshold is None:
            threshold = Quantity(Decimal("10"), self.stock.unit)
        return self.stock < threshold

    def apply_tax(self) -> Money:
        """
        Розраховує ціну з ПДВ.

        Returns:
            Ціна з ПДВ.
        """
        if self.price is None:
            return Money.zero()
        gross = self.tax_rate.calculate_gross(self.price.amount)
        return Money(gross, self.price.currency)

    def get_tax_amount(self) -> Money:
        """
        Розраховує суму ПДВ.

        Returns:
            Сума ПДВ.
        """
        if self.price is None:
            return Money.zero()
        tax = self.tax_rate.apply_to(self.price.amount)
        return Money(tax, self.price.currency)

    def change_barcode(self, barcode: Barcode) -> None:
        """Змінює штрих-код товару."""
        self.barcode = barcode

    def change_category(self, category_id: Optional[UUID]) -> None:
        """Змінює категорію товару."""
        self.category_id = category_id

    def change_supplier(self, supplier_id: Optional[UUID]) -> None:
        """Змінює постачальника товару."""
        self.supplier_id = supplier_id

    def deactivate(self) -> None:
        """Деактивує товар."""
        self.is_active = False

    def activate(self) -> None:
        """Активує товар."""
        self.is_active = True

    def __str__(self) -> str:
        return f"Product(id={self.id}, name='{self.name}', barcode={self.barcode})"

    def __repr__(self) -> str:
        return (
            f"Product(id={self.id}, name='{self.name}', "
            f"price={self.price}, stock={self.stock}, "
            f"is_fiscal={self.is_fiscal}, fiscal_stock={self.fiscal_stock})"
        )
