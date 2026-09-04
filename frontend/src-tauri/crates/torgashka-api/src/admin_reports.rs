// ─────────────────────────────────────────────────────────────────────────────
// admin_reports — «Звітність мережі» (Етап 4 адмін-панелі власника мережі,
// ТЗ розділи 5.5 — Дашборд мережі, 5.6 — Фінанси/каса мережі)
// ─────────────────────────────────────────────────────────────────────────────
// Роути (окремий /admin/* роутер БЕЗ store_middleware; RBAC owner|store_manager
// |admin через auth_routes::require_admin, як admin.rs / admin_db_sources.rs):
//
//   GET /api/v1/admin/reports/network-sales?from=&to=&limit=
//        → { from, to, stores[], totals{}, top_products[] }
//   GET /api/v1/admin/reports/cash-operations?from=&to=
//        → { from, to, stores[], totals{} }
//   GET /api/v1/admin/reports/supplier-ledger?from=&to=
//        → { from, to, suppliers[], totals{} }
//
// Семантика (зафіксовано для тестів і звітів):
//  • «Продажі по точках» (network-sales): агрегати ЛИШЕ активних точок
//    мережі (s.is_active=true). Архівні точки виключені (вимога ТЗ:
//    «виключити або позначити» — виключаємо; історичні звіти по архівній
//    точці можна отримати на рівні каси/точки окремим звітом).
//  • net_sales = SUM(total_amount) де receipt_type='sale' МІНУС
//    SUM(total_amount) де receipt_type='return'. Через receipt_type
//    (напрямок операції), а не знак суми: у застосунку суми чеків
//    зберігаються додатними, напрямок — тип чека.
//  • top_products: внесок товару = ri.total для sale, -ri.total для return
//    (CASE за receipt_type). Тобто повернення зменшують суму продажів
//    товару (мережева «сума продажів по товару» = нетто).
//  • cash-operations: сума deposit / сума collection по точках за період.
//    created_at — timestamptz; межі from/to (дати без зони, аналог наявного
//    коду звітів: created_at трактується в UTC) → Utc-межі доби.
//  • supplier-ledger: постачальники СПІЛЬНІ на мережу (supplier_ledger без
//    store_id — див. repositories/invoices.rs: INSERT без store_id).
//    Зведений баланс = balance_after останнього запису журналу
//    (operation_date DESC, created_at DESC, id DESC — та сама семантика,
//    що в infrastructure ledger.rs balance_v2/all_balances_v2). Період
//    фільтрує лише «оборот за період» (inflow/outflow/net); поточний баланс —
//    завжди останній по всьому журналу (не залежить від періоду).
//
// Гроші в DTO — рядки з scale БД (як решта Rust-фасаду, numeric → ::text):
// фронтенд форматує. Кількості — цілі.
//
// Пустий період НЕ є помилкою: рядків немає → порожні масиви, totals = 0.
// from>to → 400 з поясненням.
// ─────────────────────────────────────────────────────────────────────────────
use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{auth::Claims, auth_routes, AppState};

// ─── Помилки → HTTP ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AdminReportsErr {
    /// 401/403 — auth_routes::require_admin (власний IntoResponse).
    Auth(auth_routes::AuthRouteError),
    /// 400 — невірні query-параметри.
    BadRequest(String),
    /// 500 — БД.
    Db(sqlx::Error),
}

impl From<auth_routes::AuthRouteError> for AdminReportsErr {
    fn from(e: auth_routes::AuthRouteError) -> Self {
        AdminReportsErr::Auth(e)
    }
}

impl From<sqlx::Error> for AdminReportsErr {
    fn from(e: sqlx::Error) -> Self {
        AdminReportsErr::Db(e)
    }
}

impl IntoResponse for AdminReportsErr {
    fn into_response(self) -> Response {
        let body = |status: StatusCode, msg: String| {
            (status, Json(serde_json::json!({"detail": msg}))).into_response()
        };
        match self {
            AdminReportsErr::Auth(e) => e.into_response(),
            AdminReportsErr::BadRequest(m) => body(StatusCode::BAD_REQUEST, m),
            AdminReportsErr::Db(e) => {
                eprintln!("[torgashka-api] admin_reports: помилка БД: {e}");
                body(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Внутрішня помилка сервера".to_string(),
                )
            }
        }
    }
}

