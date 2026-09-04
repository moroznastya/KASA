//! SqlxPrroRepository — PostgreSQL-реалізація `torgashka_prro::prro::PrroRepository`.

use crate::store_ctx::{current_store_ctx, StorePool};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use torgashka_prro::prro::{
    PrroQueueItem, PrroQueueStatus, PrroRepoError, PrroRepository, PrroSetting, PrroShift,
    PrroShiftStatus,
};
use uuid::Uuid;

/// sqlx-рядок таблиці prro_shifts (для FromRow → доменна модель).
#[derive(sqlx::FromRow)]
struct ShiftRow {
    id: Uuid,
    shift_number: i32,
    opened_at: NaiveDateTime,
    closed_at: Option<NaiveDateTime>,
    signer_serial: Option<String>,
    signer_name: Option<String>,
    closed_by: Option<String>,
    zreport_number: Option<String>,
    status: String,
    receipt_count: i32,
    total_amount: Decimal,
    last_local_number: i32,
    last_mac: Option<String>,
}

impl From<ShiftRow> for PrroShift {
    fn from(r: ShiftRow) -> Self {
        Self {
            id: r.id,
            shift_number: r.shift_number as i64,
            opened_at: DateTime::from_naive_utc_and_offset(r.opened_at, Utc),
            closed_at: r
                .closed_at
                .map(|d| DateTime::from_naive_utc_and_offset(d, Utc)),
            signer_serial: r.signer_serial,
            signer_name: r.signer_name,
            closed_by: r.closed_by,
            zreport_number: r.zreport_number,
            status: if r.status == "closed" {
                PrroShiftStatus::Closed
            } else {
                PrroShiftStatus::Open
            },
            receipt_count: r.receipt_count as i64,
            total_amount: r.total_amount,
            last_local_number: r.last_local_number as i64,
            last_mac: r.last_mac,
        }
    }
}

/// sqlx-рядок таблиці prro_queue_items.
#[derive(sqlx::FromRow)]
struct QueueRow {
    id: Uuid,
    receipt_id: Option<Uuid>,
    shift_id: Option<Uuid>,
    local_number: i32,
    check_type: String,
    xml_body: String,
    check_sign: Option<String>,
    id_offline: Option<String>,
    mac: Option<String>,
    status: String,
    error: Option<String>,
    created_at: NaiveDateTime,
    sent_at: Option<NaiveDateTime>,
}

impl From<QueueRow> for PrroQueueItem {
    fn from(r: QueueRow) -> Self {
        Self {
            id: r.id,
            receipt_id: r.receipt_id,
            shift_id: r.shift_id,
            local_number: r.local_number as i64,
            check_type: r.check_type,
            xml_body: r.xml_body,
            check_sign: r.check_sign,
            id_offline: r.id_offline,
            mac: r.mac,
            status: match r.status.as_str() {
                "sent" => PrroQueueStatus::Sent,
                "failed" => PrroQueueStatus::Failed,
                _ => PrroQueueStatus::Pending,
            },
            error: r.error,
            created_at: DateTime::from_naive_utc_and_offset(r.created_at, Utc),
            sent_at: r
                .sent_at
                .map(|d| DateTime::from_naive_utc_and_offset(d, Utc)),
        }
    }
}

/// Репозиторій ПРРО на PostgreSQL.
#[derive(Clone)]
pub struct SqlxPrroRepository {
    pool: StorePool,
}

