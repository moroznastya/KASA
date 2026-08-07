//! Репозиторій ПРРО — абстракція БД (зміни + офлайн-черга + налаштування).
//! 1:1 Python `PrroRepository` + `PrroSettingsRepository` (об'єднано).
//! Реалізації: `InMemoryPrroRepository` (тести/еталони), sqlx — у
//! kasa-infrastructure (crates/kasa-infrastructure/src/prro/).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::models::{PrroQueueItem, PrroSetting, PrroShift};

/// Помилка репозиторію ПРРО (ізольована від sqlx — kasa-prro не залежить від БД).
#[derive(Debug, thiserror::Error)]
pub enum PrroRepoError {
    #[error("запис не знайдено")]
    NotFound,
    #[error("помилка БД: {0}")]
    Db(String),
    #[error("помилка валідації: {0}")]
    Validation(String),
}

/// Контракт репозиторію ПРРО — 1:1 Python `PrroRepository` + settings.
#[async_trait]
pub trait PrroRepository: Send + Sync {
    // ── PrroShift ────────────────────────────────────────────────────────
    async fn create_shift(&self, shift: PrroShift) -> Result<PrroShift, PrroRepoError>;
    async fn get_shift(&self, shift_id: Uuid) -> Result<Option<PrroShift>, PrroRepoError>;
    async fn get_shift_by_number(
        &self,
        shift_number: i64,
    ) -> Result<Option<PrroShift>, PrroRepoError>;
    /// Поточна відкрита зміна (найсвіжіша за opened_at).
    async fn get_open_shift(&self) -> Result<Option<PrroShift>, PrroRepoError>;
    async fn list_shifts(
        &self,
        page: u32,
        size: u32,
    ) -> Result<(Vec<PrroShift>, u64), PrroRepoError>;
    #[allow(clippy::too_many_arguments)]
    async fn close_shift(
        &self,
        shift_id: Uuid,
        closed_at: DateTime<Utc>,
        closed_by: String,
        zreport_number: String,
        signer_serial: Option<String>,
        signer_name: Option<String>,
    ) -> Result<Option<PrroShift>, PrroRepoError>;
    async fn increment_shift_counters(
        &self,
        shift_id: Uuid,
        amount: Decimal,
        last_local_number: Option<i64>,
        last_mac: Option<String>,
    ) -> Result<Option<PrroShift>, PrroRepoError>;

