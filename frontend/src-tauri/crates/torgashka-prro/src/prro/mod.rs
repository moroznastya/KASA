//! ПРРО: зміни (shift), офлайн-черга (queue), синхронізація (sync) — етап 7.3.
//! 1:1 Python `backend/app/application/use_cases/prro/` +
//! `backend/app/infrastructure/services/prro/offline_queue.py`.

pub mod chk_sender;
pub mod fiscalize;
pub mod models;
pub mod offline;
pub mod queue;
pub mod repository;
pub mod settings;
pub mod status_codes;
pub mod shift;
pub mod sync;

pub use chk_sender::{ChkSender, MockChkSender};
pub use fiscalize::{
    FiscalizeReceiptUseCase, FiscalizeRequestDto, FiscalizeResponseDto, PrroFiscalizeError,
};
pub use models::{
    ProductFiscalRow, PrroQueueItem, PrroQueueStatus, PrroSetting, PrroShift, PrroShiftStatus,
    ReceiptFiscalRow, ReceiptItemFiscalRow, SplitItemInput, CHECK_TYPE_CHK, CHECK_TYPE_SERVICECHK,
    CHECK_TYPE_ZREPORT, KEY_LAST_MAC_NUMBER, KEY_LAST_PACKET_ID, KEY_LAST_SHIFT_NUMBER,
    KEY_PRRO_FN, KEY_PRRO_MODE, KEY_PRRO_TN, KEY_PRRO_URL, KEY_PRRO_ZN, PRRO_OFFLINE_LIMIT_HOURS,
};
pub use offline::OfflineStateMachine;
pub use queue::{PrroOfflineQueue, QueueError};
pub use repository::{InMemoryPrroRepository, PrroRepoError, PrroRepository};
pub use settings::{
    build_fiscal_check_url, config_mode, config_url, copy_key_file, parse_bool, uuid6,
    PrroKeyStore, PrroKeyStoreError, PrroSettingsDto, PrroSettingsError, PrroSettingsUseCase,
    PASSWORD_MASK, SERVICE_PING,
};
pub use shift::{PrroShiftDto, PrroShiftError, PrroShiftUseCase};
pub use sync::{check_type_code, SyncOfflineQueueUseCase};