impl SqlxPrroRepository {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }

    /// Створює репозиторій і гарантує наявність схеми (ідемпотентно).
    pub async fn connect(pool: StorePool) -> Result<Self, sqlx::Error> {
        super::schema::ensure_prro_schema(&pool).await?;
        Ok(Self { pool })
    }

    fn map_err(e: sqlx::Error) -> PrroRepoError {
        PrroRepoError::Db(e.to_string())
    }

    /// DateTime<Utc> → naive UTC (колонки TIMESTAMP, 1:1 Alembic DateTime()).
    fn naive(dt: DateTime<Utc>) -> NaiveDateTime {
        dt.naive_utc()
    }

    /// store_id поточного запиту (StoreCtx, X-Store-Id) — ОБОВ'ЯЗКОВИЙ для
    /// per-store ізоляції (prro-таблиці мають store_id NOT NULL + RLS).
    /// Репозиторій викликається лише з HTTP-хендлерів під store_middleware;
    /// поза HTTP (sync/фон) контексту немає — чесна помилка, а не запис у
    /// «глобальний» рядок (мультиточкових глобальних таблиць більше немає).
    fn ctx_store_id(&self) -> Result<Uuid, PrroRepoError> {
        current_store_ctx()
            .map(|c| c.store_id)
            .ok_or_else(|| {
                PrroRepoError::Validation(
                    "ПРРО-операція поза контекстом торговельної точки (StoreCtx не встановлено); \
                     передайте store_id через X-Store-Id або with_store_ctx"
                        .to_string(),
                )
            })
    }
}

/// SELECT: статус кастується в text (для FromRow String).
const SHIFT_COLS: &str = "id, shift_number, opened_at, closed_at, signer_serial, signer_name, \
     closed_by, zreport_number, status::text, receipt_count, total_amount, last_local_number, last_mac";
const QUEUE_COLS: &str =
    "id, receipt_id, shift_id, local_number, check_type, xml_body, check_sign, id_offline, mac, \
     status::text, error, created_at, sent_at";
/// INSERT: без касту — значення передаються параметрами ($n::enum).
const SHIFT_INSERT_COLS: &str =
    "id, shift_number, opened_at, closed_at, signer_serial, signer_name, \
     closed_by, zreport_number, status, receipt_count, total_amount, last_local_number, last_mac, store_id";
const QUEUE_INSERT_COLS: &str =
    "id, receipt_id, shift_id, local_number, check_type, xml_body, check_sign, id_offline, mac, \
     status, error, created_at, sent_at, store_id";

