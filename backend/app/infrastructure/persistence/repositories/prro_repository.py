"""
Infrastructure Layer: PrroRepository — робота з ПРРО (зміни та офлайн-черга).

Надає асинхронний CRUD для:
  - PrroShift     — зміни ПРРО (відкриття/закриття, обіг, Z-звіти)
  - PrroQueueItem — офлайн-черга фіскальних документів, що не передані

Використання:
    repo = PrroRepository(session)
    shift = await repo.create_shift(PrroShift(shift_number=1, opened_at=now))
"""

from __future__ import annotations

from datetime import datetime
from typing import Optional
from uuid import UUID

from sqlalchemy import func, select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.models.prro import (
    PrroQueueItem,
    PrroQueueStatus,
    PrroShift,
    PrroShiftStatus,
)


class PrroRepository:
    """Репозиторій для змін ПРРО та офлайн-черги фіскальних документів."""

    def __init__(self, session: AsyncSession):
        self._session = session

    # ─────────────────────────────────────────────
    # PrroShift — зміни ПРРО
    # ─────────────────────────────────────────────

    async def create_shift(self, shift: PrroShift) -> PrroShift:
        """Створює нову зміну ПРРО."""
        self._session.add(shift)
        await self._session.flush()
        return shift

    async def get_shift(self, shift_id: UUID) -> Optional[PrroShift]:
        """Знаходить зміну за ID."""
        stmt = select(PrroShift).where(PrroShift.id == shift_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def get_shift_by_number(self, shift_number: int) -> Optional[PrroShift]:
        """Знаходить зміну за номером."""
        stmt = select(PrroShift).where(PrroShift.shift_number == shift_number)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def get_open_shift(self) -> Optional[PrroShift]:
        """Повертає поточну відкриту зміну (найсвіжішу), якщо вона є."""
        stmt = (
            select(PrroShift)
            .where(PrroShift.status == PrroShiftStatus.OPEN)
            .order_by(PrroShift.opened_at.desc())
            .limit(1)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def list_shifts(
        self,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[PrroShift], int]:
        """Список змін з пагінацією (від найновіших)."""
        count_stmt = select(func.count()).select_from(PrroShift)
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            select(PrroShift)
            .order_by(PrroShift.shift_number.desc())
            .offset(offset)
            .limit(size)
        )
        result = await self._session.execute(stmt)
        shifts = list(result.scalars().all())

        return shifts, total

    async def close_shift(
        self,
        shift_id: UUID,
        closed_at: datetime,
        closed_by: str,
        zreport_number: str,
        signer_serial: Optional[str] = None,
        signer_name: Optional[str] = None,
    ) -> Optional[PrroShift]:
        """Закриває зміну (Z-звіт) та фіксує дані підписанта."""
        shift = await self.get_shift(shift_id)
        if shift is None:
            return None

        shift.status = PrroShiftStatus.CLOSED
        shift.closed_at = closed_at
        shift.closed_by = closed_by
        shift.zreport_number = zreport_number
        if signer_serial is not None:
            shift.signer_serial = signer_serial
        if signer_name is not None:
            shift.signer_name = signer_name

        await self._session.flush()
        return shift

    async def increment_shift_counters(
        self,
        shift_id: UUID,
        amount: float,
        last_local_number: Optional[int] = None,
        last_mac: Optional[str] = None,
    ) -> Optional[PrroShift]:
        """Збільшує лічильник чеків та обіг зміни після фіскалізації."""
        shift = await self.get_shift(shift_id)
        if shift is None:
            return None

        shift.receipt_count += 1
        # Обіг зміни: коректне Decimal-додавання (уникнення Decimal+float)
        from decimal import Decimal as _Decimal
        shift.total_amount = _Decimal(str(shift.total_amount or 0)) + _Decimal(str(amount))
        if last_local_number is not None:
            shift.last_local_number = last_local_number
        if last_mac is not None:
            shift.last_mac = last_mac

        await self._session.flush()
        return shift

    async def next_local_number(self, shift_id: UUID) -> int:
        """M1: атомарний local_number — інкремент + збереження в одній SQL-операції.

        SQL UPDATE ... RETURNING гарантує N унікальних послідовних номерів
        при N паралельних фіскалізаціях (без read-then-write race).

        Raises:
            ValueError: якщо зміну не знайдено (або зміну закрито).
        """
        stmt = (
            update(PrroShift)
            .where(
                PrroShift.id == shift_id,
                PrroShift.status == PrroShiftStatus.OPEN,
            )
            .values(
                last_local_number=func.coalesce(PrroShift.last_local_number, 0) + 1
            )
            .returning(PrroShift.last_local_number)
        )
        result = await self._session.execute(stmt)
        value = result.scalar_one_or_none()
        if value is None:
            raise ValueError(f"Відкриту зміну {shift_id} не знайдено")
        return value

    async def update_shift_last_mac(
        self,
        shift_id: UUID,
        last_mac: str,
    ) -> Optional[PrroShift]:
        """Оновлює лише last_mac зміни (B1: hash-ланцюжок після sync-відправки)."""
        shift = await self.get_shift(shift_id)
        if shift is None:
            return None
        shift.last_mac = last_mac
        await self._session.flush()
        return shift

    async def update_shift(self, shift: PrroShift) -> PrroShift:
        """Оновлює існуючу зміну."""
        merged = await self._session.merge(shift)
        await self._session.flush()
        return merged

    async def delete_shift(self, shift_id: UUID) -> bool:
        """Видаляє зміну за ID. Повертає True, якщо зміну знайдено."""
        shift = await self.get_shift(shift_id)
        if shift is None:
            return False
        await self._session.delete(shift)
        await self._session.flush()
        return True

    # ─────────────────────────────────────────────
    # PrroQueueItem — офлайн-черга
    # ─────────────────────────────────────────────

    async def add_to_queue(self, item: PrroQueueItem) -> PrroQueueItem:
        """Додає фіскальний документ в офлайн-чергу."""
        self._session.add(item)
        await self._session.flush()
        return item

    async def get_queue_item(self, item_id: UUID) -> Optional[PrroQueueItem]:
        """Знаходить запис черги за ID."""
        stmt = select(PrroQueueItem).where(PrroQueueItem.id == item_id)
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def list_pending(self, limit: int = 100) -> list[PrroQueueItem]:
        """
        Повертає невідправлені документи (pending/failed) у порядку черги.

        Спочатку pending (за часом створення), потім failed — щоб повторна
        відправка не перебивала нові документи.
        """
        stmt = (
            select(PrroQueueItem)
            .where(
                PrroQueueItem.status.in_(
                    [PrroQueueStatus.PENDING, PrroQueueStatus.FAILED]
                )
            )
            .order_by(
                PrroQueueItem.status.asc(),
                PrroQueueItem.created_at.asc(),
            )
            .limit(limit)
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def list_by_shift(self, shift_id: UUID) -> list[PrroQueueItem]:
        """Документи черги за зміною (у порядку локальних номерів)."""
        stmt = (
            select(PrroQueueItem)
            .where(PrroQueueItem.shift_id == shift_id)
            .order_by(PrroQueueItem.local_number.asc())
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def list_by_receipt(self, receipt_id: UUID) -> list[PrroQueueItem]:
        """Документи черги, прив'язані до конкретного чеку."""
        stmt = (
            select(PrroQueueItem)
            .where(PrroQueueItem.receipt_id == receipt_id)
            .order_by(PrroQueueItem.created_at.asc())
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def mark_sent(
        self,
        item_id: UUID,
        sent_at: Optional[datetime] = None,
    ) -> Optional[PrroQueueItem]:
        """Позначає документ як успішно переданий у податкову."""
        item = await self.get_queue_item(item_id)
        if item is None:
            return None

        item.status = PrroQueueStatus.SENT
        item.sent_at = sent_at or datetime.utcnow()
        item.error = None

        await self._session.flush()
        return item

    async def mark_failed(self, item_id: UUID, error: str) -> Optional[PrroQueueItem]:
        """Позначає документ як помилку передачі (із текстом помилки)."""
        item = await self.get_queue_item(item_id)
        if item is None:
            return None

        item.status = PrroQueueStatus.FAILED
        item.error = error

        await self._session.flush()
        return item

    async def update_queue_check_sign(
        self, item_id: UUID, check_sign: str
    ) -> Optional[PrroQueueItem]:
        """B2: зберігає повний підписаний check_sign (ідемпотентність sync)."""
        stmt = (
            update(PrroQueueItem)
            .where(PrroQueueItem.id == item_id)
            .values(check_sign=check_sign)
            .returning(PrroQueueItem)
        )
        result = await self._session.execute(stmt)
        await self._session.flush()
        return result.scalar_one_or_none()

    async def count_pending(self) -> int:
        """Кількість документів, що очікують передачі (pending)."""
        stmt = (
            select(func.count())
            .select_from(PrroQueueItem)
            .where(PrroQueueItem.status == PrroQueueStatus.PENDING)
        )
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def delete_queue_item(self, item_id: UUID) -> bool:
        """Видаляє запис з черги. Повертає True, якщо запис знайдено."""
        item = await self.get_queue_item(item_id)
        if item is None:
            return False
        await self._session.delete(item)
        await self._session.flush()
        return True
