//! Ledger-репозиторії (етап 4 — журнал взаєморозрахунків).
//!
//! Реалізують [`LedgerService`] на sqlx/PostgreSQL — 1:1 з Python-еталоном:
//!   - v1 LedgerService (app/domain/services/ledger_service.py): POST "",
//!     GET /{supplier_id} (ORDER BY operation_date DESC), GET /balance
//!     (баланс = SUM(amount))
//!   - v2 LedgerUseCases (app/application/use_cases/ledger_use_cases.py):
//!     GET/POST /entries (ORDER BY operation_date DESC, created_at DESC,
//!     id DESC), GET /balance (останній balance_after), GET /balances
//!     (LEFT JOIN max(operation_date) — дублікати як у Python)
//!
//! ВАЖЛИВО: v2 Python зламаний (POST → UnmappedInstanceError 500, GET entries
//! → ResponseValidationError 500 при notes=NULL) — тут реалізовано ЗАДУМАНУ
//! робочу поведінку; differential-звірка — на живих ендпойнтах Python.
//!
//! Decimal v1 — рядки зі scale (Pydantic Decimal); create-відповідь зберігає
//! ВХІДНУ scale amount (identity map Python), GET — scale БД numeric(12,2).
//! v2 — float (Pydantic float).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::str::FromStr;
use uuid::Uuid;

use kasa_domain::{
    iso_naive, LedgerBalanceV1Dto, LedgerBalanceV2Dto, LedgerEntriesQuery, LedgerEntryInput,
    LedgerEntryV1Dto, LedgerEntryV2Dto, LedgerError, LedgerHistoryV1Dto, LedgerListV2Dto,
    LedgerService, SupplierBalanceV2Dto,
};

/// Локальний екстеншен: sqlx::Error → LedgerError.
trait SqlxResultExt<T> {
    fn le(self) -> Result<T, LedgerError>;
}

impl<T> SqlxResultExt<T> for Result<T, sqlx::Error> {
    fn le(self) -> Result<T, LedgerError> {
        self.map_err(|e| LedgerError::Infrastructure(e.to_string()))
    }
}

/// SQL-реалізація ledger-операцій.
#[derive(Clone)]
pub struct SqlxLedger {
    pool: PgPool,
}

impl SqlxLedger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Типи операцій БД (ledger_operation_type) — Python ORM має 4 значення.
const LEDGER_TYPES_V1: [&str; 4] = ["invoice", "payment", "return", "correction"];
/// Domain OperationType Python (v2) — 5 значень.
const LEDGER_TYPES_V2: [&str; 5] = ["invoice", "payment", "return", "correction", "write_off"];

fn not_found_supplier(id: &Uuid) -> LedgerError {
    LedgerError::NotFound(format!("Постачальника з ID '{id}' не знайдено"))
}

#[async_trait::async_trait]
impl LedgerService for SqlxLedger {
    // ─── v1 ─────────────────────────────────────────────────────────────────

