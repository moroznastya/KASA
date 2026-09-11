//! Репозиторій ПРРО — абстракція БД (зміни + офлайн-черга + налаштування).
//! 1:1 Python `PrroRepository` + `PrroSettingsRepository` (об'єднано).
//! Реалізації: `InMemoryPrroRepository` (тести/еталони), sqlx — у
//! torgashka-infrastructure (crates/torgashka-infrastructure/src/prro/).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::models::{
    ProductFiscalRow, PrroQueueItem, PrroSetting, PrroShift, ReceiptFiscalRow,
    ReceiptItemFiscalRow, SplitItemInput,
};

/// Помилка репозиторію ПРРО (ізольована від sqlx — torgashka-prro не залежить від БД).
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
#[allow(clippy::too_many_arguments)]
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

    /// M1: атомарний local_number — інкремент + збереження в одній операції.
    /// Повертає НОВЕ значення last_local_number. Гарантує N унікальних
    /// послідовних номерів при N паралельних фіскалізаціях.
    async fn next_local_number(&self, shift_id: Uuid) -> Result<i64, PrroRepoError>;
    /// Оновлює лише last_mac зміни (B1: hash-ланцюжок після sync-відправки).
    async fn update_shift_last_mac(
        &self,
        shift_id: Uuid,
        last_mac: String,
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
    /// B2: зберігає повний підписаний check_sign (ідемпотентність sync).
    async fn update_queue_check_sign(
        &self,
        item_id: Uuid,
        check_sign: String,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError>;
    async fn count_pending(&self) -> Result<u64, PrroRepoError>;
    async fn delete_queue_item(&self, item_id: Uuid) -> Result<bool, PrroRepoError>;

    // ── PrroSetting ──────────────────────────────────────────────────────
    async fn get_setting(&self, key: &str) -> Result<Option<String>, PrroRepoError>;
    async fn set_setting(&self, key: &str, value: &str) -> Result<(), PrroRepoError>;

    // ── Фіскалізація (група 8/9) — 1:1 Python FiscalizeReceiptUseCase ────
    /// Чек з позиціями (1:1 Python `_load_receipt` + selectinload(items)).
    async fn load_receipt_with_items(
        &self,
        receipt_id: Uuid,
    ) -> Result<Option<ReceiptFiscalRow>, PrroRepoError>;
    /// Товар за ID (1:1 Python `_load_product`).
    async fn load_product(
        &self,
        product_id: Uuid,
    ) -> Result<Option<ProductFiscalRow>, PrroRepoError>;
    /// Оновлює фіскальний стан чеку (1:1 Python: receipt.fiscal_status=...).
    async fn update_receipt_fiscal_state(
        &self,
        receipt_id: Uuid,
        fiscal_status: &str,
        fiscal_number: Option<&str>,
        fiscal_serial: Option<&str>,
        fiscal_sent_at: Option<DateTime<Utc>>,
        fiscal_error: Option<&str>,
        is_fiscal: Option<bool>,
    ) -> Result<(), PrroRepoError>;
    /// Перераховує total_amount чеку (split: сума фіскальних позицій).
    async fn update_receipt_total(
        &self,
        receipt_id: Uuid,
        total_amount: Decimal,
    ) -> Result<(), PrroRepoError>;
    /// Видаляє позиції з fiscal_quantity <= 0 (split: cascade delete).
    async fn delete_non_fiscal_items(&self, receipt_id: Uuid) -> Result<u64, PrroRepoError>;
    /// Оновлює quantity/total/fiscal_quantity позиції (split: ефективна кількість).
    async fn update_item_fiscal(
        &self,
        item_id: Uuid,
        quantity: Decimal,
        total: Decimal,
        fiscal_quantity: Decimal,
    ) -> Result<(), PrroRepoError>;
    /// Створює нефіскальний дублікат чеку + позиції (split, 1:1 Python).
    async fn create_receipt_duplicate(
        &self,
        source: &ReceiptFiscalRow,
        items: &[SplitItemInput],
        is_return: bool,
        split_group_id: Uuid,
    ) -> Result<Uuid, PrroRepoError>;
    /// Оновлює fiscal_stock товару (1:1 Python product.fiscal_stock).
    async fn update_product_fiscal_stock(
        &self,
        product_id: Uuid,
        new_stock: Decimal,
    ) -> Result<(), PrroRepoError>;
}

/// In-memory реалізація (тести, еталони) — детермінована, без БД.
#[derive(Debug, Default)]
pub struct InMemoryPrroRepository {
    shifts: std::sync::Mutex<Vec<PrroShift>>,
    queue: std::sync::Mutex<Vec<PrroQueueItem>>,
    settings: std::sync::Mutex<Vec<PrroSetting>>,
    receipts: std::sync::Mutex<Vec<ReceiptFiscalRow>>,
    products: std::sync::Mutex<Vec<ProductFiscalRow>>,
}

impl InMemoryPrroRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Вставляє зміну напряму (для фіксації початкового стану в тестах).
    pub fn seed_shift(&self, shift: PrroShift) {
        self.shifts
            .lock()
            .expect("lock poisoned: shifts")
            .push(shift);
    }

    /// Вставляє запис черги напряму.
    pub fn seed_queue(&self, item: PrroQueueItem) {
        self.queue.lock().expect("lock poisoned: queue").push(item);
    }

    /// Перевизначає created_at запису черги (для тестів expired).
    pub fn set_queue_created_at(&self, item_id: Uuid, created_at: DateTime<Utc>) {
        let mut queue = self.queue.lock().expect("lock poisoned: queue");
        if let Some(item) = queue.iter_mut().find(|i| i.id == item_id) {
            item.created_at = created_at;
        }
    }

    /// Вставляє налаштування напряму.
    pub fn seed_setting(&self, key: &str, value: &str) {
        self.settings
            .lock()
            .expect("lock poisoned: settings")
            .push(PrroSetting {
                key_name: key.to_string(),
                value: Some(value.to_string()),
            });
    }

    pub fn seed_receipt(&self, receipt: ReceiptFiscalRow) {
        self.receipts
            .lock()
            .expect("lock poisoned: receipts")
            .push(receipt);
    }

    pub fn seed_product(&self, product: ProductFiscalRow) {
        self.products
            .lock()
            .expect("lock poisoned: products")
            .push(product);
    }
}

