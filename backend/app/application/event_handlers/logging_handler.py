"""Event Handler: Логування доменних подій."""

from __future__ import annotations

import logging
from typing import Any

from app.domain.events import (
    BaseDomainEvent,
    ProductCreated,
    InvoiceCreated,
    ReceiptCreated,
    StockChanged,
)

logger = logging.getLogger(__name__)


class LoggingHandler:
    """Логування всіх доменних подій."""

    async def handle(self, event: BaseDomainEvent) -> None:
        """Загальний обробник для логування."""
        logger.info(
            f"[DOMAIN EVENT] {event.event_name} | "
            f"ID: {event.event_id} | "
            f"Time: {event.occurred_at.isoformat()}"
        )

        # Специфічне логування для важливих подій
        if isinstance(event, ProductCreated):
            logger.info(f"  ➕ Товар створено: {event.name} (штрих-код: {event.barcode})")
        elif isinstance(event, InvoiceCreated):
            logger.info(f"  📄 Накладна створена: {event.invoice_id}, сума: {event.total_amount}")
        elif isinstance(event, ReceiptCreated):
            logger.info(f"  🧾 Чек: {event.receipt_id}, сума: {event.total_amount}")
        elif isinstance(event, StockChanged):
            logger.info(
                f"  📦 Залишок змінено: {event.product_id} | "
                f"{event.old_quantity} → {event.new_quantity} | "
                f"причина: {event.reason}"
            )
