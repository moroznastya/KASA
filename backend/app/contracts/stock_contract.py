"""
Контракт модуля Stock (Склад).

Визначає інтерфейс для управління залишками товарів на складі.
Всі сервіси, які працюють зі складом, мають реалізовувати цей Protocol.
"""

from decimal import Decimal
from typing import Optional, Protocol
from uuid import UUID


class StockModuleInterface(Protocol):
    """
    Інтерфейс модуля складу.

    Відповідає за:
    - Оновлення залишків товарів
    - Перевірку достатності товару
    - Відстеження мінімальних рівнів
    - Резервування товарів (майбутнє)

    Модулі, які залежать від StockModule, використовують
    цей Protocol замість прямої залежності від ProductService.update_stock().
    """

    # ─── Події, які публікує ─────────────────────────────────────────────
    # publishes:
    #   - "stock.changed"     — коли змінено залишок товару
    #   - "stock.low"         — коли залишок нижче мінімального рівня
    #
    # ─── Події, на які підписується ───────────────────────────────────────
    # subscribes:
    #   - "product.created"           — для ініціалізації залишку
    #   - "invoice.confirmed"         — для збільшення залишків
    #   - "invoice.cancelled"         — для зменшення залишків (відкат)
    #   - "transfer.confirmed"        — для переміщення
    #   - "return.confirmed"          — для зменшення залишків
    #   - "receipt.created"           — для зменшення залишків при продажу
    #   - "receipt.cancelled"         — для збільшення залишків при поверненні

    # ─── Управління залишками ────────────────────────────────────────────

    async def change_stock(
        self,
        product_id: UUID,
        quantity_change: Decimal,
        reason: str,
        document_id: Optional[UUID] = None,
        document_type: Optional[str] = None,
    ) -> Decimal:
        """
        Змінює залишок товару на складі.

        Після зміни публікує подію "stock.changed".
        Якщо новий залишок нижче мінімуму — публікує "stock.low".

        Args:
            product_id: UUID товару.
            quantity_change: Зміна кількості (додатна — збільшення, від'ємна — зменшення).
            reason: Причина зміни (наприклад, "invoice_confirmed", "receipt_created").
            document_id: ID документа-причини (опціонально).
            document_type: Тип документа (опціонально).

        Returns:
            Новий залишок після зміни.

        Raises:
            InsufficientStock: Якщо недостатньо товару при зменшенні.
            ProductNotFound: Якщо товар не знайдено.
        """
        ...

    async def get_stock(self, product_id: UUID) -> Decimal:
        """
        Отримує поточний залишок товару.

        Args:
            product_id: UUID товару.

        Returns:
            Поточний залишок (Decimal).
        """
        ...

    async def get_stock_batch(self, product_ids: list[UUID]) -> dict:
        """
        Отримує залишки для декількох товарів одночасно.

        Args:
            product_ids: Список UUID товарів.

        Returns:
            Словник {product_id: stock}.
        """
        ...

    # ─── Перевірка достатності ───────────────────────────────────────────

    async def check_sufficient_stock(
        self,
        product_id: UUID,
        required_quantity: Decimal,
    ) -> bool:
        """
        Перевіряє, чи достатньо товару на складі.

        Args:
            product_id: UUID товару.
            required_quantity: Необхідна кількість.

        Returns:
            True — якщо достатньо, False — якщо недостатньо.
        """
        ...

    async def check_sufficient_stock_batch(
        self,
        items: list[tuple[UUID, Decimal]],
    ) -> tuple[bool, list[tuple[UUID, Decimal, Decimal]]]:
        """
        Перевіряє достатність для списку товарів.

        Args:
            items: Список кортежів (product_id, required_quantity).

        Returns:
            Кортеж (всі_достатньо, список_недостатніх).
            Кожен елемент списку недостатніх: (product_id, required, available).
        """
        ...

    # ─── Мінімальні рівні ────────────────────────────────────────────────

    async def set_min_stock(self, product_id: UUID, min_quantity: Decimal) -> None:
        """
        Встановлює мінімальний рівень залишку для товару.

        Args:
            product_id: UUID товару.
            min_quantity: Мінімальна кількість.
        """
        ...

    async def get_low_stock_products(self) -> list[dict]:
        """
        Отримує список товарів, залишок яких нижче мінімального рівня.

        Returns:
            Список словників з інформацією про товари з низьким залишком.
        """
        ...

    # ─── Резервування (майбутнє) ─────────────────────────────────────────

    async def reserve_stock(
        self,
        product_id: UUID,
        quantity: Decimal,
        reservation_id: str,
    ) -> None:
        """
        Резервує товар на складі (для чеків в процесі створення).

        Args:
            product_id: UUID товару.
            quantity: Кількість для резервування.
            reservation_id: ID резервації.
        """
        ...

    async def release_reservation(self, reservation_id: str) -> None:
        """
        Звільняє резервацію товару.

        Args:
            reservation_id: ID резервації.
        """
        ...