/// Пул PostgreSQL фасаду (як admin.rs / admin_db_sources.rs).
fn pool(state: &AppState) -> Result<sqlx::PgPool, AdminReportsErr> {
    state
        .write_pool
        .clone()
        .ok_or_else(|| AdminReportsErr::BadRequest("write_pool не ініціалізовано".to_string()))
}

// ─── Парсинг періоду ────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ReportRangeQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    /// Топ-N товарів мережі (default 10, 1..=50).
    pub limit: Option<i64>,
}

/// 'YYYY-MM-DD' → початок/кінець доби (імітує normalize_date_to в pos.rs:
/// дата без часу на to → 23:59:59.999999). Допускаємо і 'YYYY-MM-DDTHH:MM:SS'.
fn parse_bound(s: &str, is_to: bool) -> Result<NaiveDateTime, AdminReportsErr> {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return if is_to {
            d.and_hms_nano_opt(23, 59, 59, 999_999_000)
                .ok_or_else(|| AdminReportsErr::BadRequest(format!("Невірний період to: '{s}'")))
        } else {
            d.and_hms_opt(0, 0, 0)
                .ok_or_else(|| AdminReportsErr::BadRequest(format!("Невірний період from: '{s}'")))
        };
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt);
        }
    }
    Err(AdminReportsErr::BadRequest(format!(
        "Невірний параметр періоду: '{s}' — очікується YYYY-MM-DD або YYYY-MM-DDTHH:MM:SS"
    )))
}

/// from/to → (наівні UTC-межі, Option). Пусті параметри = відкритий період.
fn parse_range(
    q: &ReportRangeQuery,
) -> Result<(Option<NaiveDateTime>, Option<NaiveDateTime>), AdminReportsErr> {
    let from = q
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_bound(s, false))
        .transpose()?;
    let to =
        q.to.as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| parse_bound(s, true))
            .transpose()?;
    if let (Some(f), Some(t)) = (from, to) {
        if f > t {
            return Err(AdminReportsErr::BadRequest(
                "from пізніше ніж to: період порожній".to_string(),
            ));
        }
    }
    Ok((from, to))
}

// ─── DTO ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct NetworkSalesRowDto {
    pub store_id: Uuid,
    pub store_name: String,
    pub is_active: bool,
    /// Продажі (total_amount, sale) за період.
    pub sales: String,
    /// Повернення (total_amount, return) за період — додатне число.
    pub returns: String,
    /// Нетто = sales - returns.
    pub net_sales: String,
    pub sales_checks: i64,
    pub returns_checks: i64,
}

#[derive(Debug, Serialize)]
pub struct NetworkSalesTotalsDto {
    pub sales: String,
    pub returns: String,
    pub net_sales: String,
    pub sales_checks: i64,
    pub returns_checks: i64,
}

#[derive(Debug, Serialize)]
pub struct TopProductDto {
    pub product_id: Uuid,
    pub product_name: String,
    /// Нетто-сума продажів товару по мережі за період (return зі знаком мінус).
    pub total: String,
}

#[derive(Debug, Serialize)]
pub struct NetworkSalesDto {
    pub from: Option<String>,
    pub to: Option<String>,
    pub stores: Vec<NetworkSalesRowDto>,
    pub totals: NetworkSalesTotalsDto,
    pub top_products: Vec<TopProductDto>,
}

#[derive(Debug, Serialize)]
pub struct CashRowDto {
    pub store_id: Uuid,
    pub store_name: String,
    pub is_active: bool,
    pub deposit: String,
    pub collection: String,
    pub operations: i64,
}

#[derive(Debug, Serialize)]
pub struct CashTotalsDto {
    pub deposit: String,
    pub collection: String,
    pub operations: i64,
}

#[derive(Debug, Serialize)]
pub struct CashOperationsDto {
    pub from: Option<String>,
    pub to: Option<String>,
    pub stores: Vec<CashRowDto>,
    pub totals: CashTotalsDto,
}