#[async_trait]
impl PrroRepository for InMemoryPrroRepository {
    async fn create_shift(&self, shift: PrroShift) -> Result<PrroShift, PrroRepoError> {
        self.shifts
            .lock()
            .expect("lock poisoned: shifts")
            .push(shift.clone());
        Ok(shift)
    }

    async fn get_shift(&self, shift_id: Uuid) -> Result<Option<PrroShift>, PrroRepoError> {
        Ok(self
            .shifts
            .lock()
            .expect("lock poisoned: mutex")
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
            .expect("lock poisoned: mutex")
            .iter()
            .find(|s| s.shift_number == shift_number)
            .cloned())
    }

    async fn get_open_shift(&self) -> Result<Option<PrroShift>, PrroRepoError> {
        let shifts = self.shifts.lock().expect("lock poisoned: shifts");
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
        let shifts = self.shifts.lock().expect("lock poisoned: shifts");
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
        let mut shifts = self.shifts.lock().expect("lock poisoned: shifts");
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
        let mut shifts = self.shifts.lock().expect("lock poisoned: shifts");
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

    async fn next_local_number(&self, shift_id: Uuid) -> Result<i64, PrroRepoError> {
        // M1: одна lock-секція → read-modify-write атомарний для конкурентних
        // фіскалізацій в одному процесі (аналог SQL UPDATE ... RETURNING).
        let mut shifts = self.shifts.lock().expect("lock poisoned: shifts");
        let shift = shifts
            .iter_mut()
            .find(|s| s.id == shift_id)
            .ok_or(PrroRepoError::NotFound)?;
        let next = shift.last_local_number + 1;
        shift.last_local_number = next;
        Ok(next)
    }

    async fn update_shift_last_mac(
        &self,
        shift_id: Uuid,
        last_mac: String,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let mut shifts = self.shifts.lock().expect("lock poisoned: shifts");
        let shift = shifts
            .iter_mut()
            .find(|s| s.id == shift_id)
            .ok_or(PrroRepoError::NotFound)?;
        shift.last_mac = Some(last_mac);
        Ok(Some(shift.clone()))
    }

    async fn add_to_queue(&self, item: PrroQueueItem) -> Result<PrroQueueItem, PrroRepoError> {
        self.queue
            .lock()
            .expect("lock poisoned: queue")
            .push(item.clone());
        Ok(item)
    }

    async fn get_queue_item(&self, item_id: Uuid) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        Ok(self
            .queue
            .lock()
            .expect("lock poisoned: mutex")
            .iter()
            .find(|i| i.id == item_id)
            .cloned())
    }

