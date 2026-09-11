"""Unit tests: terminal_* поля карткового терміналу (ПриватБанк).

Покриває:
  - мапінг terminal_* полів: create_dto_to_entity → entity;
  - entity_to_dto → DTO (відповідь API);
  - _to_orm → ORM (збереження у БД);
  - create_sale_receipt з terminal_* полями (card) → збереження;
  - продаж card/mixed без rrn → чек створюється (термінал може не відповісти);
  - terminal_status="declined" → ReceiptValidationError (HTTP 422);
  - повернення card/mixed без rrn → ReceiptValidationError (HTTP 422);
  - повернення cash без rrn → чек створюється.
"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta, timezone
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.dto.receipt_dto import ReceiptCreateDTO, ReceiptItemDTO
from app.application.mappers.receipt_mapper import ReceiptMapper
from app.application.use_cases.receipt_use_cases import (
    ReceiptUseCases,
    ReceiptValidationError,
)
from app.domain.entities.product import Product
from app.domain.entities.receipt import Receipt
from app.domain.value_objects.money import Money
from app.domain.value_objects.quantity import Quantity
from app.infrastructure.persistence.repositories.receipt_repository import (
    SQLAlchemyReceiptRepository,
)

TERMINAL_DT = datetime(2026, 8, 5, 12, 0, 0)


def _terminal_kwargs(**overrides):
    """Базовий набір terminal_* полів (успішна транзакція)."""
    kwargs = {
        "terminal_rrn": "123456789012",
        "terminal_approval_code": "ABC123",
        "terminal_invoice_number": "000123456",
        "terminal_transaction_id": "tx-987654",
        "terminal_response_code": "0000",
        "terminal_status": "approved",
        "terminal_receipt": "ПРИВАТБАНК\nТЕРМІНАЛ: 1234",
        "terminal_card_pan": "5577****1234",
        "terminal_payment_system": "MasterCard",
        "terminal_merchant": "MERCH-1",
        "terminal_created_at": TERMINAL_DT,
    }
    kwargs.update(overrides)
    return kwargs


def _dto(
    payment_method="cash",
    cash_amount=None,
    card_amount=None,
    customer_id=None,
    include_terminal=True,
    **terminal,
):
    """DTO чеку: 2 × 100 грн = total 200 грн."""
    return ReceiptCreateDTO(
        items=[
            ReceiptItemDTO(
                product_id=uuid4(),
                name="Товар",
                quantity=Decimal("2"),
                price=Decimal("100"),
                tax_rate=20,
            )
        ],
        payment_method=payment_method,
        cash_amount=cash_amount,
        card_amount=card_amount,
        customer_id=customer_id,
        is_fiscal=True,
        **(_terminal_kwargs(**terminal) if include_terminal else {}),
    )


def _build_use_cases() -> tuple[ReceiptUseCases, AsyncMock]:
    """ReceiptUseCases з моками (без фіскалізації). Повертає (uc, repo)."""
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

    saved = Receipt(id=uuid4(), number="R-1", total=Money(Decimal("200")))
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


class TestTerminalFieldsMapping:
    """Мапінг terminal_* полів між DTO / entity / ORM."""

    def test_create_dto_to_entity_maps_terminal_fields(self):
        """create_dto_to_entity → entity містить усі terminal_* поля."""
        dto = _dto(payment_method="card", card_amount=Decimal("200"))
        entity = ReceiptMapper.create_dto_to_entity(dto)

        assert entity.terminal_rrn == "123456789012"
        assert entity.terminal_approval_code == "ABC123"
        assert entity.terminal_invoice_number == "000123456"
        assert entity.terminal_transaction_id == "tx-987654"
        assert entity.terminal_response_code == "0000"
        assert entity.terminal_status == "approved"
        assert entity.terminal_receipt == "ПРИВАТБАНК\nТЕРМІНАЛ: 1234"
        assert entity.terminal_card_pan == "5577****1234"
        assert entity.terminal_payment_system == "MasterCard"
        assert entity.terminal_merchant == "MERCH-1"
        assert entity.terminal_created_at == TERMINAL_DT

    def test_entity_to_dto_maps_terminal_fields(self):
        """entity_to_dto → DTO (відповідь API) містить terminal_* поля."""
        dto = _dto(payment_method="card", card_amount=Decimal("200"))
        entity = ReceiptMapper.create_dto_to_entity(dto)
        out = ReceiptMapper.entity_to_dto(entity)

        assert out.terminal_rrn == "123456789012"
        assert out.terminal_response_code == "0000"
        assert out.terminal_status == "approved"
        assert out.terminal_card_pan == "5577****1234"
        assert out.terminal_created_at == TERMINAL_DT

    def test_to_orm_persists_terminal_fields(self):
        """_to_orm → ORM Receipt зберігає terminal_* поля."""
        dto = _dto(
            payment_method="mixed",
            cash_amount=Decimal("100"),
            card_amount=Decimal("100"),
        )
        entity = ReceiptMapper.create_dto_to_entity(dto)
        persisted = SQLAlchemyReceiptRepository._to_orm(entity)

        assert persisted.terminal_rrn == "123456789012"
        assert persisted.terminal_status == "approved"
        assert persisted.terminal_card_pan == "5577****1234"
        assert persisted.terminal_created_at == TERMINAL_DT

    def test_defaults_are_none(self):
        """Без terminal_* даних — усі поля None (не ламаємо cash-чеки)."""
        dto = _dto(cash_amount=Decimal("200"), include_terminal=False)
        entity = ReceiptMapper.create_dto_to_entity(dto)

        assert entity.terminal_rrn is None
        assert entity.terminal_status is None
        assert entity.terminal_created_at is None


class TestTerminalFieldsInUseCases:
    """Збереження та валідація terminal_* у create_sale/create_return."""

    async def test_sale_card_saves_terminal_fields(self):
        """Продаж card з terminal_* → entity у repo.save містить поля."""
        use_cases, receipt_repo = _build_use_cases()
        dto = _dto(payment_method="card", card_amount=Decimal("200"))

        await use_cases.create_sale_receipt(dto)

        entity = receipt_repo.save.call_args.args[0]
        assert entity.terminal_rrn == "123456789012"
        assert entity.terminal_status == "approved"
        assert entity.terminal_response_code == "0000"
        assert entity.terminal_created_at == TERMINAL_DT

    async def test_sale_card_without_rrn_ok(self):
        """Продаж card без rrn (термінал не відповів) → чек створюється."""
        use_cases, receipt_repo = _build_use_cases()
        dto = _dto(
            payment_method="card",
            card_amount=Decimal("200"),
            terminal_rrn=None,
        )

        result = await use_cases.create_sale_receipt(dto)

        assert result.id is not None
        entity = receipt_repo.save.call_args.args[0]
        assert entity.terminal_rrn is None

    async def test_sale_declined_raises_422(self):
        """terminal_status='declined' (card) → ReceiptValidationError."""
        use_cases, _ = _build_use_cases()
        dto = _dto(
            payment_method="card",
            card_amount=Decimal("200"),
            terminal_status="declined",
        )

        with pytest.raises(
            ReceiptValidationError,
            match="Оплата карткою не підтверджена терміналом",
        ):
            await use_cases.create_sale_receipt(dto)

    async def test_sale_mixed_declined_raises_422(self):
        """terminal_status='declined' (mixed) → теж ReceiptValidationError."""
        use_cases, _ = _build_use_cases()
        dto = _dto(
            payment_method="mixed",
            cash_amount=Decimal("100"),
            card_amount=Decimal("100"),
            terminal_status="declined",
        )

        with pytest.raises(ReceiptValidationError):
            await use_cases.create_sale_receipt(dto)

    async def test_sale_cash_ignores_declined_status(self):
        """cash з terminal_status='declined' → чек створюється."""
        use_cases, _ = _build_use_cases()
        dto = _dto(cash_amount=Decimal("200"), terminal_status="declined")

        result = await use_cases.create_sale_receipt(dto)

        assert result.id is not None

    async def test_return_card_requires_rrn(self):
        """Повернення card без rrn → ReceiptValidationError (422)."""
        use_cases, _ = _build_use_cases()
        dto = _dto(
            payment_method="card",
            card_amount=Decimal("200"),
            terminal_rrn=None,
        )

        with pytest.raises(
            ReceiptValidationError,
            match="RRN оригінальної транзакції",
        ):
            await use_cases.create_return_receipt(dto)

    async def test_return_mixed_requires_rrn(self):
        """Повернення mixed без rrn → ReceiptValidationError (422)."""
        use_cases, _ = _build_use_cases()
        dto = _dto(
            payment_method="mixed",
            cash_amount=Decimal("100"),
            card_amount=Decimal("100"),
            terminal_rrn=None,
        )

        with pytest.raises(ReceiptValidationError, match="RRN оригінальної транзакції"):
            await use_cases.create_return_receipt(dto)

    async def test_return_cash_without_rrn_ok(self):
        """Повернення cash без rrn → чек створюється (rrn не вимагається)."""
        use_cases, _ = _build_use_cases()
        dto = _dto(cash_amount=Decimal("200"))

        result = await use_cases.create_return_receipt(dto)

        assert result.id is not None

    async def test_return_card_with_rrn_saves_fields(self):
        """Повернення card з rrn → entity у repo.save містить terminal_*."""
        use_cases, receipt_repo = _build_use_cases()
        dto = _dto(payment_method="card", card_amount=Decimal("200"))

        await use_cases.create_return_receipt(dto)

        entity = receipt_repo.save.call_args.args[0]
        assert entity.terminal_rrn == "123456789012"
        assert entity.terminal_status == "approved"



class TestTerminalCreatedAtNormalization:
    """terminal_created_at: ISO з Z (aware) → naive UTC (без DBAPIError 500).

    Фронтенд шле date.toISOString() → "2026-08-05T17:00:00.000Z" (aware).
    ORM-колонка DateTime (TIMESTAMP WITHOUT TIME ZONE), asyncpg не приймає
    aware datetime → раніше був 500 DBAPIError.
    """

    def test_create_request_iso_z_parses_naive(self):
        """CreateReceiptRequest: '2026-08-05T17:00:00.000Z' → naive datetime."""
        from app.api.v2.receipts import CreateReceiptRequest

        req = CreateReceiptRequest(
            items=[{"product_id": uuid4(), "quantity": 1, "price": 200}],
            payment_method="card",
            card_amount=200.0,
            terminal_created_at="2026-08-05T17:00:00.000Z",
        )
        assert req.terminal_created_at == datetime(2026, 8, 5, 17, 0, 0)
        assert req.terminal_created_at.tzinfo is None

    def test_create_request_offset_parses_naive_utc(self):
        """CreateReceiptRequest: '+03:00' → конвертується у naive UTC."""
        from app.api.v2.receipts import CreateReceiptRequest

        req = CreateReceiptRequest(
            items=[{"product_id": uuid4(), "quantity": 1, "price": 200}],
            payment_method="card",
            card_amount=200.0,
            terminal_created_at="2026-08-05T20:00:00+03:00",
        )
        assert req.terminal_created_at == datetime(2026, 8, 5, 17, 0, 0)
        assert req.terminal_created_at.tzinfo is None

    def test_create_request_none_ok(self):
        """CreateReceiptRequest без terminal_created_at → None (cash-чек)."""
        from app.api.v2.receipts import CreateReceiptRequest

        req = CreateReceiptRequest(
            items=[{"product_id": uuid4(), "quantity": 1, "price": 200}],
            payment_method="cash",
        )
        assert req.terminal_created_at is None

    def test_dto_post_init_converts_aware_to_naive(self):
        """DTO з aware datetime (UTC) → naive UTC через __post_init__."""
        aware = datetime(2026, 8, 5, 17, 0, 0, tzinfo=UTC)
        dto = _dto(cash_amount=Decimal("200"), terminal_created_at=aware)

        assert dto.terminal_created_at == datetime(2026, 8, 5, 17, 0, 0)
        assert dto.terminal_created_at.tzinfo is None

    def test_dto_post_init_converts_offset_to_naive_utc(self):
        """DTO з aware datetime (+03:00) → naive UTC (17:00, не 20:00)."""
        aware = datetime(2026, 8, 5, 20, 0, 0, tzinfo=timezone(timedelta(hours=3)))
        dto = _dto(cash_amount=Decimal("200"), terminal_created_at=aware)

        assert dto.terminal_created_at == datetime(2026, 8, 5, 17, 0, 0)
        assert dto.terminal_created_at.tzinfo is None

    def test_dto_post_init_keeps_naive(self):
        """Naive datetime не змінюється."""
        naive = datetime(2026, 8, 5, 17, 0, 0)
        dto = _dto(cash_amount=Decimal("200"), terminal_created_at=naive)

        assert dto.terminal_created_at == naive
        assert dto.terminal_created_at.tzinfo is None

    def test_dto_post_init_none_ok(self):
        """terminal_created_at=None → залишається None."""
        dto = _dto(cash_amount=Decimal("200"), include_terminal=False)

        assert dto.terminal_created_at is None

    async def test_sale_with_iso_z_saves_naive(self):
        """create_sale_receipt з aware datetime → entity та ORM naive (201)."""
        use_cases, receipt_repo = _build_use_cases()
        dto = _dto(
            payment_method="card",
            card_amount=Decimal("200"),
            terminal_created_at=datetime(
                2026, 8, 5, 17, 0, 0, tzinfo=UTC
            ),
        )

        result = await use_cases.create_sale_receipt(dto)

        assert result.id is not None
        entity = receipt_repo.save.call_args.args[0]
        assert entity.terminal_created_at == datetime(2026, 8, 5, 17, 0, 0)
        assert entity.terminal_created_at.tzinfo is None
        persisted = SQLAlchemyReceiptRepository._to_orm(entity)
        assert persisted.terminal_created_at == datetime(2026, 8, 5, 17, 0, 0)
        assert persisted.terminal_created_at.tzinfo is None

    async def test_sale_with_none_terminal_created_at_ok(self):
        """terminal_created_at=None → чек створюється, значення None."""
        use_cases, receipt_repo = _build_use_cases()
        dto = _dto(
            payment_method="card",
            card_amount=Decimal("200"),
            terminal_created_at=None,
        )

        result = await use_cases.create_sale_receipt(dto)

        assert result.id is not None
        entity = receipt_repo.save.call_args.args[0]
        assert entity.terminal_created_at is None