#[derive(Debug, Serialize)]
pub struct SupplierLedgerRowDto {
    pub supplier_id: Uuid,
    pub supplier_name: String,
    /// К-сть операцій за період.
    pub period_operations: i64,
    /// amount>0 за період (надходження/прихід).
    pub period_inflow: String,
    /// сума |amount| для amount<0 за період (оплати/повернення).
    pub period_outflow: String,
    /// period_inflow - period_outflow (алгебраїчний оборот за період).
    pub period_net: String,
    /// Поточний зведений баланс = balance_after останнього запису (весь журнал).
    pub current_balance: String,
    pub last_operation_date: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize)]
pub struct SupplierLedgerTotalsDto {
    /// Сума оборотів (amount>0) за період по всіх постачальниках.
    pub inflow: String,
    /// Сума оплат (|amount|, amount<0) за період по всіх постачальниках.
    pub outflow: String,
    /// inflow - outflow.
    pub net: String,
    /// Сума поточних балансів по всіх постачальниках звіту.
    pub balance: String,
}

#[derive(Debug, Serialize)]
pub struct SupplierLedgerDto {
    pub from: Option<String>,
    pub to: Option<String>,
    pub suppliers: Vec<SupplierLedgerRowDto>,
    pub totals: SupplierLedgerTotalsDto,
}

// ─── Точна арифметика грошей у totals (уникнення float) ─────────────────────

/// '1234.50' → 123450 копійок (i128). Порожнє → 0.
fn to_cents(s: &str) -> i128 {
    let s = s.trim();
    if s.is_empty() || s == "-" {
        return 0;
    }
    let (neg, s) = if let Some(stripped) = s.strip_prefix('-') {
        (true, stripped)
    } else {
        (false, s)
    };
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let mut frac = frac_part.to_string();
    while frac.len() < 2 {
        frac.push('0');
    }
    frac.truncate(2);
    let i: i128 = int_part.parse().unwrap_or(0);
    let f: i128 = frac.parse().unwrap_or(0);
    let v = i * 100 + f;
    if neg {
        -v
    } else {
        v
    }
}

fn fmt_cents(c: i128) -> String {
    let neg = c < 0;
    let a = c.abs();
    format!("{}{}.{:02}", if neg { "-" } else { "" }, a / 100, a % 100)
}

fn sum_str(a: &str, b: &str) -> String {
    fmt_cents(to_cents(a) + to_cents(b))
}

// ─── GET /api/v1/admin/reports/network-sales ────────────────────────────────