    async fn list_pending(&self, limit: u32) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let queue = self.queue.lock().expect("lock poisoned: queue");
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
            .expect("lock poisoned: mutex")
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
            .expect("lock poisoned: mutex")
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
        let mut queue = self.queue.lock().expect("lock poisoned: queue");
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
        let mut queue = self.queue.lock().expect("lock poisoned: queue");
        let item = queue
            .iter_mut()
            .find(|i| i.id == item_id)
            .ok_or(PrroRepoError::NotFound)?;
        item.status = super::models::PrroQueueStatus::Failed;
        item.error = Some(error);
        Ok(Some(item.clone()))
    }

    async fn update_queue_check_sign(
        &self,
        item_id: Uuid,
        check_sign: String,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let mut queue = self.queue.lock().expect("lock poisoned: queue");
        let item = queue
            .iter_mut()
            .find(|i| i.id == item_id)
            .ok_or(PrroRepoError::NotFound)?;
        item.check_sign = Some(check_sign);
        Ok(Some(item.clone()))
    }

    async fn count_pending(&self) -> Result<u64, PrroRepoError> {
        Ok(self
            .queue
            .lock()
            .expect("lock poisoned: mutex")
            .iter()
            .filter(|i| matches!(i.status, super::models::PrroQueueStatus::Pending))
            .count() as u64)
    }

    async fn delete_queue_item(&self, item_id: Uuid) -> Result<bool, PrroRepoError> {
        let mut queue = self.queue.lock().expect("lock poisoned: queue");
        let before = queue.len();
        queue.retain(|i| i.id != item_id);
        Ok(queue.len() != before)
    }

    async fn get_setting(&self, key: &str) -> Result<Option<String>, PrroRepoError> {
        Ok(self
            .settings
            .lock()
            .expect("lock poisoned: mutex")
            .iter()
            .find(|s| s.key_name == key)
            .and_then(|s| s.value.clone()))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), PrroRepoError> {
        let mut settings = self.settings.lock().expect("lock poisoned: settings");
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

    async fn load_receipt_with_items(
        &self,
        receipt_id: Uuid,
    ) -> Result<Option<ReceiptFiscalRow>, PrroRepoError> {
        Ok(self
            .receipts
            .lock()
            .expect("lock poisoned: mutex")
            .iter()
            .find(|r| r.id == receipt_id)
            .cloned())
    }

    async fn load_product(
        &self,
        product_id: Uuid,
    ) -> Result<Option<ProductFiscalRow>, PrroRepoError> {
        Ok(self
            .products
            .lock()
            .expect("lock poisoned: mutex")
            .iter()
            .find(|p| p.id == product_id)
            .cloned())
    }

    async fn update_receipt_fiscal_state(
        &self,
        receipt_id: Uuid,
        fiscal_status: &str,
        fiscal_number: Option<&str>,
        fiscal_serial: Option<&str>,
        fiscal_sent_at: Option<DateTime<Utc>>,
        fiscal_error: Option<&str>,
        is_fiscal: Option<bool>,
    ) -> Result<(), PrroRepoError> {
        let mut receipts = self.receipts.lock().expect("lock poisoned: receipts");
        let r = receipts
            .iter_mut()
            .find(|r| r.id == receipt_id)
            .ok_or(PrroRepoError::NotFound)?;
        r.fiscal_status = fiscal_status.to_string();
        r.fiscal_number = fiscal_number.map(str::to_string);
        r.fiscal_serial = fiscal_serial.map(str::to_string);
        r.fiscal_sent_at = fiscal_sent_at;
        r.fiscal_error = fiscal_error.map(str::to_string);
        if let Some(v) = is_fiscal {
            r.is_fiscal = v;
        }
        Ok(())
    }

    async fn update_receipt_total(
        &self,
        receipt_id: Uuid,
        total_amount: Decimal,
    ) -> Result<(), PrroRepoError> {
        let mut receipts = self.receipts.lock().expect("lock poisoned: receipts");
        let r = receipts
            .iter_mut()
            .find(|r| r.id == receipt_id)
            .ok_or(PrroRepoError::NotFound)?;
        r.total_amount = total_amount;
        Ok(())
    }

    async fn delete_non_fiscal_items(&self, receipt_id: Uuid) -> Result<u64, PrroRepoError> {
        let mut receipts = self.receipts.lock().expect("lock poisoned: receipts");
        let r = receipts
            .iter_mut()
            .find(|r| r.id == receipt_id)
            .ok_or(PrroRepoError::NotFound)?;
        let before = r.items.len();
        r.items.retain(|i| i.fiscal_quantity > Decimal::ZERO);
        Ok((before - r.items.len()) as u64)
    }

    async fn update_item_fiscal(
        &self,
        item_id: Uuid,
        quantity: Decimal,
        total: Decimal,
        fiscal_quantity: Decimal,
    ) -> Result<(), PrroRepoError> {
        let mut receipts = self.receipts.lock().expect("lock poisoned: receipts");
        for r in receipts.iter_mut() {
            if let Some(item) = r.items.iter_mut().find(|i| i.id == item_id) {
                item.quantity = quantity;
                item.total = total;
                item.fiscal_quantity = fiscal_quantity;
                return Ok(());
            }
        }
        Err(PrroRepoError::NotFound)
    }

    async fn create_receipt_duplicate(
        &self,
        source: &ReceiptFiscalRow,
        items: &[SplitItemInput],
        is_return: bool,
        split_group_id: Uuid,
    ) -> Result<Uuid, PrroRepoError> {
        let id = Uuid::new_v4();
        let dup = ReceiptFiscalRow {
            id,
            receipt_number: format!(
                "NF-{}-{}",
                source.receipt_number.chars().take(20).collect::<String>(),
                &uuid::Uuid::new_v4().simple().to_string()[..6]
            ),
            cashier_id: source.cashier_id,
            total_amount: items.iter().map(|i| i.total).sum(),
            paid_amount: source.paid_amount,
            change_amount: source.change_amount,
            debtor_id: source.debtor_id,
            is_return,
            notes: source.notes.clone(),
            payment_method: source.payment_method.clone(),
            cash_amount: source.cash_amount,
            card_amount: source.card_amount,
            original_receipt_id: source.original_receipt_id,
            return_reason: source.return_reason.clone(),
            split_group_id: Some(split_group_id),
            fiscal_status: "none".to_string(),
            fiscal_number: None,
            fiscal_serial: None,
            fiscal_sent_at: None,
            fiscal_error: None,
            is_fiscal: false,
            items: items
                .iter()
                .map(|i| ReceiptItemFiscalRow {
                    id: Uuid::new_v4(),
                    product_id: i.product_id,
                    quantity: i.quantity,
                    price: i.price,
                    total: i.total,
                    purchase_price: i.purchase_price,
                    fiscal_quantity: Decimal::ZERO,
                })
                .collect(),
        };
        self.receipts
            .lock()
            .expect("lock poisoned: receipts")
            .push(dup);
        Ok(id)
    }

    async fn update_product_fiscal_stock(
        &self,
        product_id: Uuid,
        new_stock: Decimal,
    ) -> Result<(), PrroRepoError> {
        let mut products = self.products.lock().expect("lock poisoned: products");
        let p = products
            .iter_mut()
            .find(|p| p.id == product_id)
            .ok_or(PrroRepoError::NotFound)?;
        p.fiscal_stock = new_stock;
        Ok(())
    }
}
