"""Unit tests: валідація сум оплати чеку (mixed/cash/card) у ReceiptUseCases."""

from __future__ import annotations

from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.dto.receipt_dto import ReceiptCreateDTO, ReceiptItemDTO
from app.application.use_cases.receipt_use_cases import ReceiptUseCases
from app.domain.entities.product import Product
from app.domain.entities.receipt import Receipt
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity


def _build_use_cases() -> tuple[ReceiptUseCases, AsyncMock]:
    """ReceiptUseCases з моками (без фіскалізації). Повертає (uc, receipt_repo)."""
    receipt_repo = AsyncMock()
    product_repo = AsyncMock()
    uow = MagicMock()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        return False

    uow.__aenter__ = __aenter__
    uow.__aexit__ = __aexit__
    uow.commit = AsyncMock()

    product = Product(
        id=uuid4(),
        name="Товар",
        price=Money(Decimal("100")),
        stock=Quantity(Decimal("50")),
        unit="шт",
    )
    product_repo.find_by_id.return_value = product
    product_repo.update = AsyncMock()

    saved = Receipt(id=uuid4(), number="R-1", total=Money(Decimal("300")))
    receipt_repo.save = AsyncMock(return_value=saved)

    return (
        ReceiptUseCases(
            receipt_repo=receipt_repo,
            product_repo=product_repo,
            unit_of_work=uow,
            event_bus=AsyncMock(),
        ),
        receipt_repo,
    )


def _make_dto(
    payment_method: str = "cash",
    cash_amount=None,
    card_amount=None,
    customer_id=None,
) -> ReceiptCreateDTO:
    """DTO чеку: 3 × 100 грн = total 300 грн."""
    return ReceiptCreateDTO(
        items=[
            ReceiptItemDTO(
                product_id=uuid4(),
                name="Товар",
                quantity=Decimal("3"),
                price=Decimal("100"),
                tax_rate=20,
            )
        ],
        payment_method=payment_method,
        cash_amount=cash_amount,
        card_amount=card_amount,
        customer_id=customer_id,
        is_fiscal=True,
    )


class TestPaymentValidation:
    """Валідація сум оплати у create_sale_receipt."""

    async def test_mixed_requires_cash_and_card(self):
        """mixed без card_amount → ValueError."""
        use_cases, _ = _build_use_cases()
        with pytest.raises(ValueError, match="обов'язково вкажіть cash_amount і card_amount"):
            await use_cases.create_sale_receipt(
                _make_dto(payment_method="mixed", cash_amount=Decimal("100"))
            )

    async def test_mixed_sum_must_equal_total(self):
        """mixed з cash+card (200) < total (300) → ValueError."""
        use_cases, _ = _build_use_cases()
        with pytest.raises(ValueError, match="має дорівнювати сумі чеку"):
            await use_cases.create_sale_receipt(
                _make_dto(
                    payment_method="mixed",
                    cash_amount=Decimal("100"),
                    card_amount=Decimal("100"),
                )
            )

    async def test_mixed_valid(self):
        """Коректний mixed (cash+card == total) → чек створюється."""
        use_cases, _ = _build_use_cases()
        dto = await use_cases.create_sale_receipt(
            _make_dto(
                payment_method="mixed",
                cash_amount=Decimal("100"),
                card_amount=Decimal("200"),
            )
        )
        assert dto.id is not None

    async def test_cash_rejects_card_amount(self):
        """cash з card_amount > 0 → ValueError."""
        use_cases, _ = _build_use_cases()
        with pytest.raises(ValueError, match="card_amount має бути 0 або не вказаний"):
            await use_cases.create_sale_receipt(
                _make_dto(cash_amount=Decimal("300"), card_amount=Decimal("100"))
            )

    async def test_card_rejects_cash_amount(self):
        """card з cash_amount > 0 → ValueError."""
        use_cases, _ = _build_use_cases()
        with pytest.raises(ValueError, match="cash_amount має бути 0 або не вказаний"):
            await use_cases.create_sale_receipt(
                _make_dto(
                    payment_method="card",
                    cash_amount=Decimal("100"),
                    card_amount=Decimal("300"),
                )
            )

    async def test_cash_underpaid_rejected(self):
        """cash з cash_amount (100) < total (300) → ValueError."""
        use_cases, _ = _build_use_cases()
        with pytest.raises(ValueError, match="менша за суму чеку"):
            await use_cases.create_sale_receipt(_make_dto(cash_amount=Decimal("100")))

    async def test_cash_underpaid_allowed_for_debtor(self):
        """cash < total, але customer_id (борг) → дозволено."""
        use_cases, _ = _build_use_cases()
        dto = await use_cases.create_sale_receipt(
            _make_dto(cash_amount=Decimal("100"), customer_id=uuid4())
        )
        assert dto.id is not None

    async def test_cash_change_computed_and_saved(self):
        """cash з cash_amount (500) > total (300) → change_amount = 200."""
        use_cases, receipt_repo = _build_use_cases()
        dto = await use_cases.create_sale_receipt(_make_dto(cash_amount=Decimal("500")))
        assert dto.id is not None

        # Entity, передана у репозиторій, має розраховану здачу
        entity = receipt_repo.save.call_args.args[0]
        assert entity.change_amount is not None
        assert entity.change_amount.amount == Decimal("200")
