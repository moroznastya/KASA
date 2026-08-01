"""
Черга офлайн-документів ПРРО.

При роботі в офлайн-режимі фіскальні документи (чеки, Z-звіти) не можуть
бути передані на сервер ДПС одразу. PrroOfflineQueue зберігає їх у
таблиці prro_queue_items (через PrroRepository) та надає методи для
повторної передачі після відновлення зв'язку.

Ліміт офлайн-режиму: 168 годин (7 діб) — PRRO_OFFLINE_LIMIT_HOURS.
Після перевищення ліміту документи вважаються простроченими, і їх
подальша передача неможлива (вимагається втручання адміністратора).

Використання:
    queue = PrroOfflineQueue(repository)
    await queue.add_document(
        receipt_id=receipt.id,
        shift_id=shift.id,
        local_number=5,
        check_type="CHK",
        xml_body=dat_xml,
        mac=mac_value,
    )
    pending = await queue.get_pending()
    for item in pending:
        try:
            await client.send_chk(...)
            await queue.mark_sent(item.id)
        except Exception as exc:
            await queue.mark_failed(item.id, str(exc))
"""

from __future__ import annotations

import logging
from datetime import datetime, timedelta
from typing import Optional
from uuid import UUID

from app.infrastructure.persistence.models.prro import (
    PrroQueueItem,
    PrroQueueStatus,
)
from app.infrastructure.persistence.repositories.prro_repository import (
    PrroRepository,
)

logger = logging.getLogger(__name__)

# Максимальна тривалість офлайн-режиму: 168 годин (7 діб)
PRRO_OFFLINE_LIMIT_HOURS = 168

# Типи фіскальних документів
CHECK_TYPE_CHK = "CHK"            # Чек продажу/повернення
CHECK_TYPE_ZREPORT = "ZREPORT"    # Z-звіт
CHECK_TYPE_SERVICECHK = "SERVICECHK"  # Службовий чек


class PrroOfflineQueue:
    """
    Черга офлайн-документів ПРРО на базі PrroRepository.

    Args:
        repository: репозиторій ПРРО (PrroRepository) для роботи з БД.
    """

    def __init__(self, repository: PrroRepository) -> None:
        self._repository = repository

    # ─── Додавання ─────────────────────────────────────────────────────────

    async def add_document(
        self,
        *,
        receipt_id: Optional[UUID],
        shift_id: Optional[UUID],
        local_number: int,
        check_type: str,
        xml_body: str,
        mac: str | None = None,
    ) -> PrroQueueItem:
        """
        Додає фіскальний документ в офлайн-чергу.

        Args:
            receipt_id: ID чеку (може бути None для Z-звіту/службового).
            shift_id: ID зміни ПРРО.
            local_number: локальний номер документа в межах зміни.
            check_type: тип документа (CHK / ZREPORT / SERVICECHK).
            xml_body: канонічний XML <DAT> (або підписаний check_sign).
            mac: значення MAC (хеш-ланцюжок), якщо обчислено.

        Returns:
            PrroQueueItem — створений запис черги.

        Raises:
            ValueError: якщо локальний номер від'ємний або xml_body порожній.
        """
        if local_number < 0:
            raise ValueError(f"Локальний номер не може бути від'ємним: {local_number}")
        if not xml_body or not xml_body.strip():
            raise ValueError("xml_body не може бути порожнім")

        item = PrroQueueItem(
            receipt_id=receipt_id,
            shift_id=shift_id,
            local_number=local_number,
            check_type=check_type,
            xml_body=xml_body,
            mac=mac,
            status=PrroQueueStatus.PENDING,
        )
        saved = await self._repository.add_to_queue(item)
        logger.info(
            "PRRO_QUEUE | додано документ %s #%d (shift=%s)",
            check_type, local_number, shift_id,
        )
        return saved

    # ─── Читання ───────────────────────────────────────────────────────────

    async def get_pending(self, limit: int = 100) -> list[PrroQueueItem]:
        """
        Повертає документи, що очікують передачі (pending/failed).

        Args:
            limit: максимальна кількість документів.

        Returns:
            list[PrroQueueItem] — у порядку черги (спочатку pending).
        """
        return await self._repository.list_pending(limit=limit)

    async def count_pending(self) -> int:
        """Кількість документів, що очікують передачі."""
        return await self._repository.count_pending()

    async def list_by_shift(self, shift_id: UUID) -> list[PrroQueueItem]:
        """Документи черги за зміною (у порядку локальних номерів)."""
        return await self._repository.list_by_shift(shift_id)

    # ─── Оновлення статусу ─────────────────────────────────────────────────

    async def mark_sent(
        self,
        item_id: UUID,
        sent_at: datetime | None = None,
    ) -> Optional[PrroQueueItem]:
        """
        Позначає документ як успішно переданий у податкову.

        Args:
            item_id: ID запису черги.
            sent_at: дата/час передачі (None — поточний час UTC).

        Returns:
            PrroQueueItem | None — оновлений запис або None, якщо не знайдено.
        """
        return await self._repository.mark_sent(item_id, sent_at=sent_at)

    async def mark_failed(self, item_id: UUID, error: str) -> Optional[PrroQueueItem]:
        """
        Позначає документ як помилку передачі (із текстом помилки).

        Args:
            item_id: ID запису черги.
            error: текст помилки (не містить паролів/секретів!).

        Returns:
            PrroQueueItem | None — оновлений запис або None, якщо не знайдено.
        """
        logger.warning("PRRO_QUEUE | документ %s позначено failed: %s", item_id, error)
        return await self._repository.mark_failed(item_id, error)

    # ─── Ліміт офлайн-режиму ───────────────────────────────────────────────

    @staticmethod
    def is_expired(created_at: datetime, now: datetime | None = None) -> bool:
        """
        Перевіряє, чи вичерпано ліміт офлайн-передачі для документа.

        Args:
            created_at: дата/час створення документа.
            now: поточний час (None — datetime.utcnow()).

        Returns:
            bool — True, якщо документ старіший за 168 годин.
        """
        now = now or datetime.utcnow()
        return (now - created_at) > timedelta(hours=PRRO_OFFLINE_LIMIT_HOURS)

    async def get_expired(self, limit: int = 100) -> list[PrroQueueItem]:
        """
        Повертає прострочені документи (старші за ліміт офлайн-режиму).

        Args:
            limit: максимальна кількість документів.

        Returns:
            list[PrroQueueItem] — документи, що потребують уваги адміністратора.
        """
        pending = await self._repository.list_pending(limit=limit)
        return [item for item in pending if self.is_expired(item.created_at)]


__all__ = [
    "PrroOfflineQueue",
    "PRRO_OFFLINE_LIMIT_HOURS",
    "CHECK_TYPE_CHK",
    "CHECK_TYPE_ZREPORT",
    "CHECK_TYPE_SERVICECHK",
]
