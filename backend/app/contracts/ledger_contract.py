"""
Контракт модуля Ledger (Взаєморозрахунки).

Визначає інтерфейс для роботи з журналом взаєморозрахунків
з постачальниками.
"""

from typing import Protocol, Optional, List, Tuple
from decimal import Decimal
from uuid import UUID
from datetime import datetime


class LedgerModuleInterface(Protocol):
    """
    Інтерфейс модуля взаєморозрахунків.
    
    Відповідає за:
    - Створення записів у журналі при операціях
    - Розрахунок поточного балансу постачальника
    - Історію операцій з пагінацією
    
    Модуль отримує запити через Event Bus (підписка на події
    invoice.confirmed, return.confirmed) або через прямий виклик
    для операцій оплати.
    """

    # ─── Події, які публікує ─────────────────────────────────────────────
    # publishes:
    #   - "ledger.entry_created"   — коли створено новий запис у журналі
    #
    # ─── Події, на які підписується ───────────────────────────────────────
    # subscribes:
    #   - "invoice.confirmed"      — для створення запису про накладну
    #   - "invoice.cancelled"      — для створення запису про скасування
    #   - "return.confirmed"       — для створення запису про повернення

    # ─── Створення запису ─────────────────────────────────────────────────

    async def create_ledger_entry(
        self,
        supplier_id: UUID,
        operation_type: str,
        amount: Decimal,
        operation_date: datetime,
        document_id: Optional[UUID] = None,
        document_number: Optional[str] = None,
        notes: Optional[str] = None,
    ) -> "SupplierLedger":
        """
        Створює новий запис у журналі взаєморозрахунків.
        
        Після створення публікує подію "ledger.entry_created".
        Автоматично розраховує баланс після операції.
        
        Args:
            supplier_id: ID постачальника.
            operation_type: Тип операції (invoice, payment, return, correction).
            amount: Сума операції (додатна — збільшення боргу, від'ємна — зменшення).
            operation_date: Дата операції.
            document_id: ID документа (опціонально).
            document_number: Номер документа (опціонально).
            notes: Нотатки (опціонально).
            
        Returns:
            Створений об'єкт SupplierLedger.
            
        Raises:
            SupplierNotFound: Якщо постачальника не знайдено.
            InvalidOperationType: Якщо тип операції недійсний.
        """
        ...

    # ─── Отримання балансу ───────────────────────────────────────────────

    async def get_supplier_balance(self, supplier_id: UUID) -> Decimal:
        """
        Отримує поточний баланс постачальника.
        
        Баланс розраховується як сума всіх операцій.
        Додатне значення — борг перед постачальником.
        
        Args:
            supplier_id: ID постачальника.
            
        Returns:
            Поточний баланс (Decimal).
        """
        ...

    async def get_supplier_balance_with_name(
        self,
        supplier_id: UUID,
    ) -> Tuple[Decimal, str, Optional[datetime]]:
        """
        Отримує баланс постачальника разом з назвою та датою останньої операції.
        
        Args:
            supplier_id: ID постачальника.
            
        Returns:
            Кортеж (баланс, назва постачальника, дата останньої операції).
        """
        ...

    async def get_all_supplier_balances(self) -> List[dict]:
        """
        Отримує баланси всіх постачальників.
        
        Returns:
            Список словників з інформацією про баланс кожного постачальника.
        """
        ...

    # ─── Історія операцій ────────────────────────────────────────────────

    async def get_ledger_history(
        self,
        supplier_id: UUID,
        page: int = 1,
        size: int = 20,
        operation_type: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
    ) -> Tuple[List["SupplierLedger"], int]:
        """
        Отримує історію операцій для постачальника з фільтрацією та пагінацією.
        
        Args:
            supplier_id: ID постачальника.
            page: Номер сторінки.
            size: Розмір сторінки.
            operation_type: Фільтр за типом операції (опціонально).
            date_from: Фільтр від дати (опціонально).
            date_to: Фільтр до дати (опціонально).
            
        Returns:
            Кортеж (список записів, загальна кількість).
        """
        ...

    # ─── Операції оплати ─────────────────────────────────────────────────

    async def register_payment(
        self,
        supplier_id: UUID,
        amount: Decimal,
        payment_date: datetime,
        payment_method: str,
        notes: Optional[str] = None,
    ) -> "SupplierLedger":
        """
        Реєструє оплату постачальнику.
        
        Створює запис з від'ємною сумою (зменшення боргу).
        
        Args:
            supplier_id: ID постачальника.
            amount: Сума оплати (додатна).
            payment_date: Дата оплати.
            payment_method: Спосіб оплати (cash, bank, card).
            notes: Нотатки (опціонально).
            
        Returns:
            Створений об'єкт SupplierLedger.
        """
        ...
