"""
Application Layer: use cases ПРРО (програмний РРО).

Пакет містить:
  - prro_settings_use_case.py      — налаштування ПРРО + test_connection;
  - shift_use_case.py              — відкриття/закриття зміни (Z-звіт);
  - fiscalize_receipt_use_case.py  — фіскалізація чеку (T=0/T=1);
  - sync_offline_queue_use_case.py — повторна передача офлайн-черги;
  - prro_status_use_case.py        — статус ПРРО (statusRro/infoRro);
  - prro_use_cases.py              — фасад для API v2;
  - context.py                     — фабрика компонентів ПРРО з налаштувань.
"""

from app.application.use_cases.prro.context import (
    PrroContextFactory,
    KEY_PRRO_FN,
    KEY_PRRO_TN,
    KEY_PRRO_ZN,
    KEY_PRRO_MODE,
    KEY_PRRO_URL,
    KEY_LAST_SHIFT_NUMBER,
    KEY_LAST_PACKET_ID,
    KEY_LAST_MAC_NUMBER,
    KEY_AUTO_FISCALIZE,
    CHECK_TYPE_CHK,
    CHECK_TYPE_ZREPORT,
    CHECK_TYPE_SERVICECHK,
)
from app.application.use_cases.prro.prro_settings_use_case import (
    PrroSettingsUseCase,
    PrroSettingsError,
)
from app.application.use_cases.prro.shift_use_case import (
    PrroShiftUseCase,
    PrroShiftError,
)
from app.application.use_cases.prro.fiscalize_receipt_use_case import (
    FiscalizeReceiptUseCase,
    PrroFiscalizeError,
)
from app.application.use_cases.prro.sync_offline_queue_use_case import (
    SyncOfflineQueueUseCase,
)
from app.application.use_cases.prro.prro_status_use_case import PrroStatusUseCase
from app.application.use_cases.prro.prro_use_cases import PrroUseCases

__all__ = [
    "PrroContextFactory",
    "KEY_PRRO_FN",
    "KEY_PRRO_TN",
    "KEY_PRRO_ZN",
    "KEY_PRRO_MODE",
    "KEY_PRRO_URL",
    "KEY_LAST_SHIFT_NUMBER",
    "KEY_LAST_PACKET_ID",
    "KEY_LAST_MAC_NUMBER",
    "KEY_AUTO_FISCALIZE",
    "CHECK_TYPE_CHK",
    "CHECK_TYPE_ZREPORT",
    "CHECK_TYPE_SERVICECHK",
    "PrroSettingsUseCase",
    "PrroSettingsError",
    "PrroShiftUseCase",
    "PrroShiftError",
    "FiscalizeReceiptUseCase",
    "PrroFiscalizeError",
    "SyncOfflineQueueUseCase",
    "PrroStatusUseCase",
    "PrroUseCases",
]