pub async fn network_sales(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ReportRangeQuery>,
) -> Result<Json<NetworkSalesDto>, AdminReportsErr> {
    auth_routes::require_admin(&state, &claims).await?;
    let db = pool(&state)?;
    let (from, to) = parse_range(&q)?;
    let limit = q.limit.unwrap_or(10).clamp(1, 50);

    // Агрегати по активних точках: LEFT JOIN → точки без чеків = нулі.
    let rows = sqlx::query(
        r#"
        SELECT s.id AS store_id, s.name AS store_name,
               COALESCE(SUM(r.total_amount) FILTER (WHERE r.receipt_type = 'sale'), 0)::text
                 AS sales,
               COALESCE(SUM(r.total_amount) FILTER (WHERE r.receipt_type = 'return'), 0)::text
                 AS returns,
               (COALESCE(SUM(r.total_amount) FILTER (WHERE r.receipt_type = 'sale'), 0)
                - COALESCE(SUM(r.total_amount) FILTER (WHERE r.receipt_type = 'return'), 0)
               )::text AS net_sales,
               COUNT(*) FILTER (WHERE r.receipt_type = 'sale') AS sales_checks,
               COUNT(*) FILTER (WHERE r.receipt_type = 'return') AS returns_checks
        FROM stores s
        LEFT JOIN receipts r ON r.store_id = s.id
            AND r.created_at >= COALESCE($1::timestamp, '-infinity')
            AND r.created_at <= COALESCE($2::timestamp, 'infinity')
        WHERE s.is_active = true
        GROUP BY s.id, s.name
        ORDER BY s.name
        "#,
    )
    .bind(from)
    .bind(to)
    .fetch_all(&db)
    .await?;

    let mut stores = Vec::new();
    for r in &rows {
        let sales: String = r.get("sales");
        let returns: String = r.get("returns");
        let net: String = r.get("net_sales");
        stores.push(NetworkSalesRowDto {
            store_id: r.get("store_id"),
            store_name: r.get("store_name"),
            is_active: true,
            net_sales: net.clone(),
            sales: sales.clone(),
            returns: returns.clone(),
            sales_checks: r.get("sales_checks"),
            returns_checks: r.get("returns_checks"),
        });
    }

    // Підсумок по мережі — сума рядків (копійки, без float).
    let mut totals = NetworkSalesTotalsDto {
        sales: "0".to_string(),
        returns: "0".to_string(),
        net_sales: "0".to_string(),
        sales_checks: 0,
        returns_checks: 0,
    };
    for s in &stores {
        totals.sales = sum_str(&totals.sales, &s.sales);
        totals.returns = sum_str(&totals.returns, &s.returns);
        totals.net_sales = sum_str(&totals.net_sales, &s.net_sales);
        totals.sales_checks += s.sales_checks;
        totals.returns_checks += s.returns_checks;
    }

    // Топ-N товарів мережі за нетто-сумою (return зі знаком мінус).
    let rows = sqlx::query(
        r#"
        SELECT p.id AS product_id, p.title AS product_name,
               COALESCE(SUM(
                   CASE WHEN r.receipt_type = 'return' THEN -ri.total ELSE ri.total END
               ), 0)::text AS total
        FROM receipt_items ri
        JOIN receipts r ON r.id = ri.receipt_id
        JOIN products p ON p.id = ri.product_id
        WHERE r.store_id IN (SELECT id FROM stores WHERE is_active = true)
          AND r.created_at >= COALESCE($1::timestamp, '-infinity')
          AND r.created_at <= COALESCE($2::timestamp, 'infinity')
        GROUP BY p.id, p.title
        ORDER BY total DESC
        LIMIT $3
        "#,
    )
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(&db)
    .await?;

    let top_products = rows
        .iter()
        .map(|r| TopProductDto {
            product_id: r.get("product_id"),
            product_name: r.get("product_name"),
            total: r.get("total"),
        })
        .collect();

    Ok(Json(NetworkSalesDto {
        from: q.from.clone(),
        to: q.to.clone(),
        stores,
        totals,
        top_products,
    }))
}

// ─── GET /api/v1/admin/reports/cash-operations ──────────────────────────────

pub async fn cash_operations(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ReportRangeQuery>,
) -> Result<Json<CashOperationsDto>, AdminReportsErr> {
    auth_routes::require_admin(&state, &claims).await?;
    let db = pool(&state)?;
    let (from, to) = parse_range(&q)?;
    // Межі доби (UTC-конвенція наявного коду) → timestamptz-межі для колонки.
    let from_utc: Option<DateTime<Utc>> = from.map(|d| d.and_utc());
    let to_utc: Option<DateTime<Utc>> = to.map(|d| d.and_utc());

    let rows = sqlx::query(
        r#"
        SELECT s.id AS store_id, s.name AS store_name,
               COALESCE(SUM(c.amount) FILTER (WHERE c.operation_type = 'deposit'), 0)::text
                 AS deposit,
               COALESCE(SUM(c.amount) FILTER (WHERE c.operation_type = 'collection'), 0)::text
                 AS collection,
               COUNT(*) AS operations
        FROM stores s
        LEFT JOIN cash_operations c ON c.store_id = s.id
            AND c.created_at >= COALESCE($1::timestamptz, '-infinity')
            AND c.created_at <= COALESCE($2::timestamptz, 'infinity')
        WHERE s.is_active = true
        GROUP BY s.id, s.name
        ORDER BY s.name
        "#,
    )
    .bind(from_utc)
    .bind(to_utc)
    .fetch_all(&db)
    .await?;

    let mut stores = Vec::new();
    for r in &rows {
        stores.push(CashRowDto {
            store_id: r.get("store_id"),
            store_name: r.get("store_name"),
            is_active: true,
            deposit: r.get("deposit"),
            collection: r.get("collection"),
            operations: r.get("operations"),
        });
    }

    let mut totals = CashTotalsDto {
        deposit: "0".to_string(),
        collection: "0".to_string(),
        operations: 0,
    };
    for s in &stores {
        totals.deposit = sum_str(&totals.deposit, &s.deposit);
        totals.collection = sum_str(&totals.collection, &s.collection);
        totals.operations += s.operations;
    }

    Ok(Json(CashOperationsDto {
        from: q.from.clone(),
        to: q.to.clone(),
        stores,
        totals,
    }))
}

