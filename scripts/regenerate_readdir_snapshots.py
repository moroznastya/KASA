#!/usr/bin/env python3
"""Регенерація golden-снапшотів readdirs_snapshot з Python-еталона (v1 API).

Відтворює ТОЙ САМИЙ код, що backend/app/api/v1/{products,categories,suppliers}.py,
проти поточної БД, і записує JSON у crates/torgashka-infrastructure/tests/snapshots/.

Запуск:
    DATABASE_URL="postgresql://postgres:PASS@localhost:5432/pos_system" \
        python3 scripts/regenerate_readdir_snapshots.py
"""
import asyncio
import json
import os
import sys
from decimal import Decimal
from pathlib import Path
from uuid import UUID

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "backend"))

from sqlalchemy import func, select  # noqa: E402
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine  # noqa: E402

from app.api.v1.products import _get_product_with_relations  # noqa: E402
from app.infrastructure.persistence.models.category import Category  # noqa: E402
from app.infrastructure.persistence.models.product import Product  # noqa: E402
from app.infrastructure.persistence.models.supplier import Supplier  # noqa: E402
from app.infrastructure.persistence.models.supplier_ledger import SupplierLedger  # noqa: E402
from app.schemas.product import (  # noqa: E402
    ProductListResponse,
    ProductResponse,
    ProductSearchParams,
)
from app.schemas.category import CategoryResponse  # noqa: E402
from app.schemas.supplier import SupplierResponse  # noqa: E402
from app.domain.services.product_service import ProductService  # noqa: E402

SNAPSHOT_DIR = (
    Path(__file__).resolve().parent.parent
    / "frontend/src-tauri/crates/torgashka-infrastructure/tests/snapshots"
)


def dump_json(model) -> dict:
    """Як FastAPI серіалізує Pydantic-модель у JSON (Decimal → str)."""
    return json.loads(model.model_dump_json())


def pages_of(total: int, size: int) -> int:
    return max(1, (total + size - 1) // size) if total > 0 else 1


async def gen_products(session: AsyncSession, query: str | None, size: int) -> dict:
    """GET /products?page=1&size=N (з query або без) — як у снапшоті."""
    service = ProductService(session)
    params = ProductSearchParams(query=query, page=1, size=size)
    products, total = await service.search_products(params)
    items = []
    for p in products:
        full = await _get_product_with_relations(session, p.id)
        items.append(ProductResponse.model_validate(full or p))
    resp = ProductListResponse(
        items=items, total=total, page=1, page_size=size, pages=pages_of(total, size)
    )
    return dump_json(resp)


async def gen_categories(session: AsyncSession, size: int = 5) -> dict:
    """GET /categories?page=1&size=5 (ORDER BY name) — як у снапшоті."""
    total = (await session.execute(select(func.count(Category.id)))).scalar() or 0
    result = await session.execute(
        select(Category).order_by(Category.name).offset(0).limit(size)
    )
    categories = result.scalars().all()
    return {
        "items": [dump_json(CategoryResponse.model_validate(c)) for c in categories],
        "total": total,
        "page": 1,
        "page_size": size,
        "pages": pages_of(total, size),
    }


async def _supplier_to_response(session: AsyncSession, supplier: Supplier) -> SupplierResponse:
    """Копія з v1 suppliers.py: SupplierResponse + current_balance."""
    balance = (
        await session.execute(
            select(func.coalesce(func.sum(SupplierLedger.amount), 0)).where(
                SupplierLedger.supplier_id == supplier.id
            )
        )
    ).scalar() or 0
    response = SupplierResponse.model_validate(supplier)
    # Гроші — завжди 2 знаки після коми. Python-еталон з int-coalesce дає
    # Decimal("0") → "0"; Rust-реалізація дає "0.00". Нормалізуємо scale.
    response.current_balance = Decimal(str(balance)).quantize(Decimal("0.01"))
    return response


async def gen_suppliers(session: AsyncSession, size: int = 3) -> dict:
    """GET /suppliers?page=1&size=3 (ORDER BY name + balance) — як у снапшоті."""
    total = (await session.execute(select(func.count(Supplier.id)))).scalar() or 0
    result = await session.execute(
        select(Supplier).order_by(Supplier.name).offset(0).limit(size)
    )
    suppliers = result.scalars().all()
    return {
        "items": [
            dump_json(await _supplier_to_response(session, s)) for s in suppliers
        ],
        "total": total,
        "page": 1,
        "page_size": size,
        "pages": pages_of(total, size),
    }


async def main() -> None:
    url = os.environ.get("DATABASE_URL")
    if not url:
        sys.exit("DATABASE_URL не задано")
    engine = create_async_engine(url)
    async with engine.connect() as conn:
        session = AsyncSession(bind=conn)
        snapshots = {
            "products_default": await gen_products(session, None, 3),
            "products_query": await gen_products(session, "хліб", 5),
            "categories_default": await gen_categories(session, 5),
            "suppliers_default": await gen_suppliers(session, 3),
        }
    await engine.dispose()

    for name, data in snapshots.items():
        path = SNAPSHOT_DIR / f"{name}.json"
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
        print(f"OK {name}: {path} ({len(data.get('items', []))} items)")


if __name__ == "__main__":
    asyncio.run(main())
