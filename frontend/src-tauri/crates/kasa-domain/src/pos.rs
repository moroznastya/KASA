//! POS-порти (етап 3): чеки v2, робочі сесії, списання, переміщення, зміни ПРРО.
//!
//! Контракт між application і infrastructure — 1:1 з Python-еталоном:
//!   - receipts v2 (POST /sale|/return, GET list/detail/items/stats/search/
//!     by-product/returnable-quantity) — ReceiptUseCases + v2/receipts.py
//!   - work-sessions (/my, /report, /user/{id}) — v1/work_sessions.py
//!   - write-offs (CRUD + confirm, авто-confirm при create) — v1/write_offs.py
//!   - transfers (CRUD + confirm/cancel, тільки чернетки редагуються)
//!   - prro shifts (list; open/close — ПРРО недоступний → 400 як Python)
//!
//! Decimal-поля передаються рядками (як у JSON Python-еталону). Create-
//! відповіді зберігають ВХІДНУ scale (identity map Python: `"1"` ≠ `"1.000"`),
//! GET/confirm — scale колонки (::text): `"1.000"`, `"0.00"`.

use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

/// Помилки POS-шару → HTTP 1:1 з Python.
#[derive(Debug, thiserror::Error)]
pub enum PosError {
    /// 404 Not Found.
    #[error("{0}")]
    NotFound(String),
    /// 400 Bad Request (ValueError Python).
    #[error("{0}")]
    BadRequest(String),
    /// 422 Unprocessable Entity (ReceiptValidationError / Pydantic).
    #[error("{0}")]
    Validation(String),
    /// 403 Forbidden (роль).
    #[error("{0}")]
    Forbidden(String),
    /// 500 Internal Server Error.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

/// Контракт POS-операцій (етап 3).
#[async_trait::async_trait]
pub trait PosService: Send + Sync {
    // ─── Чеки v2 ────────────────────────────────────────────────────────────
    async fn create_sale_receipt(&self, input: &ReceiptCreateInput)
        -> Result<ReceiptDto, PosError>;
    async fn create_return_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError>;
    async fn get_receipt(&self, id: Uuid) -> Result<ReceiptDto, PosError>;
    async fn list_receipts(&self, q: &ReceiptListQuery) -> Result<ReceiptListDto, PosError>;
    async fn today_stats(&self) -> Result<ReceiptStatsDto, PosError>;
    async fn search_receipts(&self, q: &ReceiptSearchQuery) -> Result<ReceiptSearchDto, PosError>;
    async fn recent_sales_by_product(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ProductRecentSalesDto>, PosError>;
    async fn returnable_quantity(&self, product_id: Uuid) -> Result<ReturnableQtyDto, PosError>;
    async fn receipt_items(&self, receipt_id: Uuid) -> Result<Vec<ReceiptItemDetailDto>, PosError>;

    // ─── Робочі сесії ───────────────────────────────────────────────────────
    async fn my_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<MySessionsDto, PosError>;
    async fn work_report(&self, month: i64, year: i64) -> Result<WorkReportDto, PosError>;
    async fn user_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<UserSessionsDto, PosError>;

    // ─── Списання ───────────────────────────────────────────────────────────
    async fn list_write_offs(&self, page: i64, size: i64) -> Result<WriteOffListDto, PosError>;
    async fn get_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError>;
    async fn create_write_off(&self, input: &WriteOffCreateInput) -> Result<WriteOffDto, PosError>;
    async fn update_write_off(
        &self,
        id: Uuid,
        input: &WriteOffUpdateInput,
    ) -> Result<WriteOffDto, PosError>;
    async fn delete_write_off(&self, id: Uuid) -> Result<(), PosError>;
    async fn confirm_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError>;

    // ─── Переміщення ────────────────────────────────────────────────────────
    async fn list_transfers(&self, page: i64, size: i64) -> Result<TransferListDto, PosError>;
    async fn get_transfer(&self, id: Uuid) -> Result<TransferDto, PosError>;
    async fn create_transfer(&self, input: &TransferCreateInput) -> Result<TransferDto, PosError>;
    async fn update_transfer(
        &self,
        id: Uuid,
        input: &TransferUpdateInput,
    ) -> Result<TransferDto, PosError>;
    async fn delete_transfer(&self, id: Uuid) -> Result<(), PosError>;
    async fn confirm_transfer(&self, id: Uuid, status: &str) -> Result<TransferDto, PosError>;

    // ─── Зміни ПРРО (X/Z) ───────────────────────────────────────────────────
    async fn list_shifts(&self, page: i64, size: i64) -> Result<ShiftListDto, PosError>;
    async fn open_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError>;
    async fn close_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError>;
}

/// Blanket: `Arc<T>` делегує [`PosService`].
#[async_trait::async_trait]
impl<T: PosService + ?Sized> PosService for std::sync::Arc<T> {
    async fn create_sale_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        (**self).create_sale_receipt(input).await
    }
    async fn create_return_receipt(
        &self,
        input: &ReceiptCreateInput,
    ) -> Result<ReceiptDto, PosError> {
        (**self).create_return_receipt(input).await
    }
    async fn get_receipt(&self, id: Uuid) -> Result<ReceiptDto, PosError> {
        (**self).get_receipt(id).await
    }
    async fn list_receipts(&self, q: &ReceiptListQuery) -> Result<ReceiptListDto, PosError> {
        (**self).list_receipts(q).await
    }
    async fn today_stats(&self) -> Result<ReceiptStatsDto, PosError> {
        (**self).today_stats().await
    }
    async fn search_receipts(&self, q: &ReceiptSearchQuery) -> Result<ReceiptSearchDto, PosError> {
        (**self).search_receipts(q).await
    }
    async fn recent_sales_by_product(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ProductRecentSalesDto>, PosError> {
        (**self).recent_sales_by_product(query, limit).await
    }
    async fn returnable_quantity(&self, product_id: Uuid) -> Result<ReturnableQtyDto, PosError> {
        (**self).returnable_quantity(product_id).await
    }
    async fn receipt_items(&self, receipt_id: Uuid) -> Result<Vec<ReceiptItemDetailDto>, PosError> {
        (**self).receipt_items(receipt_id).await
    }
    async fn my_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<MySessionsDto, PosError> {
        (**self).my_sessions(user_id, month, year).await
    }
    async fn work_report(&self, month: i64, year: i64) -> Result<WorkReportDto, PosError> {
        (**self).work_report(month, year).await
    }
    async fn user_sessions(
        &self,
        user_id: Uuid,
        month: i64,
        year: i64,
    ) -> Result<UserSessionsDto, PosError> {
        (**self).user_sessions(user_id, month, year).await
    }
    async fn list_write_offs(&self, page: i64, size: i64) -> Result<WriteOffListDto, PosError> {
        (**self).list_write_offs(page, size).await
    }
    async fn get_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        (**self).get_write_off(id).await
    }
    async fn create_write_off(&self, input: &WriteOffCreateInput) -> Result<WriteOffDto, PosError> {
        (**self).create_write_off(input).await
    }
    async fn update_write_off(
        &self,
        id: Uuid,
        input: &WriteOffUpdateInput,
    ) -> Result<WriteOffDto, PosError> {
        (**self).update_write_off(id, input).await
    }
    async fn delete_write_off(&self, id: Uuid) -> Result<(), PosError> {
        (**self).delete_write_off(id).await
    }
    async fn confirm_write_off(&self, id: Uuid) -> Result<WriteOffDto, PosError> {
        (**self).confirm_write_off(id).await
    }
    async fn list_transfers(&self, page: i64, size: i64) -> Result<TransferListDto, PosError> {
        (**self).list_transfers(page, size).await
    }
    async fn get_transfer(&self, id: Uuid) -> Result<TransferDto, PosError> {
        (**self).get_transfer(id).await
    }
    async fn create_transfer(&self, input: &TransferCreateInput) -> Result<TransferDto, PosError> {
        (**self).create_transfer(input).await
    }
    async fn update_transfer(
        &self,
        id: Uuid,
        input: &TransferUpdateInput,
    ) -> Result<TransferDto, PosError> {
        (**self).update_transfer(id, input).await
    }
    async fn delete_transfer(&self, id: Uuid) -> Result<(), PosError> {
        (**self).delete_transfer(id).await
    }
    async fn confirm_transfer(&self, id: Uuid, status: &str) -> Result<TransferDto, PosError> {
        (**self).confirm_transfer(id, status).await
    }
    async fn list_shifts(&self, page: i64, size: i64) -> Result<ShiftListDto, PosError> {
        (**self).list_shifts(page, size).await
    }
    async fn open_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        (**self).open_shift(comment).await
    }
    async fn close_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PosError> {
        (**self).close_shift(comment).await
    }
}

