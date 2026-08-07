//! SqlxPrroRepository — PostgreSQL-реалізація `kasa_prro::prro::PrroRepository`.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use kasa_prro::prro::{
    PrroQueueItem, PrroQueueStatus, PrroRepoError, PrroRepository, PrroSetting, PrroShift,
    PrroShiftStatus,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
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
    pool: PgPool,
}

impl SqlxPrroRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Створює репозиторій і гарантує наявність схеми (ідемпотентно).
    pub async fn connect(pool: PgPool) -> Result<Self, sqlx::Error> {
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
}

/// SELECT: статус кастується в text (для FromRow String).
const SHIFT_COLS: &str = "id, shift_number, opened_at, closed_at, signer_serial, signer_name, \
     closed_by, zreport_number, status::text, receipt_count, total_amount, last_local_number, last_mac";
const QUEUE_COLS: &str = "id, receipt_id, shift_id, local_number, check_type, xml_body, mac, \
     status::text, error, created_at, sent_at";
/// INSERT: без касту — значення передаються параметрами ($n::enum).
const SHIFT_INSERT_COLS: &str =
    "id, shift_number, opened_at, closed_at, signer_serial, signer_name, \
     closed_by, zreport_number, status, receipt_count, total_amount, last_local_number, last_mac";
const QUEUE_INSERT_COLS: &str =
    "id, receipt_id, shift_id, local_number, check_type, xml_body, mac, \
     status, error, created_at, sent_at";

#[async_trait]
impl PrroRepository for SqlxPrroRepository {
    async fn create_shift(&self, shift: PrroShift) -> Result<PrroShift, PrroRepoError> {
        sqlx::query(&format!(
            "INSERT INTO prro_shifts ({SHIFT_INSERT_COLS}) VALUES \
             ($1,$2,$3,$4,$5,$6,$7,$8,$9::prro_shift_status,$10,$11,$12,$13)"
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
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(shift)
    }

    async fn get_shift(&self, shift_id: Uuid) -> Result<Option<PrroShift>, PrroRepoError> {
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE id = $1"
        ))
        .bind(shift_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn get_shift_by_number(
        &self,
        shift_number: i64,
    ) -> Result<Option<PrroShift>, PrroRepoError> {
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE shift_number = $1"
        ))
        .bind(shift_number)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn get_open_shift(&self) -> Result<Option<PrroShift>, PrroRepoError> {
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts WHERE status = 'open'::prro_shift_status \
             ORDER BY opened_at DESC LIMIT 1"
        ))
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
        let total: (i64,) = sqlx::query_as("SELECT count(*) FROM prro_shifts")
            .fetch_one(&self.pool)
            .await
            .map_err(Self::map_err)?;
        let offset = (page.saturating_sub(1) * size) as i64;
        let rows = sqlx::query_as::<_, ShiftRow>(&format!(
            "SELECT {SHIFT_COLS} FROM prro_shifts ORDER BY shift_number DESC \
             LIMIT $1 OFFSET $2"
        ))
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
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "UPDATE prro_shifts SET status = 'closed'::prro_shift_status, closed_at = $2, \
             closed_by = $3, zreport_number = $4, \
             signer_serial = COALESCE($5, signer_serial), signer_name = COALESCE($6, signer_name) \
             WHERE id = $1 RETURNING {SHIFT_COLS}"
        ))
        .bind(shift_id)
        .bind(Self::naive(closed_at))
        .bind(closed_by)
        .bind(zreport_number)
        .bind(signer_serial)
        .bind(signer_name)
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
        let row = sqlx::query_as::<_, ShiftRow>(&format!(
            "UPDATE prro_shifts SET receipt_count = receipt_count + 1, \
             total_amount = total_amount + $2, \
             last_local_number = COALESCE($3, last_local_number), \
             last_mac = COALESCE($4, last_mac) \
             WHERE id = $1 RETURNING {SHIFT_COLS}"
        ))
        .bind(shift_id)
        .bind(amount)
        .bind(last_local_number)
        .bind(last_mac)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroShift::from))
    }

    async fn add_to_queue(&self, item: PrroQueueItem) -> Result<PrroQueueItem, PrroRepoError> {
        sqlx::query(&format!(
            "INSERT INTO prro_queue_items ({QUEUE_INSERT_COLS}) VALUES \
             ($1,$2,$3,$4,$5,$6,$7,$8::prro_queue_status,$9,$10,$11)"
        ))
        .bind(item.id)
        .bind(item.receipt_id)
        .bind(item.shift_id)
        .bind(item.local_number)
        .bind(&item.check_type)
        .bind(&item.xml_body)
        .bind(&item.mac)
        .bind(item.status.as_str())
        .bind(&item.error)
        .bind(Self::naive(item.created_at))
        .bind(item.sent_at.map(Self::naive))
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(item)
    }

    async fn get_queue_item(&self, item_id: Uuid) -> Result<Option<PrroQueueItem>, PrroRepoError> {
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items WHERE id = $1"
        ))
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroQueueItem::from))
    }

    async fn list_pending(&self, limit: u32) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let rows = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items \
             WHERE status IN ('pending'::prro_queue_status, 'failed'::prro_queue_status) \
             ORDER BY (status = 'failed'::prro_queue_status) ASC, created_at ASC LIMIT $1"
        ))
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(rows.into_iter().map(PrroQueueItem::from).collect())
    }

    async fn list_by_shift(&self, shift_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let rows = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items WHERE shift_id = $1 ORDER BY local_number ASC"
        ))
        .bind(shift_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(rows.into_iter().map(PrroQueueItem::from).collect())
    }

    async fn list_by_receipt(&self, receipt_id: Uuid) -> Result<Vec<PrroQueueItem>, PrroRepoError> {
        let rows = sqlx::query_as::<_, QueueRow>(&format!(
            "SELECT {QUEUE_COLS} FROM prro_queue_items WHERE receipt_id = $1 ORDER BY created_at ASC"
        ))
        .bind(receipt_id)
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
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "UPDATE prro_queue_items SET status = 'sent'::prro_queue_status, \
             sent_at = COALESCE($2, now()), error = NULL WHERE id = $1 RETURNING {QUEUE_COLS}"
        ))
        .bind(item_id)
        .bind(sent_at.map(Self::naive))
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
        let row = sqlx::query_as::<_, QueueRow>(&format!(
            "UPDATE prro_queue_items SET status = 'failed'::prro_queue_status, error = $2 \
             WHERE id = $1 RETURNING {QUEUE_COLS}"
        ))
        .bind(item_id)
        .bind(error)
        .fetch_optional(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(row.map(PrroQueueItem::from))
    }

    async fn count_pending(&self) -> Result<u64, PrroRepoError> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM prro_queue_items WHERE status = 'pending'::prro_queue_status",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(n as u64)
    }

    async fn delete_queue_item(&self, item_id: Uuid) -> Result<bool, PrroRepoError> {
        let res = sqlx::query("DELETE FROM prro_queue_items WHERE id = $1")
            .bind(item_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn get_setting(&self, key: &str) -> Result<Option<String>, PrroRepoError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM prro_settings WHERE key_name = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(Self::map_err)?;
        Ok(row.map(|r| r.0))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), PrroRepoError> {
        sqlx::query(
            "INSERT INTO prro_settings (key_name, value, updated_at) VALUES ($1, $2, now()) \
             ON CONFLICT (key_name) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(())
    }
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
