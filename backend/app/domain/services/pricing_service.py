"""
Domain Service: PricingService.

Чиста бізнес-логіка для розрахунку цін, ПДВ та загальних сум.
Не має залежності від SQLAlchemy або інфраструктурних компонентів.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Optional

from ..value_objects.money import Money
from ..value_objects.tax_rate import TaxRate


class PricingService:
    """
    Сервіс ціноутворення.

    Відповідає за:
    - Розрахунок ПДВ
    - Розрахунок загальної суми з ПДВ
    - Розрахунок загальної суми без ПДВ
    - Розрахунок націнки
    - Розрахунок знижки
    """

    @staticmethod
    def calculate_vat(net_amount: Money, tax_rate: TaxRate) -> Money:
        """
        Розраховує суму ПДВ.

        Args:
            net_amount: Сума без ПДВ.
            tax_rate: Ставка ПДВ.

        Returns:
            Сума ПДВ.
        """
        tax = tax_rate.apply_to(net_amount.amount)
        return Money(tax, net_amount.currency)

    @staticmethod
    def calculate_gross(net_amount: Money, tax_rate: TaxRate) -> Money:
        """
        Розраховує суму з ПДВ.

        Args:
            net_amount: Сума без ПДВ.
            tax_rate: Ставка ПДВ.

        Returns:
            Сума з ПДВ.
        """
        gross = tax_rate.calculate_gross(net_amount.amount)
        return Money(gross, net_amount.currency)

    @staticmethod
    def calculate_net(gross_amount: Money, tax_rate: TaxRate) -> Money:
        """
        Розраховує суму без ПДВ із суми з ПДВ.

        Args:
            gross_amount: Сума з ПДВ.
            tax_rate: Ставка ПДВ.

        Returns:
            Сума без ПДВ.
        """
        net = tax_rate.extract_net(gross_amount.amount)
        return Money(net, gross_amount.currency)

    @staticmethod
    def calculate_total(items: list[tuple[Money, Decimal]]) -> Money:
        """
        Розраховує загальну суму списку позицій.

        Args:
            items: Список кортежів (ціна, кількість).

        Returns:
            Загальна сума.
        """
        if not items:
            return Money.zero()

        total = Decimal("0.00")
        currency = items[0][0].currency

        for price, quantity in items:
            if price.currency != currency:
                raise ValueError(
                    f"Currency mismatch: {price.currency} vs {currency}"
                )
            total += price.amount * quantity

        return Money(total, currency)

    @staticmethod
    def calculate_total_with_vat(
        items: list[tuple[Money, Decimal, TaxRate]],
    ) -> Money:
        """
        Розраховує загальну суму з ПДВ для списку позицій.

        Args:
            items: Список кортежів (ціна, кількість, ставка ПДВ).

        Returns:
            Загальна сума з ПДВ.
        """
        if not items:
            return Money.zero()

        total = Decimal("0.00")
        currency = items[0][0].currency

        for price, quantity, tax_rate in items:
            if price.currency != currency:
                raise ValueError(
                    f"Currency mismatch: {price.currency} vs {currency}"
                )
            gross = tax_rate.calculate_gross(price.amount * quantity)
            total += gross

        return Money(total, currency)

    @staticmethod
    def calculate_total_vat(
        items: list[tuple[Money, Decimal, TaxRate]],
    ) -> Money:
        """
        Розраховує загальну суму ПДВ для списку позицій.

        Args:
            items: Список кортежів (ціна, кількість, ставка ПДВ).

        Returns:
            Загальна сума ПДВ.
        """
        if not items:
            return Money.zero()

        total_vat = Decimal("0.00")
        currency = items[0][0].currency

        for price, quantity, tax_rate in items:
            if price.currency != currency:
                raise ValueError(
                    f"Currency mismatch: {price.currency} vs {currency}"
                )
            line_total = price.amount * quantity
            vat = tax_rate.apply_to(line_total)
            total_vat += vat

        return Money(total_vat, currency)

    @staticmethod
    def calculate_markup(
        cost_price: Money,
        selling_price: Money,
    ) -> Decimal:
        """
        Розраховує націнку у відсотках.

        Args:
            cost_price: Собівартість.
            selling_price: Ціна продажу.

        Returns:
            Націнка у відсотках (Decimal, наприклад 0.25 = 25%).

        Raises:
            ValueError: Якщо собівартість нульова або валюти не співпадають.
        """
        if cost_price.currency != selling_price.currency:
            raise ValueError("Currency mismatch between cost and selling price")
        if cost_price.is_zero():
            raise ValueError("Cannot calculate markup with zero cost price")
        if cost_price > selling_price:
            raise ValueError("Selling price cannot be less than cost price")

        markup = (selling_price.amount - cost_price.amount) / cost_price.amount
        return markup.quantize(Decimal("0.0001"))

    @staticmethod
    def calculate_discount(
        original_price: Money,
        discount_percent: Decimal,
    ) -> Money:
        """
        Розраховує ціну зі знижкою.

        Args:
            original_price: Початкова ціна.
            discount_percent: Відсоток знижки (наприклад, 0.10 = 10%).

        Returns:
            Ціна зі знижкою.

        Raises:
            ValueError: Якщо відсоток знижки не в діапазоні [0, 1].
        """
        if not Decimal("0") <= discount_percent <= Decimal("1"):
            raise ValueError(
                f"Discount percent must be between 0 and 1, got {discount_percent}"
            )
        discount_amount = original_price.amount * discount_percent
        discounted = original_price.amount - discount_amount
        if discounted < 0:
            raise ValueError("Discounted price cannot be negative")
        return Money(discounted, original_price.currency)

    @staticmethod
    def split_by_tax_rate(
        items: list[tuple[Money, Decimal, TaxRate]],
    ) -> dict[int, Money]:
        """
        Групує суми за ставками ПДВ.

        Args:
            items: Список кортежів (ціна, кількість, ставка ПДВ).

        Returns:
            Словник {відсоток_пдв: загальна_сума}.
        """
        result: dict[int, Decimal] = {}

        for price, quantity, tax_rate in items:
            line_total = price.amount * quantity
            percent = tax_rate.percent
            result[percent] = result.get(percent, Decimal("0.00")) + line_total

        currency = items[0][0].currency if items else "UAH"
        return {
            percent: Money(amount, currency)
            for percent, amount in result.items()
        }