// ─── Чеки v2: вхідні структури ─────────────────────────────────────────────

/// Позиція чеку (v2 CreateReceiptRequest.items[]).
#[derive(Debug, Clone)]
pub struct ReceiptItemInput {
    pub product_id: Uuid,
    pub name: String,
    /// Decimal рядком (вхідна scale зберігається у відповіді).
    pub quantity: String,
    pub price: String,
    pub tax_rate: i64,
}

/// POST /api/v2/receipts/sale|return (CreateReceiptRequest).
#[derive(Debug, Clone)]
pub struct ReceiptCreateInput {
    pub items: Vec<ReceiptItemInput>,
    pub payment_method: String,
    pub cash_amount: Option<String>,
    pub card_amount: Option<String>,
    pub customer_id: Option<Uuid>,
    /// sub з JWT (Python: request.scope["user_id"]).
    pub cashier_id: Option<Uuid>,
    pub notes: String,
    pub terminal_rrn: Option<String>,
    pub terminal_approval_code: Option<String>,
    pub terminal_invoice_number: Option<String>,
    pub terminal_transaction_id: Option<String>,
    pub terminal_response_code: Option<String>,
    pub terminal_status: Option<String>,
    pub terminal_receipt: Option<String>,
    pub terminal_card_pan: Option<String>,
    pub terminal_payment_system: Option<String>,
    pub terminal_merchant: Option<String>,
    pub terminal_created_at: Option<NaiveDateTime>,
    pub is_fiscal: bool,
    pub split_group_id: Option<Uuid>,
}

