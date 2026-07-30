"""Unit tests: Supplier Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.supplier import Supplier


class TestSupplierRepository:

    @pytest.mark.asyncio
    async def test_save_supplier(self, supplier_repo, session):
        s = Supplier(id=uuid4(), name="Постачальник 1")
        created = await supplier_repo.save(s)
        await session.commit()
        assert created.name == "Постачальник 1"

    @pytest.mark.asyncio
    async def test_find_by_name(self, supplier_repo, session):
        s = Supplier(id=uuid4(), name="Унікальний")
        await supplier_repo.save(s)
        await session.commit()
        found = await supplier_repo.find_by_name("Унікальний")
        assert found is not None