    // ── PrroQueueItem ────────────────────────────────────────────────────
    async fn add_to_queue(&self, item: PrroQueueItem) -> Result<PrroQueueItem, PrroRepoError>;
    async fn get_queue_item(&self, item_id: Uuid) -> Result<Option<PrroQueueItem>, PrroRepoError>;
    /// pending/failed у порядку черги (спершу pending за created_at) — 1:1.
    async fn list_pending(&self, limit: u32) -> Result<Vec<PrroQueueItem>, PrroRepoError>;
    async fn list_by_shift(&self, shift_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError>;
    async fn list_by_receipt(&self, receipt_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError>;
    async fn mark_sent(
        &self,
        item_id: Uuid,
        sent_at: Option<DateTime<Utc>>,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError>;
    async fn mark_failed(
        &self,
        item_id: Uuid,
        error: String,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError>;
    async fn count_pending(&self) -> Result<u64, PrroRepoError>;
    async fn delete_queue_item(&self, item_id: Uuid) -> Result<bool, PrroRepoError>;

    // ── PrroSetting ──────────────────────────────────────────────────────
    async fn get_setting(&self, key: &str) -> Result<Option<String>, PrroRepoError>;
    async fn set_setting(&self, key: &str, value: &str) -> Result<(), PrroRepoError>;
}

/// In-memory реалізація (тести, еталони) — детермінована, без БД.
#[derive(Debug, Default)]
pub struct InMemoryPrroRepository {
    shifts: std::sync::Mutex<Vec<PrroShift>>,
    queue: std::sync::Mutex<Vec<PrroQueueItem>>,
    settings: std::sync::Mutex<Vec<PrroSetting>>,
}

impl InMemoryPrroRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Вставляє зміну напряму (для фіксації початкового стану в тестах).
    pub fn seed_shift(&self, shift: PrroShift) {
        self.shifts.lock().unwrap().push(shift);
    }

    /// Вставляє запис черги напряму.
    pub fn seed_queue(&self, item: PrroQueueItem) {
        self.queue.lock().unwrap().push(item);
    }

    /// Перевизначає created_at запису черги (для тестів expired).
    pub fn set_queue_created_at(&self, item_id: Uuid, created_at: DateTime<Utc>) {
        let mut queue = self.queue.lock().unwrap();
        if let Some(item) = queue.iter_mut().find(|i| i.id == item_id) {
            item.created_at = created_at;
        }
    }

    /// Вставляє налаштування напряму.
    pub fn seed_setting(&self, key: &str, value: &str) {
        self.settings.lock().unwrap().push(PrroSetting {
            key_name: key.to_string(),
            value: Some(value.to_string()),
        });
    }
}

#[async_trait]
impl PrroRepository for InMemoryPrroRepository {
    async fn create_shift(&self, shift: PrroShift) -> Result<PrroShift, PrroRepoError> {
        self.shifts.lock().unwrap().push(shift.clone());
        Ok(shift)
    }

    async fn get_shift(&self, shift_id: Uuid) -> Result<Option<PrroShift>, PrroRepoError> {
        Ok(self
            .shifts
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == shift_id)
            .cloned())
    }

    async fn get_shift_by_number(
        &self,
        shift_number: i64,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        Ok(self
            .shifts
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.shift_number == shift_number)
            .cloned())
    }

    async fn get_open_shift(&self) -> Result<Option<PrroShift>, PrroRepoError> {
        let shifts = self.shifts.lock().unwrap();
        Ok(shifts
            .iter()
            .filter(|s| matches!(s.status, super::models::PrroShiftStatus::Open))
            .max_by_key(|s| s.opened_at)
            .cloned())
    }

    async fn list_shifts(
        &self,
        page: u32,
        size: u32,
    ) -> Result<(Vec<PrroShift>, u64), PrroRepoError> {
        let shifts = self.shifts.lock().unwrap();
        let mut sorted: Vec<_> = shifts.iter().cloned().collect();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.shift_number));
        let total = sorted.len() as u64;
        let offset = ((page.saturating_sub(1)) * size) as usize;
        let page_items = sorted
            .into_iter()
            .skip(offset)
            .take(size as usize)
            .collect();
        Ok((page_items, total))
    }

    #[allow(clippy::too_many_arguments)]
    async fn close_shift(
        &self,
        shift_id: Uuid,
        closed_at: DateTime<Utc>,
        closed_by: String,
        zreport_number: String,
        signer_serial: Option<String>,
        signer_name: Option<String>,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let mut shifts = self.shifts.lock().unwrap();
        let shift = shifts
            .iter_mut()
            .find(|s| s.id == shift_id)
            .ok_or(PrroRepoError::NotFound)?;
        shift.status = super::models::PrroShiftStatus::Closed;
        shift.closed_at = Some(closed_at);
        shift.closed_by = Some(closed_by);
        shift.zreport_number = Some(zreport_number);
        if signer_serial.is_some() {
            shift.signer_serial = signer_serial;
        }
        if signer_name.is_some() {
            shift.signer_name = signer_name;
        }
        Ok(Some(shift.clone()))
    }

    async fn increment_shift_counters(
        &self,
        shift_id: Uuid,
        amount: Decimal,
        last_local_number: Option<i64>,
        last_mac: Option<String>,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let mut shifts = self.shifts.lock().unwrap();
        let shift = shifts
            .iter_mut()
            .find(|s| s.id == shift_id)
            .ok_or(PrroRepoError::NotFound)?;
        shift.receipt_count += 1;
        shift.total_amount += amount;
        if let Some(n) = last_local_number {
            shift.last_local_number = n;
        }
        if last_mac.is_some() {
            shift.last_mac = last_mac;
        }
        Ok(Some(shift.clone()))
    }

    async fn add_to_queue(&self, item: PrroQueueItem) -> Result<PrroQueueItem, PrroRepoError> {
        self.queue.lock().unwrap().push(item.clone());
        Ok(item)
    }

    async fn get_queue_item(&self, item_id: Uuid) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        Ok(self
            .queue
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.id == item_id)
            .cloned())
    }

    async fn list_pending(&self, limit: u32) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let queue = self.queue.lock().unwrap();
        let mut items: Vec<_> = queue
            .iter()
            .filter(|i| {
                matches!(
                    i.status,
                    super::models::PrroQueueStatus::Pending
                        | super::models::PrroQueueStatus::Failed
                )
            })
            .cloned()
            .collect();
        // порядок: спершу pending (за created_at), потім failed (за created_at)
        items.sort_by_key(|i| (i.status.as_str() != "pending", i.created_at));
        Ok(items.into_iter().take(limit as usize).collect())
    }

    async fn list_by_shift(&self, shift_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let mut items: Vec<_> = self
            .queue
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.shift_id == Some(shift_id))
            .cloned()
            .collect();
        items.sort_by_key(|i| i.local_number);
        Ok(items)
    }

    async fn list_by_receipt(&self, receipt_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let mut items: Vec<_> = self
            .queue
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.receipt_id == Some(receipt_id))
            .cloned()
            .collect();
        items.sort_by_key(|i| i.created_at);
        Ok(items)
    }

    async fn mark_sent(
        &self,
        item_id: Uuid,
        sent_at: Option<DateTime<Utc>>,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let mut queue = self.queue.lock().unwrap();
        let item = queue
            .iter_mut()
            .find(|i| i.id == item_id)
            .ok_or(PrroRepoError::NotFound)?;
        item.status = super::models::PrroQueueStatus::Sent;
        item.sent_at = Some(sent_at.unwrap_or_else(Utc::now));
        item.error = None;
        Ok(Some(item.clone()))
    }

    async fn mark_failed(
        &self,
        item_id: Uuid,
        error: String,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let mut queue = self.queue.lock().unwrap();
        let item = queue
            .iter_mut()
            .find(|i| i.id == item_id)
            .ok_or(PrroRepoError::NotFound)?;
        item.status = super::models::PrroQueueStatus::Failed;
        item.error = Some(error);
        Ok(Some(item.clone()))
    }

    async fn count_pending(&self) -> Result<u64, PrroRepoError> {
        Ok(self
            .queue
            .lock()
            .unwrap()
            .iter()
            .filter(|i| matches!(i.status, super::models::PrroQueueStatus::Pending))
            .count() as u64)
    }

    async fn delete_queue_item(&self, item_id: Uuid) -> Result<bool, PrroRepoError> {
        let mut queue = self.queue.lock().unwrap();
        let before = queue.len();
        queue.retain(|i| i.id != item_id);
        Ok(queue.len() != before)
    }

    async fn get_setting(&self, key: &str) -> Result<Option<String>, PrroRepoError> {
        Ok(self
            .settings
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.key_name == key)
            .and_then(|s| s.value.clone()))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), PrroRepoError> {
        let mut settings = self.settings.lock().unwrap();
        if let Some(s) = settings.iter_mut().find(|s| s.key_name == key) {
            s.value = Some(value.to_string());
        } else {
            settings.push(PrroSetting {
                key_name: key.to_string(),
                value: Some(value.to_string()),
            });
        }
        Ok(())
    }
}