    async fn create_entry_v1(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV1Dto, LedgerError> {
        let pool = &self.pool;
        // Перевірка постачальника → 404.
        let supplier: Option<Uuid> = sqlx::query_scalar("SELECT id FROM suppliers WHERE id = $1")
            .bind(input.supplier_id)
            .fetch_optional(pool)
            .await
            .le()?;
        if supplier.is_none() {
            return Err(not_found_supplier(&input.supplier_id));
        }
        // Невідомий тип → 400 (Python ValueError).
        if !LEDGER_TYPES_V1.contains(&input.operation_type.as_str()) {
            return Err(LedgerError::BadRequest(format!(
                "Невідомий тип операції: '{}'",
                input.operation_type
            )));
        }
        // Поточний баланс = SUM(amount) (v1 LedgerService).
        let current_raw: Option<String> =
            sqlx::query_scalar("SELECT COALESCE(SUM(amount), 0)::numeric::text FROM supplier_ledger WHERE supplier_id = $1")
                .bind(input.supplier_id)
                .fetch_one(pool)
                .await
                .le()?;
        let current =
            Decimal::from_str(current_raw.as_deref().unwrap_or("0")).unwrap_or(Decimal::ZERO);
        let amount = Decimal::from_str(&input.amount).unwrap_or(Decimal::ZERO);
        let balance_after = current + amount;
        let balance_str = balance_after.to_string();

        let op_date = input.operation_date.unwrap_or_else(utc_now_naive);
        let row = sqlx::query(
            r#"INSERT INTO supplier_ledger
               (supplier_id, operation_type, document_id, document_number,
                amount, balance_after, operation_date, notes, created_at)
               VALUES ($1, $2::ledger_operation_type, $3, $4, $5::numeric, $6::numeric, $7, $8, now())
               RETURNING id, amount::text, balance_after::text, operation_date, notes, created_at"#,
        )
        .bind(input.supplier_id)
        .bind(&input.operation_type)
        .bind(input.document_id)
        .bind(input.document_number.as_deref())
        .bind(&input.amount)
        .bind(&balance_str)
        .bind(op_date)
        .bind(input.notes.as_deref())
        .fetch_one(pool)
        .await
        .le()?;

        let notes: Option<String> = row.try_get("notes").le()?;
        let created_at: NaiveDateTime = row.try_get("created_at").le()?;
        Ok(LedgerEntryV1Dto {
            id: row.try_get("id").le()?,
            supplier_id: input.supplier_id,
            operation_type: input.operation_type.clone(),
            document_id: input.document_id,
            document_number: input.document_number.clone(),
            // Python повертає amount зі scale вводу (entry.amount до flush).
            amount: input.amount.clone(),
            balance_after: balance_str,
            operation_date: op_date,
            notes,
            created_at,
        })
    }

    async fn history_v1(
        &self,
        supplier_id: Uuid,
        page: i64,
        size: i64,
    ) -> Result<LedgerHistoryV1Dto, LedgerError> {
        let pool = &self.pool;
        let supplier: Option<Uuid> = sqlx::query_scalar("SELECT id FROM suppliers WHERE id = $1")
            .bind(supplier_id)
            .fetch_optional(pool)
            .await
            .le()?;
        if supplier.is_none() {
            return Err(not_found_supplier(&supplier_id));
        }
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM supplier_ledger WHERE supplier_id = $1")
                .bind(supplier_id)
                .fetch_one(pool)
                .await
                .le()?;
        let offset = (page - 1) * size;
        let rows = sqlx::query(
            r#"SELECT id, operation_type::text, document_id, document_number,
                      amount::text, balance_after::text, operation_date, notes, created_at
               FROM supplier_ledger
               WHERE supplier_id = $1
               ORDER BY operation_date DESC
               OFFSET $2 LIMIT $3"#,
        )
        .bind(supplier_id)
        .bind(offset)
        .bind(size)
        .fetch_all(pool)
        .await
        .le()?;
        let items = rows
            .iter()
            .map(|r| LedgerEntryV1Dto {
                id: r.try_get("id").expect("id"),
                supplier_id,
                operation_type: r.try_get("operation_type").expect("type"),
                document_id: r.try_get("document_id").expect("doc_id"),
                document_number: r.try_get("document_number").expect("doc_num"),
                amount: r.try_get("amount").expect("amount"),
                balance_after: r.try_get("balance_after").expect("bal"),
                operation_date: r.try_get("operation_date").expect("op_date"),
                notes: r.try_get("notes").expect("notes"),
                created_at: r.try_get("created_at").expect("created_at"),
            })
            .collect();
        Ok(LedgerHistoryV1Dto {
            items,
            total,
            page,
            size,
        })
    }

    async fn balance_v1(&self, supplier_id: Uuid) -> Result<LedgerBalanceV1Dto, LedgerError> {
        let pool = &self.pool;
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM suppliers WHERE id = $1")
            .bind(supplier_id)
            .fetch_optional(pool)
            .await
            .le()?;
        let name = match name {
            Some(n) => n,
            None => return Err(not_found_supplier(&supplier_id)),
        };
        let row = sqlx::query(
            "SELECT COALESCE(SUM(amount), 0)::numeric::text AS balance, MAX(operation_date) AS last_updated FROM supplier_ledger WHERE supplier_id = $1",
        )
        .bind(supplier_id)
        .fetch_one(pool)
        .await
        .le()?;
        Ok(LedgerBalanceV1Dto {
            supplier_id,
            supplier_name: name,
            current_balance: row.try_get("balance").le()?,
            last_updated: row.try_get("last_updated").le()?,
        })
    }

    // ─── v2 ─────────────────────────────────────────────────────────────────

    async fn list_entries_v2(
        &self,
        q: &LedgerEntriesQuery,
    ) -> Result<LedgerListV2Dto, LedgerError> {
        let pool = &self.pool;
        // Python: OperationType(operation_type).value — невалідний → ValueError → 500.
        if let Some(op) = &q.operation_type {
            if !LEDGER_TYPES_V2.contains(&op.as_str()) {
                return Err(LedgerError::InvalidOperationType(
                    "Внутрішня помилка сервера".to_string(),
                ));
            }
        }
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT count(*) FROM supplier_ledger WHERE 1=1",
        );
        if let Some(sid) = q.supplier_id {
            qb.push(" AND supplier_id = ");
            qb.push_bind(sid);
        }
        if let Some(op) = &q.operation_type {
            qb.push(" AND operation_type::text = ");
            qb.push_bind(op);
        }
        if let Some(df) = q.date_from {
            qb.push(" AND operation_date >= ");
            qb.push_bind(df);
        }
        if let Some(dt) = q.date_to {
            qb.push(" AND operation_date <= ");
            qb.push_bind(dt);
        }
        let total: i64 = qb.build_query_scalar().fetch_one(pool).await.le()?;

        let mut qb2 = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id, supplier_id, operation_type::text, document_id, document_number, \
             amount::float8, balance_after::float8, operation_date, notes, created_at \
             FROM supplier_ledger WHERE 1=1",
        );
        if let Some(sid) = q.supplier_id {
            qb2.push(" AND supplier_id = ");
            qb2.push_bind(sid);
        }
        if let Some(op) = &q.operation_type {
            qb2.push(" AND operation_type::text = ");
            qb2.push_bind(op);
        }
        if let Some(df) = q.date_from {
            qb2.push(" AND operation_date >= ");
            qb2.push_bind(df);
        }
        if let Some(dt) = q.date_to {
            qb2.push(" AND operation_date <= ");
            qb2.push_bind(dt);
        }
        qb2.push(" ORDER BY operation_date DESC");
        qb2.push(" OFFSET ");
        qb2.push_bind((q.page - 1) * q.size);
        qb2.push(" LIMIT ");
        qb2.push_bind(q.size);
        let rows = qb2.build().fetch_all(pool).await.le()?;
        let items = rows
            .iter()
            .map(|r| LedgerEntryV2Dto {
                id: r.try_get("id").expect("id"),
                supplier_id: r.try_get("supplier_id").expect("sid"),
                amount: r.try_get("amount").expect("amount"),
                operation_type: r.try_get("operation_type").expect("type"),
                balance_after: r.try_get("balance_after").expect("bal"),
                created_at: r.try_get("created_at").expect("created_at"),
                document_id: r.try_get("document_id").expect("doc_id"),
                document_number: r
                    .try_get::<Option<String>, _>("document_number")
                    .expect("doc_num")
                    .unwrap_or_default(),
                notes: r
                    .try_get::<Option<String>, _>("notes")
                    .expect("notes")
                    .unwrap_or_default(),
            })
            .collect();
        Ok(LedgerListV2Dto {
            items,
            total,
            page: q.page,
            size: q.size,
        })
    }

    async fn create_entry_v2(
        &self,
        input: &LedgerEntryInput,
    ) -> Result<LedgerEntryV2Dto, LedgerError> {
        let pool = &self.pool;
        // Python v2 create: 400 якщо supplier не знайдено.
        let supplier: Option<Uuid> = sqlx::query_scalar("SELECT id FROM suppliers WHERE id = $1")
            .bind(input.supplier_id)
            .fetch_optional(pool)
            .await
            .le()?;
        if supplier.is_none() {
            return Err(LedgerError::BadRequest(format!(
                "Постачальника з ID '{}' не знайдено",
                input.supplier_id
            )));
        }
        // Python: OperationType(dto.operation_type) → ValueError → 400.
        if !LEDGER_TYPES_V2.contains(&input.operation_type.as_str()) {
            return Err(LedgerError::BadRequest(format!(
                "'{}' is not a valid OperationType",
                input.operation_type
            )));
        }
        // Поточний баланс = останній balance_after (v2 repo.get_supplier_balance).
        let current_raw: Option<f64> = sqlx::query_scalar(
            "SELECT balance_after::float8 FROM supplier_ledger WHERE supplier_id = $1 \
             ORDER BY operation_date DESC, created_at DESC, id DESC LIMIT 1",
        )
        .bind(input.supplier_id)
        .fetch_optional(pool)
        .await
        .le()?;
        let current = current_raw.unwrap_or(0.0);
        let amount: f64 = Decimal::from_str(&input.amount)
            .unwrap_or(Decimal::ZERO)
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0);
        let balance_after = current + amount;
        let balance_str = format!("{balance_after:.2}");
        let amount_str = format!("{amount:.2}");

        let op_date = utc_now_naive();
        let row = sqlx::query(
            r#"INSERT INTO supplier_ledger
               (supplier_id, operation_type, document_id, document_number,
                amount, balance_after, operation_date, notes, created_at)
               VALUES ($1, $2::ledger_operation_type, $3, $4, $5::numeric, $6::numeric, $7, $8, now())
               RETURNING id, amount::float8, balance_after::float8, created_at"#,
        )
        .bind(input.supplier_id)
        .bind(&input.operation_type)
        .bind(input.document_id)
        .bind(input.document_number.as_deref())
        .bind(&amount_str)
        .bind(&balance_str)
        .bind(op_date)
        .bind(input.notes.as_deref())
        .fetch_one(pool)
        .await
        .le()?;

        Ok(LedgerEntryV2Dto {
            id: row.try_get("id").le()?,
            supplier_id: input.supplier_id,
            amount: row.try_get("amount").le()?,
            operation_type: input.operation_type.clone(),
            balance_after: Some(row.try_get("balance_after").le()?),
            created_at: Some(row.try_get("created_at").le()?),
            document_id: input.document_id,
            document_number: input.document_number.clone().unwrap_or_default(),
            notes: input.notes.clone().unwrap_or_default(),
        })
    }

    async fn balance_v2(&self, supplier_id: Uuid) -> Result<LedgerBalanceV2Dto, LedgerError> {
        let pool = &self.pool;
        let supplier: Option<Uuid> = sqlx::query_scalar("SELECT id FROM suppliers WHERE id = $1")
            .bind(supplier_id)
            .fetch_optional(pool)
            .await
            .le()?;
        if supplier.is_none() {
            return Err(not_found_supplier(&supplier_id));
        }
        let balance: f64 = sqlx::query_scalar(
            "SELECT balance_after::float8 FROM supplier_ledger WHERE supplier_id = $1 \
             ORDER BY operation_date DESC, created_at DESC, id DESC LIMIT 1",
        )
        .bind(supplier_id)
        .fetch_optional(pool)
        .await
        .le()?
        .unwrap_or(0.0);
        Ok(LedgerBalanceV2Dto {
            supplier_id,
            balance,
        })
    }

    async fn all_balances_v2(&self) -> Result<Vec<SupplierBalanceV2Dto>, LedgerError> {
        // 1:1 з Python get_all_supplier_balances: LEFT JOIN max(operation_date)
        // → дублікати рядків при однакових датах (як у Python).
        let rows = sqlx::query(
            r#"SELECT s.id AS supplier_id, s.name AS supplier_name,
                      sl.balance_after::float8 AS balance, sl.operation_date AS last_operation_date
               FROM suppliers s
               LEFT JOIN (SELECT supplier_id, MAX(operation_date) AS max_date
                          FROM supplier_ledger GROUP BY supplier_id) m
                 ON s.id = m.supplier_id
               LEFT JOIN supplier_ledger sl
                 ON sl.supplier_id = m.supplier_id AND sl.operation_date = m.max_date
               ORDER BY s.name"#,
        )
        .fetch_all(&self.pool)
        .await
        .le()?;
        Ok(rows
            .iter()
            .map(|r| SupplierBalanceV2Dto {
                supplier_id: r.try_get("supplier_id").expect("sid"),
                supplier_name: r.try_get("supplier_name").expect("name"),
                balance: r
                    .try_get::<Option<f64>, _>("balance")
                    .expect("balance")
                    .unwrap_or(0.0),
                last_operation_date: r
                    .try_get::<Option<NaiveDateTime>, _>("last_operation_date")
                    .expect("last_date"),
            })
            .collect())
    }
}

fn utc_now_naive() -> NaiveDateTime {
    chrono::Utc::now().naive_utc()
}

/// Серіалізація NaiveDateTime як Python isoformat (використовується в API).
pub fn ledger_iso(dt: NaiveDateTime) -> String {
    iso_naive(dt)
}
