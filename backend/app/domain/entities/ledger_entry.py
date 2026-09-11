"""
Domain Entity: LedgerEntry (Запис журналу взаєморозрахунків).

Чиста доменна сутність без залежності від SQLAlchemy.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
from enum import Enum
from typing import Optional
from uuid import UUID, uuid4

from ..value_objects.money import Money


class OperationType(Enum):
    """Тип операції в журналі взаєморозрахунків."""
    INVOICE = "invoice"              # Надходження товару (збільшення боргу)
    PAYMENT = "payment"              # Оплата постачальнику (зменшення боргу)
    RETURN = "return"                # Повернення товару (зменшення боргу)
    CORRECTION = "correction"        # Коригування
    WRITE_OFF = "write_off"          # Списання боргу


@dataclass
class LedgerEntry:
    """
    Запис у журналі взаєморозрахунків з постачальником.

    Відповідає за:
    - Фіксацію кожної фінансової операції
    - Розрахунок балансу після операції
    - Аудит фінансових рухів
    """

    id: UUID = field(default_factory=uuid4)
    supplier_id: UUID = field(default_factory=uuid4)
    amount: Money = field(default_factory=lambda: Money.zero())
    operation_type: OperationType = OperationType.INVOICE
    balance_after: Optional[Money] = None
    created_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    document_id: Optional[UUID] = None
    document_number: str = ""
    notes: str = ""

    @property
    def is_debit(self) -> bool:
        """Чи є операція дебетовою (збільшення боргу)."""
        return self.amount.is_positive()

    @property
    def is_credit(self) -> bool:
        """Чи є операція кредитовою (зменшення боргу)."""
        return not self.amount.is_positive() and not self.amount.is_zero()

    @property
    def is_zero_amount(self) -> bool:
        """Чи нульова сума операції."""
        return self.amount.is_zero()

    def __str__(self) -> str:
        return (
            f"LedgerEntry(id={self.id}, supplier_id={self.supplier_id}, "
            f"amount={self.amount}, type={self.operation_type.value})"
        )

    def __repr__(self) -> str:
        return (
            f"LedgerEntry(id={self.id}, supplier_id={self.supplier_id}, "
            f"amount={self.amount}, type={self.operation_type.value}, "
            f"balance_after={self.balance_after})"
        )
