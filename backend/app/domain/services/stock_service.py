"""
Domain Service: StockService.

Чиста бізнес-логіка для управління залишками товарів.
Не має залежності від SQLAlchemy або інфраструктурних компонентів.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Optional

from ..value_objects.quantity import Quantity


class StockService:
    """
    Сервіс управління залишками.

    Відповідає за:
    - Перевірку достатності товару на складі
    - Резервування товарів
    - Розрахунок рекомендованої кількості для замовлення
    - Перевірку мінімальних рівнів залишків
    """

    @staticmethod
    def check_sufficient(
        available: Quantity,
        required: Quantity,
    ) -> bool:
        """
        Перевіряє, чи достатньо товару.

        Args:
            available: Доступна кількість.
            required: Необхідна кількість.

        Returns:
            True якщо достатньо, False якщо недостатньо.

        Raises:
            ValueError: Якщо одиниці виміру не співпадають.
        """
        if available.unit != required.unit:
            raise ValueError(
                f"Unit mismatch: {available.unit} vs {required.unit}"
            )
        return available >= required

    @staticmethod
    def check_sufficient_batch(
        items: list[tuple[Quantity, Quantity]],
    ) -> tuple[bool, list[tuple[int, Quantity, Quantity]]]:
        """
        Перевіряє достатність для списку товарів.

        Args:
            items: Список кортежів (доступно, потрібно).

        Returns:
            Кортеж (всі_достатньо, список_недостатніх).
            Кожен елемент списку недостатніх: (індекс, доступно, потрібно).
        """
        insufficient: list[tuple[int, Quantity, Quantity]] = []
        all_sufficient = True

        for i, (available, required) in enumerate(items):
            if not StockService.check_sufficient(available, required):
                insufficient.append((i, available, required))
                all_sufficient = False

        return all_sufficient, insufficient

    @staticmethod
    def calculate_reservation(
        available: Quantity,
        requested: Quantity,
    ) -> Quantity:
        """
        Розраховує кількість для резервування.

        Якщо запитувана кількість перевищує доступну,
        резервується вся доступна кількість.

        Args:
            available: Доступна кількість.
            requested: Запитувана кількість.

        Returns:
            Кількість для резервування.

        Raises:
            ValueError: Якщо одиниці виміру не співпадають.
        """
        if available.unit != requested.unit:
            raise ValueError(
                f"Unit mismatch: {available.unit} vs {requested.unit}"
            )
        if requested <= available:
            return requested
        return available

    @staticmethod
    def calculate_recommended_order(
        current_stock: Quantity,
        min_stock: Quantity,
        max_stock: Optional[Quantity] = None,
        average_daily_sales: Optional[Quantity] = None,
        lead_time_days: int = 7,
    ) -> Quantity:
        """
        Розраховує рекомендовану кількість для замовлення.

        Використовує формулу:
        recommended = max(min_stock - current_stock, 0)
        Якщо є max_stock: recommended = min(recommended, max_stock - current_stock)
        Якщо є average_daily_sales: recommended = max(recommended, avg_sales * lead_time - current_stock)

        Args:
            current_stock: Поточний залишок.
            min_stock: Мінімальний залишок.
            max_stock: Максимальний залишок (опціонально).
            average_daily_sales: Середньоденні продажі (опціонально).
            lead_time_days: Час поставки в днях.

        Returns:
            Рекомендована кількість для замовлення.

        Raises:
            ValueError: Якщо одиниці виміру не співпадають.
        """
        unit = current_stock.unit

        if min_stock.unit != unit:
            raise ValueError(
                f"Unit mismatch: current={unit}, min_stock={min_stock.unit}"
            )

        # Базова рекомендація: поповнити до мінімального рівня
        recommended = Decimal("0")
        if current_stock < min_stock:
            recommended = min_stock.value - current_stock.value

        # Якщо є середньоденні продажі, враховуємо їх
        if average_daily_sales is not None:
            if average_daily_sales.unit != unit:
                raise ValueError(
                    f"Unit mismatch: current={unit}, avg_sales={average_daily_sales.unit}"
                )
            sales_based = (average_daily_sales.value * Decimal(str(lead_time_days))) - current_stock.value
            recommended = max(recommended, sales_based)

        # Якщо є максимальний рівень, обмежуємо
        if max_stock is not None:
            if max_stock.unit != unit:
                raise ValueError(
                    f"Unit mismatch: current={unit}, max_stock={max_stock.unit}"
                )
            max_possible = max_stock.value - current_stock.value
            recommended = min(recommended, max_possible)

        # Не може бути від'ємним
        recommended = max(recommended, Decimal("0"))

        return Quantity(recommended, unit)

    @staticmethod
    def is_low_stock(
        current_stock: Quantity,
        min_stock: Quantity,
    ) -> bool:
        """
        Перевіряє, чи залишок нижче мінімального рівня.

        Args:
            current_stock: Поточний залишок.
            min_stock: Мінімальний залишок.

        Returns:
            True якщо залишок нижче мінімального.

        Raises:
            ValueError: Якщо одиниці виміру не співпадають.
        """
        if current_stock.unit != min_stock.unit:
            raise ValueError(
                f"Unit mismatch: {current_stock.unit} vs {min_stock.unit}"
            )
        return current_stock < min_stock

    @staticmethod
    def calculate_stock_after_change(
        current_stock: Quantity,
        change: Quantity,
        allow_negative: bool = False,
    ) -> Quantity:
        """
        Розраховує залишок після зміни.

        Args:
            current_stock: Поточний залишок.
            change: Зміна (додатна — збільшення, від'ємна — зменшення).
            allow_negative: Чи дозволені від'ємні залишки.

        Returns:
            Новий залишок.

        Raises:
            ValueError: Якщо одиниці виміру не співпадають.
            ValueError: Якщо результат від'ємний і negative не дозволено.
        """
        if current_stock.unit != change.unit:
            raise ValueError(
                f"Unit mismatch: {current_stock.unit} vs {change.unit}"
            )

        new_value = current_stock.value + change.value

        if new_value < 0 and not allow_negative:
            raise ValueError(
                f"Stock cannot be negative: {new_value} {current_stock.unit}. "
                f"Current: {current_stock.value}, change: {change.value}"
            )

        return Quantity(new_value, current_stock.unit)
