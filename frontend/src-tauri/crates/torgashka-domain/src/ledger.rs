//! Ledger-порти (етап 4): журнал взаєморозрахунків з постачальниками.
//!
//! Контракт між application і infrastructure — 1:1 з Python-еталоном:
//!   - v1 (/api/v1/ledger) — LedgerService: POST "", GET /{supplier_id},
//!     GET /balance/{supplier_id} (без кешу)
//!   - v2 (/api/v2/ledger) — LedgerUseCases: GET/POST /entries,
//!     GET /balance/{supplier_id}, GET /balances (кеш TTL 30с у Python)
//!
//! ВАЖЛИВО: v2-шар Python має баги (POST /entries → 500 UnmappedInstanceError,
//! GET /entries → 500 ResponseValidationError при notes=NULL) — Rust реалізує
//! ЗАДУМАНУ робочу поведінку (фронтенд використовує v2). Differential проти
//! Python — на живих ендпойнтах (v1 повністю, v2 GET там, де Python 200).
//!
//! Decimal-поля v1 — рядки (Pydantic Decimal), v2 — float (Pydantic float).

use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

/// Помилки ledger-шару → HTTP 1:1 з Python.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// 404 Not Found (v1 POST/history/balance, v2 balance).
    #[error("{0}")]
    NotFound(String),
    /// 400 Bad Request (v1 невідомий тип, v2 create supplier/type).
    #[error("{0}")]
    BadRequest(String),
    /// 500 Internal Server Error (v2 entries невалідний operation_type → Python ValueError → 500).
    #[error("{0}")]
    InvalidOperationType(String),
    /// 500 Internal Server Error.
    #[error("помилка БД: {0}")]
    Infrastructure(String),
}

/// Blanket impl: Arc<dyn LedgerService> також є LedgerService (як для PosService).
#[async_trait::async_trait]
impl<T: LedgerService + ?Sized> LedgerService for std::sync::Arc<T> {
    async fn create_entry_v1(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV1Dto, LedgerError> {
        self.as_ref().create_entry_v1(input).await
    }
    async fn history_v1(
        &self,
        supplier_id: Uuid,
        page: i64,
        size: i64,
    ) -> Result<LedgerHistoryV1Dto, LedgerError> {
        self.as_ref().history_v1(supplier_id, page, size).await
    }
    async fn balance_v1(&self, supplier_id: Uuid) -> Result<LedgerBalanceV1Dto, LedgerError> {
        self.as_ref().balance_v1(supplier_id).await
    }
    async fn list_entries_v2(
        &self,
        q: &LedgerEntriesQuery,
    ) -> Result<LedgerListV2Dto, LedgerError> {
        self.as_ref().list_entries_v2(q).await
    }
    async fn create_entry_v2(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV2Dto, LedgerError> {
        self.as_ref().create_entry_v2(input).await
    }
    async fn balance_v2(&self, supplier_id: Uuid) -> Result<LedgerBalanceV2Dto, LedgerError> {
        self.as_ref().balance_v2(supplier_id).await
    }
    async fn all_balances_v2(&self) -> Result<Vec<SupplierBalanceV2Dto>, LedgerError> {
        self.as_ref().all_balances_v2().await
    }
}

/// Контракт ledger-операцій (етап 4).
#[async_trait::async_trait]
pub trait LedgerService: Send + Sync {
    // ─── v1 (LedgerService Python) ──────────────────────────────────────────
    /// POST /api/v1/ledger — створення запису (require_admin).
    async fn create_entry_v1(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV1Dto, LedgerError>;
    /// GET /api/v1/ledger/{supplier_id} — історія постачальника (пагінація).
    async fn history_v1(
        &self,
        supplier_id: Uuid,
        page: i64,
        size: i64,
    ) -> Result<LedgerHistoryV1Dto, LedgerError>;
    /// GET /api/v1/ledger/balance/{supplier_id} — баланс (сума amount).
    async fn balance_v1(&self, supplier_id: Uuid) -> Result<LedgerBalanceV1Dto, LedgerError>;

    // ─── v2 (LedgerUseCases Python) ─────────────────────────────────────────
    /// GET /api/v2/ledger/entries — фільтри + пагінація.
    async fn list_entries_v2(&self, q: &LedgerEntriesQuery)
        -> Result<LedgerListV2Dto, LedgerError>;
    /// POST /api/v2/ledger/entries — створення запису.
    async fn create_entry_v2(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV2Dto, LedgerError>;
    /// GET /api/v2/ledger/balance/{supplier_id} — останній balance_after.
    async fn balance_v2(&self, supplier_id: Uuid) -> Result<LedgerBalanceV2Dto, LedgerError>;
    /// GET /api/v2/ledger/balances — всі постачальники (LEFT JOIN max date).
    async fn all_balances_v2(&self) -> Result<Vec<SupplierBalanceV2Dto>, LedgerError>;
}

/// Вхідні дані створення ledger-запису (v1 + v2).
#[derive(Debug, Clone)]
pub struct LedgerEntryInput {
    pub supplier_id: Uuid,
    /// v1: Decimal-рядок з вхідною scale (як Pydantic Decimal); v2: "123.45".
    pub amount: String,
    pub operation_type: String,
    pub document_id: Option<Uuid>,
    pub document_number: Option<String>,
    /// v1: обов'язкове; v2: ігнорується (Python v2 не має operation_date → now).
    pub operation_date: Option<NaiveDateTime>,
    pub notes: Option<String>,
}

/// Фільтри GET /api/v2/ledger/entries.
#[derive(Debug, Clone, Default)]
pub struct LedgerEntriesQuery {
    pub page: i64,
    pub size: i64,
    pub supplier_id: Option<Uuid>,
    pub operation_type: Option<String>,
    pub date_from: Option<NaiveDateTime>,
    pub date_to: Option<NaiveDateTime>,
}

// ─── v1 DTO ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct LedgerEntryV1Dto {
    pub id: Uuid,
    pub supplier_id: Uuid,
    pub operation_type: String,
    pub document_id: Option<Uuid>,
    pub document_number: Option<String>,
    /// Decimal-рядок (Pydantic Decimal): create — вхідна scale, GET — scale БД "100.50".
    pub amount: String,
    pub balance_after: String,
    pub operation_date: NaiveDateTime,
    pub notes: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerHistoryV1Dto {
    pub items: Vec<LedgerEntryV1Dto>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerBalanceV1Dto {
    pub supplier_id: Uuid,
    pub supplier_name: String,
    /// Decimal-рядок (сума amount усіх операцій).
    pub current_balance: String,
    pub last_updated: Option<NaiveDateTime>,
}

// ─── v2 DTO ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct LedgerEntryV2Dto {
    pub id: Uuid,
    pub supplier_id: Uuid,
    /// float (Pydantic float) — amount з БД numeric(12,2).
    pub amount: f64,
    pub operation_type: String,
    pub balance_after: Option<f64>,
    pub created_at: Option<NaiveDateTime>,
    pub document_id: Option<Uuid>,
    pub document_number: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerListV2Dto {
    pub items: Vec<LedgerEntryV2Dto>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerBalanceV2Dto {
    pub supplier_id: Uuid,
    pub balance: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupplierBalanceV2Dto {
    pub supplier_id: Uuid,
    pub supplier_name: Option<String>,
    pub balance: f64,
    pub last_operation_date: Option<NaiveDateTime>,
}