/// GET /api/v2/receipts?page=&size=&search=&date_from=&date_to=&payment_method=
#[derive(Debug, Clone, Default)]
pub struct ReceiptListQuery {
    pub page: i64,
    pub size: i64,
    pub search: Option<String>,
    pub date_from: Option<NaiveDateTime>,
    pub date_to: Option<NaiveDateTime>,
    pub payment_method: Option<String>,
}

/// GET /api/v2/receipts/search?q=&date_from=&date_to=&receipt_type=&page=&size=
#[derive(Debug, Clone, Default)]
pub struct ReceiptSearchQuery {
    pub q: String,
    pub date_from: Option<NaiveDateTime>,
    pub date_to: Option<NaiveDateTime>,
    /// "sale" (default) | "return" | None
    pub receipt_type: Option<String>,
    pub page: i64,
    pub size: i64,
}

// ─── Чеки v2: DTO відповідей ───────────────────────────────────────────────

/// Позиція чеку у відповіді (v2 ReceiptItemResponse).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptItemDto {
    pub product_id: Uuid,
    /// Python ORM-шлях: завжди "" (ORM не зберігає name).
    pub name: String,
    pub quantity: f64,
    pub price: f64,
    pub tax_rate: i64,
}

/// Повний чек (v2 ReceiptResponse) — поля 1:1 з Python.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptDto {
    pub id: Uuid,
    pub number: String,
    pub items: Vec<ReceiptItemDto>,
    pub total: Option<f64>,
    pub payment_method: String,
    pub created_at: Option<String>,
    pub cash_amount: Option<f64>,
    pub card_amount: Option<f64>,
    pub change_amount: Option<f64>,
    pub customer_id: Option<Uuid>,
    pub notes: String,
    pub is_fiscal: bool,
    pub fiscal_status: String,
    pub fiscal_number: Option<String>,
    pub fiscal_serial: Option<String>,
    pub fiscal_sent_at: Option<String>,
    pub fiscal_error: Option<String>,
    pub split_group_id: Option<Uuid>,
    pub terminal_rrn: Option<String>,
    pub terminal_approval_code: Option<String>,
    pub terminal_invoice_number: Option<String>,
    pub terminal_transaction_id: Option<String>,
    pub terminal_response_code: Option<String>,
    pub terminal_status: Option<String>,
    pub terminal_receipt: Option<String>,
    pub terminal_card_pan: Option<String>,
    pub terminal_payment_system: Option<String>,
    pub terminal_merchant: Option<String>,
    pub terminal_created_at: Option<String>,
    pub fiscal_check_url: Option<String>,
}

/// GET /api/v2/receipts → {items, total, page, size}
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptListDto {
    pub items: Vec<ReceiptDto>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

/// GET /api/v2/receipts/stats/today
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptStatsDto {
    pub total_sales: f64,
    pub total_returns: f64,
    pub total_profit: f64,
    pub total_vat: f64,
    pub receipts_count: i64,
    pub items_sold: i64,
    pub date: String,
}

/// Елемент пошуку (ReceiptSearchItem).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptSearchItemDto {
    pub id: Uuid,
    pub receipt_number: String,
    pub receipt_type: String,
    pub total_amount: f64,
    pub created_at: Option<String>,
    pub cashier_name: String,
    pub items_count: i64,
}

