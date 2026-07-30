"""Фікстури для тестування Use Cases."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock
from uuid import uuid4

import pytest

from app.application.use_cases import (
    ProductUseCases,
    InvoiceUseCases,
    ReceiptUseCases,
    AuthUseCases,
)


@pytest.fixture
def mock_product_repo():
    return AsyncMock()


@pytest.fixture
def mock_invoice_repo():
    return AsyncMock()


@pytest.fixture
def mock_receipt_repo():
    return AsyncMock()


@pytest.fixture
def mock_user_repo():
    return AsyncMock()


@pytest.fixture
def mock_event_bus():
    bus = AsyncMock()
    bus.publish = AsyncMock()
    return bus


@pytest.fixture
def mock_unit_of_work():
    uow = AsyncMock()
    uow.commit = AsyncMock()
    uow.rollback = AsyncMock()
    return uow


@pytest.fixture
def product_use_cases(mock_product_repo, mock_event_bus, mock_unit_of_work):
    return ProductUseCases(
        product_repo=mock_product_repo,
        event_bus=mock_event_bus,
        unit_of_work=mock_unit_of_work,
    )


@pytest.fixture
def invoice_use_cases(mock_invoice_repo, mock_event_bus, mock_unit_of_work):
    return InvoiceUseCases(
        invoice_repo=mock_invoice_repo,
        event_bus=mock_event_bus,
        unit_of_work=mock_unit_of_work,
    )


@pytest.fixture
def receipt_use_cases(mock_receipt_repo, mock_event_bus, mock_unit_of_work):
    return ReceiptUseCases(
        receipt_repo=mock_receipt_repo,
        event_bus=mock_event_bus,
        unit_of_work=mock_unit_of_work,
    )


@pytest.fixture
def auth_use_cases(mock_user_repo, mock_event_bus, mock_unit_of_work):
    return AuthUseCases(
        user_repo=mock_user_repo,
        event_bus=mock_event_bus,
        unit_of_work=mock_unit_of_work,
    )