// ─── GET /api/v1/admin/reports/supplier-ledger ──────────────────────────────

pub async fn supplier_ledger(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ReportRangeQuery>,
) -> Result<Json<SupplierLedgerDto>, AdminReportsErr> {
    auth_routes::require_admin(&state, &claims).await?;
    let db = pool(&state)?;
    let (from, to) = parse_range(&q)?;

    let rows = sqlx::query(
        r#"
        SELECT sup.id AS supplier_id, sup.name AS supplier_name,
               COALESCE(per.inflow, 0)::text AS period_inflow,
               COALESCE(per.outflow, 0)::text AS period_outflow,
               COALESCE(per.ops, 0) AS period_operations,
               COALESCE(last.balance_after, 0)::text AS current_balance,
               last.operation_date AS last_operation_date
        FROM suppliers sup
        LEFT JOIN (
            SELECT supplier_id,
                   COALESCE(SUM(amount) FILTER (WHERE amount > 0), 0) AS inflow,
                   COALESCE(SUM(-amount) FILTER (WHERE amount < 0), 0) AS outflow,
                   COUNT(*) AS ops
            FROM supplier_ledger
            WHERE operation_date >= COALESCE($1::timestamp, '-infinity')
              AND operation_date <= COALESCE($2::timestamp, 'infinity')
            GROUP BY supplier_id
        ) per ON per.supplier_id = sup.id
        LEFT JOIN LATERAL (
            SELECT balance_after, operation_date
            FROM supplier_ledger
            WHERE supplier_id = sup.id
            ORDER BY operation_date DESC, created_at DESC, id DESC
            LIMIT 1
        ) last ON true
        WHERE sup.id IN (SELECT DISTINCT supplier_id FROM supplier_ledger)
        ORDER BY sup.name
        "#,
    )
    .bind(from)
    .bind(to)
    .fetch_all(&db)
    .await?;

    let mut suppliers = Vec::new();
    for r in &rows {
        let inflow: String = r.get("period_inflow");
        let outflow: String = r.get("period_outflow");
        let net = fmt_cents(to_cents(&inflow) - to_cents(&outflow));
        suppliers.push(SupplierLedgerRowDto {
            supplier_id: r.get("supplier_id"),
            supplier_name: r.get("supplier_name"),
            period_operations: r.get("period_operations"),
            period_inflow: inflow,
            period_outflow: outflow,
            period_net: net,
            current_balance: r.get("current_balance"),
            last_operation_date: r.get("last_operation_date"),
        });
    }

    // Постачальники без операцій за період: per.ops = 0, але current_balance
    // лишається останнім по журналу → включаємо їх у зведений баланс мережі.
    let mut totals = SupplierLedgerTotalsDto {
        inflow: "0".to_string(),
        outflow: "0".to_string(),
        net: "0".to_string(),
        balance: "0".to_string(),
    };
    for s in &suppliers {
        totals.inflow = sum_str(&totals.inflow, &s.period_inflow);
        totals.outflow = sum_str(&totals.outflow, &s.period_outflow);
        totals.net = sum_str(&totals.net, &s.period_net);
        totals.balance = sum_str(&totals.balance, &s.current_balance);
    }

    Ok(Json(SupplierLedgerDto {
        from: q.from.clone(),
        to: q.to.clone(),
        suppliers,
        totals,
    }))
}