/// GET /api/v2/receipts/search → {items, total, page, page_size, pages}
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptSearchDto {
    pub items: Vec<ReceiptSearchItemDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

/// Останній продаж (RecentSaleInfo).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RecentSaleDto {
    pub receipt_id: Uuid,
    pub receipt_number: String,
    pub created_at: Option<String>,
    pub quantity: f64,
    pub price: f64,
}

/// Товар (ProductBriefInfo).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductBriefInfoDto {
    pub id: Uuid,
    pub title: String,
    pub barcode: Option<String>,
    pub price: Option<f64>,
    pub unit: Option<String>,
}

/// Елемент recent-sales.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductRecentSalesDto {
    pub product: ProductBriefInfoDto,
    pub total_sold: f64,
    pub total_returned: f64,
    pub returnable: f64,
    pub recent_sales: Vec<RecentSaleDto>,
}

/// GET /api/v2/receipts/products/{id}/returnable-quantity
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReturnableQtyDto {
    pub product_id: String,
    pub total_sold: f64,
    pub total_returned: f64,
    pub returnable: f64,
}

/// Позиція чеку для повернення (GET /{id}/items).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReceiptItemDetailDto {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub product_barcode: Option<String>,
    pub quantity: f64,
    pub price: f64,
    pub total: f64,
    pub purchase_price: Option<f64>,
    pub created_at: Option<String>,
}

// ─── Робочі сесії ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkSessionDto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub login_time: String,
    pub logout_time: Option<String>,
    pub duration_hours: Option<f64>,
    pub is_active: bool,
}

/// GET /api/v1/work-sessions/my
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MySessionsDto {
    pub sessions: Vec<WorkSessionDto>,
    pub total_hours: f64,
    pub hourly_rate: Option<f64>,
}

/// Підсумок користувача (UserHoursSummary).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UserHoursSummaryDto {
    pub user_id: Uuid,
    pub user_name: String,
    pub total_hours: f64,
    pub hourly_rate: Option<f64>,
    pub salary: Option<f64>,
}

/// GET /api/v1/work-sessions/report
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkReportDto {
    pub month: i64,
    pub year: i64,
    pub items: Vec<UserHoursSummaryDto>,
}

/// GET /api/v1/work-sessions/user/{id}
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UserSessionsDto {
    pub user_id: Uuid,
    pub user_name: String,
    pub total_hours: f64,
    pub sessions: Vec<WorkSessionDto>,
}

// ─── Списання ──────────────────────────────────────────────────────────────

/// Позиція документа (WriteOffItemCreate / TransferItemCreate).
#[derive(Debug, Clone)]
pub struct DocItemInput {
    pub product_id: Uuid,
    pub quantity: String,
    pub cost_price: Option<String>,
    pub price: Option<String>,
}

/// POST /api/v1/write-offs (WriteOffCreate).
#[derive(Debug, Clone)]
pub struct WriteOffCreateInput {
    pub number: Option<String>,
    pub reason: String,
    pub write_off_date: NaiveDateTime,
    pub notes: Option<String>,
    pub created_by: Uuid,
    pub items: Vec<DocItemInput>,
}

