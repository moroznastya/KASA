"""Dependencies для API v2 — отримання Use Cases та сервісів через DI."""

from __future__ import annotations

from typing import AsyncGenerator

from fastapi import Depends, Request

from app.application.use_cases import (
    ProductUseCases,
    InvoiceUseCases,
    ReceiptUseCases,
    AuthUseCases,
    LedgerUseCases,
)
from app.domain.repositories import ICategoryRepository
from app.domain.services.cache_service import ICacheService


async def get_product_use_cases(request: Request) -> ProductUseCases:
    """Отримати ProductUseCases з DI контейнера."""
    return request.app.state.di_container.resolve("product_use_cases")


async def get_invoice_use_cases(request: Request) -> InvoiceUseCases:
    """Отримати InvoiceUseCases з DI контейнера."""
    return request.app.state.di_container.resolve("invoice_use_cases")


async def get_receipt_use_cases(request: Request) -> ReceiptUseCases:
    """Отримати ReceiptUseCases з DI контейнера."""
    return request.app.state.di_container.resolve("receipt_use_cases")


async def get_auth_use_cases(request: Request) -> AuthUseCases:
    """Отримати AuthUseCases з DI контейнера."""
    return request.app.state.di_container.resolve("auth_use_cases")


async def get_ledger_use_cases(request: Request) -> LedgerUseCases:
    """Отримати LedgerUseCases з DI контейнера."""
    return request.app.state.di_container.resolve("ledger_use_cases")


async def get_category_repository(request: Request) -> ICategoryRepository:
    """Отримати CategoryRepository з DI контейнера."""
    return request.app.state.di_container.resolve("category_repository")


async def get_cache_service(request: Request) -> ICacheService:
    """Отримати ICacheService з DI контейнера.

    Повертає RedisCacheService, зареєстрований як singleton.
    Якщо Redis недоступний, повертає NullCacheService (без кешу).
    """
    return request.app.state.di_container.resolve("cache_service")
