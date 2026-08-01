"""
Application Layer: PrroUseCases — фасад для всіх use cases ПРРО.

Об'єднує:
  - PrroSettingsUseCase    (налаштування, test_connection);
  - PrroShiftUseCase       (відкриття/закриття зміни);
  - FiscalizeReceiptUseCase (фіскалізація чеку);
  - SyncOfflineQueueUseCase (синхронізація офлайн-черги);
  - PrroStatusUseCase      (статус ПРРО, журнал черги).

Використовується як єдина залежність у API v2 (api/v2/prro.py)
та в тестах (можна замокати один об'єкт).
"""

from __future__ import annotations

from uuid import UUID

from app.application.dto.prro_dto import (
    CloseShiftRequestDTO,
    FiscalizeRequestDTO,
    FiscalizeResponseDTO,
    OpenShiftRequestDTO,
    PrroQueueItemDTO,
    PrroSettingsDTO,
    PrroShiftDTO,
    PrroStatusDTO,
)
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
)
from app.application.use_cases.prro.prro_settings_use_case import (
    PrroSettingsUseCase,
)
from app.application.use_cases.prro.prro_status_use_case import PrroStatusUseCase
from app.application.use_cases.prro.shift_use_case import PrroShiftUseCase
from app.application.use_cases.prro.sync_offline_queue_use_case import (
    SyncOfflineQueueUseCase,
)


class PrroUseCases:
    """
    Фасад use cases ПРРО.

    Args:
        settings: PrroSettingsUseCase.
        shift: PrroShiftUseCase.
        fiscalize: FiscalizeReceiptUseCase.
        sync: SyncOfflineQueueUseCase.
        status: PrroStatusUseCase.
    """

    def __init__(
        self,
        settings: PrroSettingsUseCase,
        shift: PrroShiftUseCase,
        fiscalize: FiscalizeReceiptUseCase,
        sync: SyncOfflineQueueUseCase,
        status: PrroStatusUseCase,
    ) -> None:
        self._settings = settings
        self._shift = shift
        self._fiscalize = fiscalize
        self._sync = sync
        self._status = status

    # ─── Налаштування ──────────────────────────────────────────────────────

    async def get_settings(self) -> PrroSettingsDTO:
        """Поточні налаштування ПРРО (пароль замасковано)."""
        return await self._settings.get_settings()

    async def save_settings(
        self,
        *,
        key_file_path: str | None = None,
        key_file_content: bytes | None = None,
        key_file_name: str | None = None,
        key_password: str | None = None,
        prro_fn: str | None = None,
        prro_tn: str | None = None,
        prro_zn: str | None = None,
        mode: str | None = None,
        auto_fiscalize: bool | None = None,
    ) -> PrroSettingsDTO:
        """Зберігає налаштування ПРРО."""
        return await self._settings.save_settings(
            key_file_path=key_file_path,
            key_file_content=key_file_content,
            key_file_name=key_file_name,
            key_password=key_password,
            prro_fn=prro_fn,
            prro_tn=prro_tn,
            prro_zn=prro_zn,
            mode=mode,
            auto_fiscalize=auto_fiscalize,
        )

    async def test_connection(self) -> dict:
        """Перевірка зв'язку з фіскальним сервером (ping)."""
        return await self._settings.test_connection()

    async def get_prro_fn(self) -> str | None:
        """Фіскальний номер ПРРО (для QR-посилань)."""
        return await self._settings.get_prro_fn()

    # ─── Зміни ─────────────────────────────────────────────────────────────

    async def open_shift(self, dto: OpenShiftRequestDTO | None = None) -> PrroShiftDTO:
        """Відкриває зміну ПРРО."""
        return await self._shift.open_shift(
            comment=dto.comment if dto else None
        )

    async def close_shift(self, dto: CloseShiftRequestDTO | None = None) -> PrroShiftDTO:
        """Закриває зміну ПРРО (Z-звіт)."""
        return await self._shift.close_shift(
            comment=dto.comment if dto else None
        )

    async def auto_reminder_check(self) -> dict | None:
        """Попередження про відкриту > 24 год зміну."""
        return await self._shift.auto_reminder_check()

    async def list_shifts(
        self, page: int = 1, size: int = 20
    ) -> tuple[list[PrroShiftDTO], int]:
        """Список змін з пагінацією."""
        return await self._shift.list_shifts(page=page, size=size)

    # ─── Фіскалізація ──────────────────────────────────────────────────────

    async def fiscalize_receipt(
        self, receipt_id: UUID, manual: bool = False
    ) -> FiscalizeResponseDTO:
        """Фіскалізує чек продажу/повернення."""
        return await self._fiscalize.fiscalize_receipt(
            receipt_id, manual=manual
        )

    # ─── Синхронізація ─────────────────────────────────────────────────────

    async def sync_offline_queue(self, limit: int = 100) -> dict:
        """Синхронізує офлайн-чергу ПРРО."""
        return await self._sync.sync(limit=limit)

    # ─── Статус ────────────────────────────────────────────────────────────

    async def get_status(self) -> PrroStatusDTO:
        """Статус ПРРО."""
        return await self._status.get_status()

    async def get_queue(
        self, page: int = 1, size: int = 20, status_filter: str | None = None
    ) -> dict:
        """Журнал офлайн-черги ПРРО."""
        return await self._status.get_queue(
            page=page, size=size, status_filter=status_filter
        )


__all__ = ["PrroUseCases"]
