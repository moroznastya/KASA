"""Unit tests: Supplier Repository."""

from __future__ import annotations

from uuid import uuid4

import pytest

from app.infrastructure.persistence.models.supplier import Supplier


class TestSupplierRepository:

    @pytest.mark.asyncio
    async def test_save_supplier(self, supplier_repo, session):
        """Створення нового постачальника."""
        s = Supplier(
            id=uuid4(),
            name="Постачальник 1",
            edrpou="12345678",
        )
        created = await supplier_repo.save(s)
        await session.commit()
        assert created.name == "Постачальник 1"
        assert created.edrpou == "12345678"

    @pytest.mark.asyncio
    async def test_find_by_id(self, supplier_repo, session):
        """Пошук постачальника за ID."""
        s = Supplier(
            id=uuid4(),
            name="Унікальний",
            edrpou="UNQ12345",
        )
        await supplier_repo.save(s)
        await session.commit()
        found = await supplier_repo.find_by_id(s.id)
        assert found is not None
        assert found.name == "Унікальний"

    @pytest.mark.asyncio
    async def test_find_by_name(self, supplier_repo, session):
        """Пошук постачальника за назвою."""
        s = Supplier(
            id=uuid4(),
            name="ТОВ Ромашка",
            edrpou="87654321",
        )
        await supplier_repo.save(s)
        await session.commit()
        found = await supplier_repo.find_by_name("ТОВ Ромашка")
        assert found is not None
        assert found.id == s.id

    @pytest.mark.asyncio
    async def test_delete_supplier(self, supplier_repo, session):
        """Видалення постачальника."""
        s = Supplier(
            id=uuid4(),
            name="Видалити",
            edrpou="00000000",
        )
        await supplier_repo.save(s)
        await session.commit()

        await supplier_repo.delete(s.id)
        await session.commit()

        found = await supplier_repo.find_by_id(s.id)
        assert found is None