/// PUT /api/v1/write-offs/{id} (WriteOffUpdate).
#[derive(Debug, Clone, Default)]
pub struct WriteOffUpdateInput {
    pub number: Option<Option<String>>,
    pub reason: Option<String>,
    pub write_off_date: Option<NaiveDateTime>,
    pub notes: Option<Option<String>>,
    pub items: Option<Vec<DocItemInput>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WriteOffItemDto {
    pub id: Uuid,
    pub write_off_id: Uuid,
    pub product_id: Uuid,
    pub quantity: String,
    pub cost_price: String,
    pub price: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WriteOffDto {
    pub id: Uuid,
    pub number: String,
    pub reason: String,
    pub write_off_date: String,
    pub notes: Option<String>,
    pub status: String,
    pub total_amount: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub items: Vec<WriteOffItemDto>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WriteOffListDto {
    pub items: Vec<WriteOffDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

// ─── Переміщення ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TransferCreateInput {
    pub number: Option<String>,
    pub from_location: String,
    pub to_location: String,
    pub transfer_date: NaiveDateTime,
    pub notes: Option<String>,
    pub created_by: Uuid,
    pub items: Vec<DocItemInput>,
}

#[derive(Debug, Clone, Default)]
pub struct TransferUpdateInput {
    pub number: Option<Option<String>>,
    pub from_location: Option<String>,
    pub to_location: Option<String>,
    pub transfer_date: Option<NaiveDateTime>,
    pub notes: Option<Option<String>>,
    pub items: Option<Vec<DocItemInput>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TransferItemDto {
    pub id: Uuid,
    pub transfer_id: Uuid,
    pub product_id: Uuid,
    pub quantity: String,
    pub cost_price: String,
    pub price: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TransferDto {
    pub id: Uuid,
    pub number: String,
    pub from_location: String,
    pub to_location: String,
    pub transfer_date: String,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub items: Vec<TransferItemDto>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TransferListDto {
    pub items: Vec<TransferDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub pages: i64,
}

// ─── Зміни ПРРО ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PrroShiftDto {
    pub id: Uuid,
    pub shift_number: i64,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub signer_name: Option<String>,
    pub status: String,
    pub receipt_count: i64,
    pub total_amount: String,
    pub zreport_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ShiftListDto {
    pub items: Vec<PrroShiftDto>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

// ─── Утиліти Decimal (scale-обізнані, як Python Decimal) ──────────────────

/// Парсить десятковий рядок у ціле зі scale 3 (Numeric(10,3)).
/// "1" → 1000, "1.5" → 1500, "-2.5" → -2500. None — невалідний.
pub fn parse_scaled3(s: &str) -> Option<i64> {
    let (neg, rest) = match s.trim().strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.trim()),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac_part.len() > 3 || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let int_val: i64 = int_part.parse().ok()?;
    let frac_val: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().ok()? * 100,
        2 => frac_part.parse::<i64>().ok()? * 10,
        _ => frac_part.parse().ok()?,
    };
    let v = int_val * 1000 + frac_val;
    Some(if neg { -v } else { v })
}

/// Парсить десятковий рядок у ціле зі scale 2 (Numeric(12,2)).
pub fn parse_scaled2(s: &str) -> Option<i64> {
    let (neg, rest) = match s.trim().strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.trim()),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac_part.len() > 2 || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let int_val: i64 = int_part.parse().ok()?;
    let frac_val: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().ok()? * 10,
        _ => frac_part.parse().ok()?,
    };
    let v = int_val * 100 + frac_val;
    Some(if neg { -v } else { v })
}

/// total чеку = сума quantity*price (Decimal-точність Python).
/// Повертає f64 через ділення scaled5 (quantity scale3 × price scale2).
pub fn receipt_total(items: &[ReceiptItemInput]) -> f64 {
    let mut sum: i128 = 0;
    for it in items {
        if let (Some(q), Some(p)) = (parse_scaled3(&it.quantity), parse_scaled2(&it.price)) {
            sum += (q as i128) * (p as i128);
        }
    }
    sum as f64 / 100_000.0
}

/// ISO-форматування NaiveDateTime (Python isoformat без TZ):
/// "2026-08-07T14:11:56" або "2026-08-07T14:11:56.830629" (6 цифр мікросекунд).
pub fn iso_naive(dt: NaiveDateTime) -> String {
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = dt.and_utc().timestamp_subsec_micros();
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}

/// ISO-форматування з маркером UTC 'Z' (як WorkSessionResponse серіалізатор).
pub fn iso_utc_z(dt: NaiveDateTime) -> String {
    format!("{}Z", iso_naive(dt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_parsing() {
        assert_eq!(parse_scaled3("1"), Some(1000));
        assert_eq!(parse_scaled3("1.5"), Some(1500));
        assert_eq!(parse_scaled3("2.5"), Some(2500));
        assert_eq!(parse_scaled2("100"), Some(10000));
        assert_eq!(parse_scaled2("142.7"), Some(14270));
        assert_eq!(parse_scaled3("-3"), Some(-3000));
    }

    #[test]
    fn total_computation() {
        let items = vec![ReceiptItemInput {
            product_id: Uuid::nil(),
            name: String::new(),
            quantity: "2".to_string(),
            price: "100".to_string(),
            tax_rate: 20,
        }];
        assert_eq!(receipt_total(&items), 200.0);
        let items = vec![
            ReceiptItemInput {
                product_id: Uuid::nil(),
                name: String::new(),
                quantity: "0.333".to_string(),
                price: "100".to_string(),
                tax_rate: 20,
            },
            ReceiptItemInput {
                product_id: Uuid::nil(),
                name: String::new(),
                quantity: "1".to_string(),
                price: "50.5".to_string(),
                tax_rate: 20,
            },
        ];
        // 33.3 + 50.5 = 83.8
        assert_eq!(receipt_total(&items), 83.8);
    }
}
