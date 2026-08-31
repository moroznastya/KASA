"""Unit tests: M1 — атомарний local_number (N паралельних → N унікальних).
1:1 Rust `tests/prro_concurrency.rs`.
"""

from __future__ import annotations

import asyncio
from datetime import datetime
from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.models.prro import PrroShift
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository


@pytest.fixture
def repo(session: AsyncSession) -> PrroRepository:
    return PrroRepository(session)


@pytest.fixture
async def open_shift(session: AsyncSession):
    shift = PrroShift(
        id=uuid4(), shift_number=1, opened_at=datetime.utcnow(),
        status="open", receipt_count=0, total_amount=0,
        last_local_number=0, last_mac=None,
    )
    session.add(shift)
    await session.flush()
    return shift


class TestAtomicLocalNumberM1:
    async def test_next_local_number_sequential(self, repo, open_shift):
        n1 = await repo.next_local_number(open_shift.id)
        n2 = await repo.next_local_number(open_shift.id)
        n3 = await repo.next_local_number(open_shift.id)
        assert (n1, n2, n3) == (1, 2, 3), "послідовна нумерація"

    async def test_next_local_number_concurrent_unique_sequential(self, repo, open_shift):
        """Критерій M1: N паралельних фіскалізацій → N унікальних послідовних
        local_number (без дублікатів і без пропусків)."""
        n = 20
        numbers = await asyncio.gather(
            *(repo.next_local_number(open_shift.id) for _ in range(n))
        )
        assert len(numbers) == n
        assert len(set(numbers)) == n, "N унікальних номерів"
        assert sorted(numbers) == list(range(1, n + 1)), "послідовні без пропусків"

    async def test_next_local_number_rejects_closed_shift(self, repo, open_shift, session):
        from app.infrastructure.persistence.models.prro import PrroShiftStatus

        open_shift.status = PrroShiftStatus.CLOSED
        await session.flush()
        with pytest.raises(ValueError):
            await repo.next_local_number(open_shift.id)
