//! ПРРО: зміни (shift), офлайн-черга (queue), синхронізація (sync) — етап 7.3.
//! 1:1 Python `backend/app/application/use_cases/prro/` +
//! `backend/app/infrastructure/services/prro/offline_queue.py`.

pub mod chk_sender;
pub mod models;
pub mod queue;
pub mod repository;
pub mod shift;
pub mod sync;

pub use chk_sender::{ChkSender, MockChkSender};
pub use models::{
    PrroQueueItem, PrroQueueStatus, PrroSetting, PrroShift, PrroShiftStatus, CHECK_TYPE_CHK,
    CHECK_TYPE_SERVICECHK, CHECK_TYPE_ZREPORT, KEY_LAST_MAC_NUMBER, KEY_LAST_PACKET_ID,
    KEY_LAST_SHIFT_NUMBER, KEY_PRRO_FN, KEY_PRRO_MODE, KEY_PRRO_TN, KEY_PRRO_URL, KEY_PRRO_ZN,
    PRRO_OFFLINE_LIMIT_HOURS,
};
pub use queue::{PrroOfflineQueue, QueueError};
pub use repository::{InMemoryPrroRepository, PrroRepoError, PrroRepository};
pub use shift::{PrroShiftDto, PrroShiftError, PrroShiftUseCase};
pub use sync::{check_type_code, SyncOfflineQueueUseCase};