#[async_trait]
impl PrroRepository for SqlxPrroRepository {
    async fn create_shift(&self, shift: PrroShift) -> Result<PrroShift, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        sqlx::query(&format!(
            "INSERT INTO prro_shifts ({SHIFT_INSERT_COLS}) VALUES \
             ($1,$2,$3,$4,$5,$6,$7,$8,$9::prro_shift_status,$10,$11,$12,$13,$14)"
        ))
        .bind(shift.id)
        .bind(shift.shift_number)
        .bind(Self::naive(shift.opened_at))
        .bind(shift.closed_at.map(Self::naive))
        .bind(&shift.signer_serial)
        .bind(&shift.signer_name)
        .bind(&shift.closed_by)
        .bind(&shift.zreport_number)
        .bind(shift.status.as_str())
        .bind(shift.receipt_count as i32)
        .bind(shift.total_amount)
        .bind(shift.last_local_number as i32)
        .bind(&shift.last_mac)
        .bind(store_id)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(shift)
    }

    async fn get_shift(&self, shift_id: Uuid) -> Result<Option<PrroShift>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE id = $1 AND store_id = $2"
        ))
        .bind(shift_id)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn get_shift_by_number(
        &self,
        shift_number: i64,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE shift_number = $1 AND store_id = $2"
        ))
        .bind(shift_number)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn get_open_shift(&self) -> Result<Option<PrroShift>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE status = 'open'::prro_shift_status \
             AND store_id = $1 ORDER BY opened_at DESC LIMIT 1"
        ))
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn list_shifts(
        &self,
        page: u32,
        size: u32,
    ) -> Result<(Vec<PrroShift>, u64), PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let total: (i64,) = sqlx::query_as("SELECT count(*) FROM prro_shifts WHERE store_id = $1")
            .bind(store_id)
            .fetch_one(&self.pool)
            .await
            .map_err(Self::map_err)?;
        let offset = (page.saturating_sub(1) * size) as i64;
        let rows = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE store_id = $1 \
             ORDER BY shift_number DESC LIMIT $2 OFFSET $3"
        ))
        .bind(store_id)
        .bind(size as i64)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok((
            rows.into_iter().map(PrroShift::from).collect(),
            total.0 as u64,
        ))
    }

    async fn close_shift(
        &self,
        shift_id: Uuid,
        closed_at: DateTime<Utc>,
        closed_by: String,
        zreport_number: String,
        signer_serial: Option<String>,
        signer_name: Option<String>,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "UPDATE prro_shifts SET status = 'closed'::prro_shift_status, closed_at = $2, \
             closed_by = $3, zreport_number = $4, \
             signer_serial = COALESCE($5, signer_serial), signer_name = COALESCE($6, signer_name) \
             WHERE id = $1 AND store_id = $7 RETURNING {SHIFT_COLS}"
        ))
        .bind(shift_id)
        .bind(Self::naive(closed_at))
        .bind(closed_by)
        .bind(zreport_number)
        .bind(signer_serial)
        .bind(signer_name)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn increment_shift_counters(
        &self,
        shift_id: Uuid,
        amount: Decimal,
        last_local_number: Option<i64>,
        last_mac: Option<String>,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "UPDATE prro_shifts SET receipt_count = receipt_count + 1, \
             total_amount = total_amount + $2, \
             last_local_number = COALESCE($3, last_local_number), \
             last_mac = COALESCE($4, last_mac) \
             WHERE id = $1 AND store_id = $5 RETURNING {SHIFT_COLS}"
        ))
        .bind(shift_id)
        .bind(amount)
        .bind(last_local_number)
        .bind(last_mac)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn next_local_number(&self, shift_id: Uuid) -> Result<i64, PrroRepoError> {
        // M1: атомарний local_number — інкремент + збереження в одній SQL-операції
        // (UPDATE ... RETURNING), без read-then-write race.
        let store_id = self.ctx_store_id()?;
        let row: Option<(i64,)> = sqlx::query_as(
            "UPDATE prro_shifts SET last_local_number = COALESCE(last_local_number, 0) + 1              WHERE id = $1 AND store_id = $2 AND status = 'open'::prro_shift_status              RETURNING last_local_number",
        )
        .bind(shift_id)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        row.map(|r| r.0).ok_or(PrroRepoError::NotFound)
    }

    async fn update_shift_last_mac(
        &self,
        shift_id: Uuid,
        last_mac: String,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "UPDATE prro_shifts SET last_mac = $2 WHERE id = $1 AND store_id = $3 RETURNING {SHIFT_COLS}"
        ))
        .bind(shift_id)
        .bind(last_mac)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn add_to_queue(&self, item: PrroQueueItem) -> Result<PrroQueueItem, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        sqlx::query(&format!(
            "INSERT INTO prro_queue_items ({QUEUE_INSERT_COLS}) VALUES \
             ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::prro_queue_status,$11,$12,$13,$14)"
        ))
        .bind(item.id)
        .bind(item.receipt_id)
        .bind(item.shift_id)
        .bind(item.local_number)
        .bind(&item.check_type)
        .bind(&item.xml_body)
        .bind(&item.check_sign)
        .bind(&item.id_offline)
        .bind(&item.mac)
        .bind(item.status.as_str())
        .bind(&item.error)
        .bind(Self::naive(item.created_at))
        .bind(item.sent_at.map(Self::naive))
        .bind(store_id)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(item)
    }

    async fn get_queue_item(&self, item_id: Uuid) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items WHERE id = $1 AND store_id = $2"
        ))
        .bind(item_id)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroQueueItem::from))
    }

    async fn list_pending(&self, limit: u32) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let rows = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items \
             WHERE store_id = $1 AND \
                   status IN ('pending'::prro_queue_status, 'failed'::prro_queue_status) \
             ORDER BY (status = 'failed'::prro_queue_status) ASC, created_at ASC LIMIT $2"
        ))
        .bind(store_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(rows.into_iter().map(PrroQueueItem::from).collect())
    }

    async fn list_by_shift(&self, shift_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let rows = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items WHERE shift_id = $1 AND store_id = $2 \
             ORDER BY local_number ASC"
        ))
        .bind(shift_id)
        .bind(store_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(rows.into_iter().map(PrroQueueItem::from).collect())
    }

    async fn list_by_receipt(&self, receipt_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let rows = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items WHERE receipt_id = $1 AND store_id = $2 \
             ORDER BY created_at ASC"
        ))
        .bind(receipt_id)
        .bind(store_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(rows.into_iter().map(PrroQueueItem::from).collect())
    }

    async fn mark_sent(
        &self,
        item_id: Uuid,
        sent_at: Option<DateTime<Utc>>,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "UPDATE prro_queue_items SET status = 'sent'::prro_queue_status, \
             sent_at = COALESCE($2, now()), error = NULL \
             WHERE id = $1 AND store_id = $3 RETURNING {QUEUE_COLS}"
        ))
        .bind(item_id)
        .bind(sent_at.map(Self::naive))
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroQueueItem::from))
    }

    async fn mark_failed(
        &self,
        item_id: Uuid,
        error: String,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "UPDATE prro_queue_items SET status = 'failed'::prro_queue_status, error = $2 \
             WHERE id = $1 AND store_id = $3 RETURNING {QUEUE_COLS}"
        ))
        .bind(item_id)
        .bind(error)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroQueueItem::from))
    }

    async fn update_queue_check_sign(
        &self,
        item_id: Uuid,
        check_sign: String,
    ) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "UPDATE prro_queue_items SET check_sign = $2 \
             WHERE id = $1 AND store_id = $3 RETURNING {QUEUE_COLS}"
        ))
        .bind(item_id)
        .bind(check_sign)
        .bind(store_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroQueueItem::from))
    }

    async fn count_pending(&self) -> Result<u64, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM prro_queue_items \
             WHERE store_id = $1 AND status = 'pending'::prro_queue_status",
        )
        .bind(store_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(n as u64)
    }

    async fn delete_queue_item(&self, item_id: Uuid) -> Result<bool, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let res = sqlx::query("DELETE FROM prro_queue_items WHERE id = $1 AND store_id = $2")
            .bind(item_id)
            .bind(store_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn get_setting(&self, key: &str) -> Result<Option<String>, PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM prro_settings WHERE store_id = $1 AND key_name = $2")
                .bind(store_id)
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(Self::map_err)?;
        Ok(row.map(|r| r.0))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), PrroRepoError> {
        let store_id = self.ctx_store_id()?;
        sqlx::query(
            "INSERT INTO prro_settings (store_id, key_name, value, updated_at) \
             VALUES ($1, $2, $3, now()) \
             ON CONFLICT (store_id, key_name) \
             DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(store_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(())
    }

    // ── Фіскалізація (група 8/9) ───────────────────────────────────────────

    async fn load_receipt_with_items(
        &self,
        receipt_id: Uuid,
    ) -> Result<Option<torgashka_prro::prro::ReceiptFiscalRow>, PrroRepoError> {
        let rec: Option<ReceiptRow> = sqlx::query_as::<_, ReceiptRow>(
            "SELECT id, receipt_number, cashier_id, total_amount, paid_amount, change_amount, \
             debtor_id, is_return, notes, payment_method::text, cash_amount, card_amount, \
             original_receipt_id, return_reason, split_group_id, fiscal_status::text, \
             fiscal_number, fiscal_serial, fiscal_sent_at, fiscal_error, is_fiscal \
             FROM receipts WHERE id = $1",
        )
        .bind(receipt_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        let Some(rec) = rec else { return Ok(None) };
        let items: Vec<ReceiptItemRow> = sqlx::query_as::<_, ReceiptItemRow>(
            "SELECT id, product_id, quantity, price, total, purchase_price, \
             fiscal_quantity FROM receipt_items WHERE receipt_id = $1 ORDER BY created_at",
        )
        .bind(receipt_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(Some(torgashka_prro::prro::ReceiptFiscalRow {
            id: rec.id,
            receipt_number: rec.receipt_number,
            cashier_id: rec.cashier_id,
            total_amount: rec.total_amount,
            paid_amount: rec.paid_amount,
            change_amount: rec.change_amount,
            debtor_id: rec.debtor_id,
            is_return: rec.is_return,
            notes: rec.notes,
            payment_method: rec.payment_method,
            cash_amount: rec.cash_amount,
            card_amount: rec.card_amount,
            original_receipt_id: rec.original_receipt_id,
            return_reason: rec.return_reason,
            split_group_id: rec.split_group_id,
            fiscal_status: rec.fiscal_status,
            fiscal_number: rec.fiscal_number,
            fiscal_serial: rec.fiscal_serial,
            fiscal_sent_at: rec
                .fiscal_sent_at
                .map(|d| DateTime::from_naive_utc_and_offset(d, Utc)),
            fiscal_error: rec.fiscal_error,
            is_fiscal: rec.is_fiscal,
            items: items.into_iter().map(Into::into).collect(),
        }))
    }

    async fn load_product(
        &self,
        product_id: Uuid,
    ) -> Result<Option<torgashka_prro::prro::ProductFiscalRow>, PrroRepoError> {
        let row: Option<ProductRow> = sqlx::query_as::<_, ProductRow>(
            "SELECT id, title, fiscal_stock, tax_rate FROM products WHERE id = $1",
        )
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(|r| torgashka_prro::prro::ProductFiscalRow {
            id: r.id,
            title: r.title,
            fiscal_stock: r.fiscal_stock,
            tax_rate: r.tax_rate,
        }))
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
        let sent = fiscal_sent_at.map(Self::naive);
        sqlx::query(
            "UPDATE receipts SET fiscal_status = $2::fiscal_status, \
             fiscal_number = $3, fiscal_serial = $4, fiscal_sent_at = $5, \
             fiscal_error = $6, is_fiscal = COALESCE($7, is_fiscal) WHERE id = $1",
        )
        .bind(receipt_id)
        .bind(fiscal_status)
        .bind(fiscal_number)
        .bind(fiscal_serial)
        .bind(sent)
        .bind(fiscal_error)
        .bind(is_fiscal)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(())
    }

    async fn update_receipt_total(
        &self,
        receipt_id: Uuid,
        total_amount: Decimal,
    ) -> Result<(), PrroRepoError> {
        sqlx::query("UPDATE receipts SET total_amount = $2 WHERE id = $1")
            .bind(receipt_id)
            .bind(total_amount)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    async fn delete_non_fiscal_items(&self, receipt_id: Uuid) -> Result<u64, PrroRepoError> {
        let res =
            sqlx::query("DELETE FROM receipt_items WHERE receipt_id = $1 AND fiscal_quantity <= 0")
                .bind(receipt_id)
                .execute(&self.pool)
                .await
                .map_err(Self::map_err)?;
        Ok(res.rows_affected())
    }

    async fn update_item_fiscal(
        &self,
        item_id: Uuid,
        quantity: Decimal,
        total: Decimal,
        fiscal_quantity: Decimal,
    ) -> Result<(), PrroRepoError> {
        sqlx::query(
            "UPDATE receipt_items SET quantity = $2, total = $3, fiscal_quantity = $4 \
             WHERE id = $1",
        )
        .bind(item_id)
        .bind(quantity)
        .bind(total)
        .bind(fiscal_quantity)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(())
    }

    async fn create_receipt_duplicate(
        &self,
        source: &torgashka_prro::prro::ReceiptFiscalRow,
        items: &[torgashka_prro::prro::SplitItemInput],
        is_return: bool,
        split_group_id: Uuid,
    ) -> Result<Uuid, PrroRepoError> {
        let id = Uuid::new_v4();
        let receipt_number = format!(
            "NF-{}-{}",
            source.receipt_number.chars().take(20).collect::<String>(),
            &Uuid::new_v4().simple().to_string()[..6]
        );
        let total_amount: Decimal = items.iter().map(|i| i.total).sum();
        sqlx::query(
            "INSERT INTO receipts (id, receipt_number, receipt_type, cashier_id, total_amount, \
             paid_amount, change_amount, debtor_id, is_return, notes, payment_method, cash_amount, \
             card_amount, original_receipt_id, return_reason, split_group_id, is_fiscal, \
             fiscal_status, created_at) \
             VALUES ($1, $2, 'sale'::receipt_type, $3, $4, $5, $6, $7, $8, $9, \
             $10::receipt_payment_method, $11, $12, $13, $14, $15, false, 'none'::fiscal_status, now())",
        )
        .bind(id)
        .bind(&receipt_number)
        .bind(source.cashier_id)
        .bind(total_amount)
        .bind(source.paid_amount)
        .bind(source.change_amount)
        .bind(source.debtor_id)
        .bind(is_return)
        .bind(&source.notes)
        .bind(&source.payment_method)
        .bind(source.cash_amount)
        .bind(source.card_amount)
        .bind(source.original_receipt_id)
        .bind(&source.return_reason)
        .bind(split_group_id)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        for item in items {
            sqlx::query(
                "INSERT INTO receipt_items (id, receipt_id, product_id, quantity, price, total, \
                 purchase_price, fiscal_quantity, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 0, now())",
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(item.product_id)
            .bind(item.quantity)
            .bind(item.price)
            .bind(item.total)
            .bind(item.purchase_price)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        }
        Ok(id)
    }

    async fn update_product_fiscal_stock(
        &self,
        product_id: Uuid,
        new_stock: Decimal,
    ) -> Result<(), PrroRepoError> {
        sqlx::query("UPDATE products SET fiscal_stock = $2 WHERE id = $1")
            .bind(product_id)
            .bind(new_stock)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }
}

/// Рядок receipts для фіскалізації (група 8/9).
#[derive(sqlx::FromRow)]
struct ReceiptRow {
    id: Uuid,
    receipt_number: String,
    cashier_id: Uuid,
    total_amount: Decimal,
    paid_amount: Option<Decimal>,
    change_amount: Option<Decimal>,
    debtor_id: Option<Uuid>,
    is_return: bool,
    notes: Option<String>,
    payment_method: Option<String>,
    cash_amount: Option<Decimal>,
    card_amount: Option<Decimal>,
    original_receipt_id: Option<Uuid>,
    return_reason: Option<String>,
    split_group_id: Option<Uuid>,
    fiscal_status: String,
    fiscal_number: Option<String>,
    fiscal_serial: Option<String>,
    fiscal_sent_at: Option<NaiveDateTime>,
    fiscal_error: Option<String>,
    is_fiscal: bool,
}

/// Рядок receipt_items для фіскалізації.
#[derive(sqlx::FromRow)]
struct ReceiptItemRow {
    id: Uuid,
    product_id: Uuid,
    quantity: Decimal,
    price: Decimal,
    total: Decimal,
    purchase_price: Option<Decimal>,
    fiscal_quantity: Decimal,
}

impl From<ReceiptItemRow> for torgashka_prro::prro::ReceiptItemFiscalRow {
    fn from(r: ReceiptItemRow) -> Self {
        Self {
            id: r.id,
            product_id: r.product_id,
            quantity: r.quantity,
            price: r.price,
            total: r.total,
            purchase_price: r.purchase_price,
            fiscal_quantity: r.fiscal_quantity,
        }
    }
}

/// Рядок products для фіскалізації.
#[derive(sqlx::FromRow)]
struct ProductRow {
    id: Uuid,
    title: Option<String>,
    fiscal_stock: Decimal,
    tax_rate: Option<Decimal>,
}

/// Дозволяє використовувати sqlx-репозиторій як `PrroSetting`-джерело (не використовується
/// напряму — збережено для повноти 1:1 Python PrroSettingsRepository).
#[allow(dead_code)]
fn _setting_from_row(key: String, value: Option<String>) -> PrroSetting {
    PrroSetting {
        key_name: key,
        value,
    }
}
