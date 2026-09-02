// ─────────────────────────────────────────────────────────────────────────────
// Серверні SQL-приймачі push — не-чекові типи каси (ЕТАП 7b, offline-first)
// ─────────────────────────────────────────────────────────────────────────────
// Дизайн: docs/design/sync-schema-design.md, розділ 2.2 (типи push-дельт).
// Каса ЕТАПУ 6 реально створює офлайн (transactions.rs): purchase_order,
// inventory, transfer, write_off — кожен зі stock-ефектом ЛОКАЛЬНО в момент
// операції. Сервер-агрегатор приймає їх ідемпотентно (UNIQUE client_uuid,
// Alembic 0013 / Rust-схема) і відображає ФАКТ каси:
//
//   * документ створюється одразу зі статусом 'confirmed' (каса вже провела
//     операцію локально; власниківський draft→confirm-процес — окремий шлях
//     Python backend, касові документи в нього не потрапляють);
//   * stock-ефект (та сама таблиця `stock` per store, що й у Rust-repo):
//       purchase_order → +qty;  write_off → −qty;
//       transfer       → ±qty за стороною каси (ctx=from → −, ctx=to → +);
//       inventory      → АБСОЛЮТНИЙ рівень (факт перерахунку, set level).
//   * created_at = RFC3339 created_at каси (конверт PushEnvelope), НЕ now() —
//     звіти за датами не синкуються (QA §4.3.2);
//   * одна транзакція на агрегат: документ + items + stock-ефект атомарно.
//
// Схема-конвенція: Rust-схема (schema.sql / pos_system_fresh_test) —
// transfers.from_location/to_location (varchar, uuid каси як рядок). Робоча
// Python-БД pos_system_fresh має ручний спадок from_store_id/to_store_id —
// відкрите питання конвергенції схем Python/Rust (зафіксовано, QA §5.6).
// ─────────────────────────────────────────────────────────────────────────────

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Спроба розпарсити created_at каси (RFC3339 або ISO без таймзони) в UTC.
/// None → сервер візьме now() (контракт: created_at обов'язковий для push,
/// але м'який fallback не ламає прийом аномальних пакетів).
pub fn parse_created_at_utc(s: Option<&str>) -> Option<NaiveDateTime> {
    let s = s?.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc).naive_utc());
    }
    // Без таймзони — трактуємо як локальний час каси (UTC у контексті POS).
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
}

// ─── Міні-парсери payload (безпечні, без unwrap) ───────────────────────────

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .filter(|x| !x.is_null())
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn b(v: &Value, key: &str) -> Option<bool> {
    v.get(key).filter(|x| !x.is_null()).and_then(|x| x.as_bool())
}

fn u(v: &Value, key: &str) -> Result<Option<Uuid>, String> {
    match v.get(key).filter(|x| !x.is_null()) {
        None => Ok(None),
        Some(Value::String(s)) => Uuid::parse_str(s).map(Some).map_err(|e| {
            format!("поле '{key}': невалідний UUID '{s}': {e}")
        }),
        Some(other) => Err(format!("поле '{key}': очікувався UUID-рядок, маємо {other}")),
    }
}

/// Decimal-значення у вигляді рядка (String | Number → канонічний рядок).
fn dec(v: &Value, key: &str) -> Result<Option<String>, String> {
    match v.get(key).filter(|x| !x.is_null()) {
        None => Ok(None),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(s.clone()))
            }
        }
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(other) => Err(format!("поле '{key}': очікувалось число, маємо {other}")),
    }
}

/// scaled3: "2.500" → 2500 (мілі-одиниці), scaled2: "100.00" → 10000 (копійки).
/// Повторює семантику parse_scaled3/parse_scaled2 репозиторіїв (torgashka).
fn scaled3(s: &str) -> Option<i64> {
    let clean: String = s.trim().replace(',', ".");
    let mut parts = clean.splitn(2, '.');
    let int: i64 = parts.next()?.trim().parse().ok()?;
    let frac = parts.next().unwrap_or("");
    let mut f = frac.to_string();
    while f.len() < 3 {
        f.push('0');
    }
    f.truncate(3);
    let frac: i64 = f.parse().ok()?;
    Some(int * 1000 + if int >= 0 { frac } else { -frac })
}

fn scaled2(s: &str) -> Option<i64> {
    let clean: String = s.trim().replace(',', ".");
    let mut parts = clean.splitn(2, '.');
    let int: i64 = parts.next()?.trim().parse().ok()?;
    let frac = parts.next().unwrap_or("");
    let mut f = frac.to_string();
    while f.len() < 2 {
        f.push('0');
    }
    f.truncate(2);
    let frac: i64 = f.parse().ok()?;
    Some(int * 100 + if int >= 0 { frac } else { -frac })
}

fn dec2(v: i64) -> String {
    format!("{}.{:02}", v / 100, (v % 100).abs())
}

/// Позиції payload: усі мають product_id + quantity (Decimal). Повертає
/// (product_id, quantity-scaled3, рядок quantity, cost_price?, price?).
fn parse_items(v: &Value, kind: &str) -> Result<Vec<(Uuid, i64, String, Option<String>, Option<String>)>, String> {
    let arr = match v.get("items") {
        Some(Value::Array(a)) if !a.is_empty() => a,
        _ => return Err(format!("{kind}: payload.items порожній або відсутній")),
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, it) in arr.iter().enumerate() {
        let pid = u(it, "product_id")?.ok_or_else(|| {
            format!("{kind}: items[{i}].product_id обов'язковий")
        })?;
        let qty_s = dec(it, "quantity")?.or_else(|| {
            // inventory-фронт кладе quantity = факт; fallback actual_quantity.
            dec(it, "actual_quantity").ok().flatten()
        }).ok_or_else(|| format!("{kind}: items[{i}].quantity обов'язковий"))?;
        let qty3 = scaled3(&qty_s)
            .ok_or_else(|| format!("{kind}: items[{i}].quantity '{qty_s}' нечислове"))?;
        let cost = dec(it, "cost_price")?;
        let price = dec(it, "price")?;
        out.push((pid, qty3, qty_s, cost, price));
    }
    Ok(out)
}

/// Генерація номера документа (як next_doc_number у repo pos.rs): {P}-{date}-{seq}.
async fn next_doc_number(
    pool: &PgPool,
    table: &str,
    prefix: &str,
) -> Result<String, String> {
    let today = Utc::now().format("%Y%m%d").to_string();
    let pfx = format!("{prefix}-{today}-");
    let q = format!("SELECT max(number) FROM {table} WHERE number LIKE $1");
    let row: (Option<String>,) = sqlx::query_as(&q)
        .bind(format!("{pfx}%"))
        .fetch_one(pool)
        .await
        .map_err(|e| format!("номер {table}: {e}"))?;
    let last_seq = row
        .0
        .and_then(|n| {
            let tail: String = n.chars().rev().take(3).collect::<String>().chars().rev().collect();
            tail.parse::<i64>().ok()
        })
        .unwrap_or(0);
    Ok(format!("{pfx}{:03}", last_seq + 1))
}

/// UPSERT серверного stock per store (та сама таблиця, що й Rust-repo).
async fn stock_add(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    store_id: Uuid,
    product_id: Uuid,
    delta3: i64, // мілі-одиниці; від'ємні — зменшення
) -> Result<(), String> {
    let sign = if delta3 >= 0 { 1 } else { -1 };
    let abs = delta3.abs();
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, updated_at) \
         VALUES ($1, $2, $3::numeric, now()) \
         ON CONFLICT (store_id, product_id) \
         DO UPDATE SET quantity = stock.quantity + $4::numeric, updated_at = now()",
    )
    .bind(store_id)
    .bind(product_id)
    .bind(if sign >= 0 { format!("{abs}") } else { format!("-{abs}") })
    .bind(format!("{}", delta3 as f64 / 1000.0))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("stock_effect: {e}"))?;
    Ok(())
}

/// Абсолютний рівень (інвентаризація: факт перерахунку каси).
async fn stock_set(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    store_id: Uuid,
    product_id: Uuid,
    level3: i64,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO stock (store_id, product_id, quantity, updated_at) \
         VALUES ($1, $2, $3::numeric, now()) \
         ON CONFLICT (store_id, product_id) \
         DO UPDATE SET quantity = EXCLUDED.quantity, updated_at = now()",
    )
    .bind(store_id)
    .bind(product_id)
    .bind(format!("{}", level3 as f64 / 1000.0))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("stock_set: {e}"))?;
    Ok(())
}

// ─── Приймачі ───────────────────────────────────────────────────────────────

/// purchase_order (закупка/надходження каси): purchase_orders + items +
/// stock +qty. Payload (фронт, ЕТАП 6): supplier_id, order_date, is_fiscal,
/// notes?, items[{product_id, quantity, price, total}].
pub async fn accept_purchase_order(
    pool: &PgPool,
    store_id: Uuid,
    cashier: Uuid,
    client_uuid: Uuid,
    created_at: Option<NaiveDateTime>,
    payload: &Value,
) -> Result<Uuid, String> {
    let supplier_id = u(payload, "supplier_id")?
        .ok_or_else(|| "purchase_order: supplier_id обов'язковий".to_string())?;
    let order_date = s(payload, "order_date")
        .and_then(|d| parse_created_at_utc(Some(&d)))
        .or(created_at)
        .unwrap_or_else(|| Utc::now().naive_utc());
    let expected_date = s(payload, "expected_date")
        .and_then(|d| parse_created_at_utc(Some(&d)));
    let notes = s(payload, "notes");
    let is_fiscal = b(payload, "is_fiscal").unwrap_or(false);
    let items = parse_items(payload, "purchase_order")?;
    let ts = created_at.unwrap_or_else(|| Utc::now().naive_utc());

    // total: Σ qty×price (копійки; як Rust create_write_off).
    let mut total_cents: i128 = 0;
    for (_, q3, _, _, pr) in &items {
        let prc = pr.as_deref().and_then(scaled2).unwrap_or(0) as i128;
        total_cents += (*q3 as i128) * prc / 1000;
    }

    let mut tx = pool.begin().await.map_err(|e| format!("BEGIN: {e}"))?;
    let number = next_doc_number(pool, "purchase_orders", "ЗМ").await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO purchase_orders \
            (id, number, supplier_id, order_date, expected_date, status, is_fiscal, notes, \
             total_amount, created_at, updated_at, created_by_id, store_id, client_uuid) \
         VALUES ($1,$2,$3,$4::timestamp,$5::timestamp,'confirmed',$6,$7,$8::numeric, \
                 $9::timestamp,$9::timestamp,$10,$11,$12)",
    )
    .bind(id).bind(&number).bind(supplier_id).bind(order_date).bind(expected_date)
    .bind(is_fiscal).bind(notes.as_deref()).bind(dec2(total_cents as i64))
    .bind(ts).bind(cashier).bind(store_id).bind(client_uuid)
    .execute(&mut *tx).await.map_err(|e| format!("INSERT purchase_orders: {e}"))?;
    for (pid, q3, _q, _, pr) in &items {
        let price = pr.as_deref().and_then(scaled2).unwrap_or(0);
        sqlx::query(
            "INSERT INTO purchase_order_items \
                (id, purchase_order_id, product_id, quantity, price, total, created_at, store_id) \
             VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7::timestamp,$8)",
        )
        .bind(Uuid::new_v4()).bind(id).bind(pid).bind(format!("{}", *q3 as f64 / 1000.0))
        .bind(dec2(price)).bind(dec2(((*q3 as i128) * price as i128 / 1000) as i64))
        .bind(ts).bind(store_id)
        .execute(&mut *tx).await.map_err(|e| format!("INSERT purchase_order_items: {e}"))?;
        stock_add(&mut tx, store_id, *pid, *q3).await?;
    }
    tx.commit().await.map_err(|e| format!("COMMIT: {e}"))?;
    Ok(id)
}

/// inventory (інвентаризація каси): inventories + items + stock = факт.
/// Payload: location?, inventory_date, notes?, items[{product_id, quantity
/// (=факт), actual_quantity, accounting_quantity, difference, cost_price, price}].
pub async fn accept_inventory(
    pool: &PgPool,
    store_id: Uuid,
    cashier: Uuid,
    client_uuid: Uuid,
    created_at: Option<NaiveDateTime>,
    payload: &Value,
) -> Result<Uuid, String> {
    let location = s(payload, "location").unwrap_or_default();
    let inv_date = s(payload, "inventory_date")
        .and_then(|d| parse_created_at_utc(Some(&d)))
        .or(created_at)
        .unwrap_or_else(|| Utc::now().naive_utc());
    let notes = s(payload, "notes");
    let items = parse_items(payload, "inventory")?;
    let ts = created_at.unwrap_or_else(|| Utc::now().naive_utc());

    let mut tx = pool.begin().await.map_err(|e| format!("BEGIN: {e}"))?;
    let number = next_doc_number(pool, "inventories", "ІН").await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO inventories \
            (id, number, location, inventory_date, status, notes, created_at, updated_at, \
             created_by_id, store_id, client_uuid) \
         VALUES ($1,$2,$3,$4::timestamp,'confirmed',$5,$6::timestamp,$6::timestamp,$7,$8,$9)",
    )
    .bind(id).bind(&number).bind(&location).bind(inv_date).bind(notes.as_deref())
    .bind(ts).bind(cashier).bind(store_id).bind(client_uuid)
    .execute(&mut *tx).await.map_err(|e| format!("INSERT inventories: {e}"))?;
    for (pid, q3, qty_s, cost, price) in &items {
        let acc = dec(&payload["items"], "accounting_quantity").ok().flatten()
            .or_else(|| None);
        let _ = acc;
        // accounting_quantity/difference — з позиції (якщо є), інакше 0.
        let acc_s = item_field_qty(payload, *pid, "accounting_quantity", qty_s);
        let diff_s = item_field_qty(payload, *pid, "difference", &format!("0"));
        let cost_c = cost.as_deref().and_then(scaled2).unwrap_or(0);
        let price_c = price.as_deref().and_then(scaled2).unwrap_or(0);
        sqlx::query(
            "INSERT INTO inventory_items \
                (id, inventory_id, product_id, actual_quantity, accounting_quantity, difference, \
                 cost_price, price, created_at, store_id) \
             VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7::numeric,$8::numeric, \
                     $9::timestamp,$10)",
        )
        .bind(Uuid::new_v4()).bind(id).bind(pid)
        .bind(format!("{}", *q3 as f64 / 1000.0))
        .bind(acc_s).bind(diff_s)
        .bind(dec2(cost_c)).bind(dec2(price_c))
        .bind(ts).bind(store_id)
        .execute(&mut *tx).await.map_err(|e| format!("INSERT inventory_items: {e}"))?;
        stock_set(&mut tx, store_id, *pid, *q3).await?;
    }
    tx.commit().await.map_err(|e| format!("COMMIT: {e}"))?;
    Ok(id)
}

/// Дістати числове поле позиції (за product_id) з items[] payload.
fn item_field_qty(payload: &Value, pid: Uuid, key: &str, fallback: &str) -> String {
    if let Some(Value::Array(arr)) = payload.get("items") {
        for it in arr {
            if let Ok(Some(p)) = u(it, "product_id") {
                if p == pid {
                    if let Some(x) = dec(it, key).ok().flatten() {
                        return x;
                    }
                }
            }
        }
    }
    fallback.to_string()
}

/// transfer (переміщення каси): transfers + items + stock ±qty за стороною.
/// Payload: from_store_id, to_store_id, notes?, items[{product_id, quantity}].
/// Сервер визначає сторону за store_id каси (X-Store-Id) проти сторін payload:
/// каса=from → −qty (відвантаження); каса=to → +qty (прийом). from_location/
/// to_location (Rust-схема, varchar) зберігають uuid сторони як рядок.
pub async fn accept_transfer(
    pool: &PgPool,
    store_id: Uuid,
    cashier: Uuid,
    client_uuid: Uuid,
    created_at: Option<NaiveDateTime>,
    payload: &Value,
) -> Result<Uuid, String> {
    let from = u(payload, "from_store_id")?
        .ok_or_else(|| "transfer: from_store_id обов'язковий".to_string())?;
    let to = u(payload, "to_store_id")?
        .ok_or_else(|| "transfer: to_store_id обов'язковий".to_string())?;
    let notes = s(payload, "notes");
    let items = parse_items(payload, "transfer")?;
    let ts = created_at.unwrap_or_else(|| Utc::now().naive_utc());
    let side = if store_id == from { 1 } else if store_id == to { -1 } else { 0 };

    let mut tx = pool.begin().await.map_err(|e| format!("BEGIN: {e}"))?;
    let number = next_doc_number(pool, "transfers", "ПМ").await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO transfers \
            (id, number, from_location, to_location, transfer_date, status, notes, \
             created_at, updated_at, created_by_id, store_id, client_uuid) \
         VALUES ($1,$2,$3,$4,$5::timestamp,'confirmed',$6,$7::timestamp,$7::timestamp,$8,$9,$10)",
    )
    .bind(id).bind(&number).bind(from.to_string()).bind(to.to_string()).bind(ts)
    .bind(notes.as_deref()).bind(ts).bind(cashier).bind(store_id).bind(client_uuid)
    .execute(&mut *tx).await.map_err(|e| format!("INSERT transfers: {e}"))?;
    for (pid, q3, _q, cost, price) in &items {
        let cost_c = cost.as_deref().and_then(scaled2).unwrap_or(0);
        let price_c = price.as_deref().and_then(scaled2).unwrap_or(0);
        sqlx::query(
            "INSERT INTO transfer_items \
                (id, transfer_id, product_id, quantity, cost_price, price, created_at, store_id) \
             VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7::timestamp,$8)",
        )
        .bind(Uuid::new_v4()).bind(id).bind(pid).bind(format!("{}", *q3 as f64 / 1000.0))
        .bind(dec2(cost_c)).bind(dec2(price_c)).bind(ts).bind(store_id)
        .execute(&mut *tx).await.map_err(|e| format!("INSERT transfer_items: {e}"))?;
        match side {
            1 => stock_add(&mut tx, from, *pid, -*q3).await?,
            -1 => stock_add(&mut tx, to, *pid, *q3).await?,
            _ => {} // каса не сторона — зберігаємо документ без stock-ефекту
        }
    }
    tx.commit().await.map_err(|e| format!("COMMIT: {e}"))?;
    Ok(id)
}

/// write_off (списання каси): write_offs + items + stock −qty.
/// Payload: reason, write_off_date, notes?, items[{product_id, quantity}].
pub async fn accept_write_off(
    pool: &PgPool,
    store_id: Uuid,
    cashier: Uuid,
    client_uuid: Uuid,
    created_at: Option<NaiveDateTime>,
    payload: &Value,
) -> Result<Uuid, String> {
    let reason = s(payload, "reason")
        .filter(|r| r.trim().len() >= 2)
        .ok_or_else(|| "write_off: reason обов'язковий (>= 2 символи)".to_string())?;
    let wo_date = s(payload, "write_off_date")
        .and_then(|d| parse_created_at_utc(Some(&d)))
        .or(created_at)
        .unwrap_or_else(|| Utc::now().naive_utc());
    let notes = s(payload, "notes");
    let items = parse_items(payload, "write_off")?;
    let ts = created_at.unwrap_or_else(|| Utc::now().naive_utc());
    let mut total_cents: i128 = 0;
    for (_, q3, _, _, pr) in &items {
        let prc = pr.as_deref().and_then(scaled2).unwrap_or(0) as i128;
        total_cents += (*q3 as i128) * prc / 1000;
    }

    let mut tx = pool.begin().await.map_err(|e| format!("BEGIN: {e}"))?;
    let number = next_doc_number(pool, "write_offs", "СП").await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO write_offs \
            (id, number, reason, write_off_date, notes, status, total_amount, \
             created_at, updated_at, created_by_id, store_id, client_uuid) \
         VALUES ($1,$2,$3,$4::timestamp,$5,'confirmed',$6::numeric, \
                 $7::timestamp,$7::timestamp,$8,$9,$10)",
    )
    .bind(id).bind(&number).bind(&reason).bind(wo_date).bind(notes.as_deref())
    .bind(dec2(total_cents as i64)).bind(ts).bind(cashier).bind(store_id).bind(client_uuid)
    .execute(&mut *tx).await.map_err(|e| format!("INSERT write_offs: {e}"))?;
    for (pid, q3, _q, cost, price) in &items {
        let cost_c = cost.as_deref().and_then(scaled2).unwrap_or(0);
        let price_c = price.as_deref().and_then(scaled2).unwrap_or(0);
        sqlx::query(
            "INSERT INTO write_off_items \
                (id, write_off_id, product_id, quantity, cost_price, price, created_at, store_id) \
             VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7::timestamp,$8)",
        )
        .bind(Uuid::new_v4()).bind(id).bind(pid).bind(format!("{}", *q3 as f64 / 1000.0))
        .bind(dec2(cost_c)).bind(dec2(price_c)).bind(ts).bind(store_id)
        .execute(&mut *tx).await.map_err(|e| format!("INSERT write_off_items: {e}"))?;
        stock_add(&mut tx, store_id, *pid, -*q3).await?;
    }
    tx.commit().await.map_err(|e| format!("COMMIT: {e}"))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rfc3339_parses_to_utc_naive() {
        let d = parse_created_at_utc(Some("2026-09-10T09:31:05+03:00")).expect("rfc3339");
        assert_eq!(d.format("%Y-%m-%dT%H:%M:%S").to_string(), "2026-09-10T06:31:05");
        // ISO без таймзони — як є.
        let d2 = parse_created_at_utc(Some("2026-09-10T09:31:05")).expect("naive");
        assert_eq!(d2.format("%Y-%m-%dT%H:%M:%S").to_string(), "2026-09-10T09:31:05");
        assert!(parse_created_at_utc(None).is_none());
        assert!(parse_created_at_utc(Some("not-a-date")).is_none());
    }

    #[test]
    fn scaled_helpers_parse_decimal_strings_and_numbers() {
        assert_eq!(scaled3("2.500"), Some(2500));
        assert_eq!(scaled3("10"), Some(10_000));
        assert_eq!(scaled3("-3"), Some(-3000));
        assert_eq!(scaled2("20.00"), Some(2000));
        assert_eq!(scaled2("5"), Some(500));
        assert_eq!(dec2(250_00), "250.00");
        assert_eq!(dec2(-250_00), "-250.00");
    }

    #[test]
    fn items_parse_accepts_number_and_string_quantity() {
        let v = json!({"items": [
            {"product_id": "65d5db51-672f-4a38-9c1e-f36c5feb5374", "quantity": 2.5},
            {"product_id": "65d5db51-672f-4a38-9c1e-f36c5feb5374", "quantity": "2.500"},
        ]});
        let items = parse_items(&v, "t").expect("items");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1, 2500);
        assert_eq!(items[1].1, 2500);
    }

    #[test]
    fn items_parse_rejects_empty() {
        assert!(parse_items(&json!({"items": []}), "t").is_err());
        assert!(parse_items(&json!({}), "t").is_err());
    }

    #[test]
    fn items_parse_rejects_bad_product_id() {
        let v = json!({"items": [{"product_id": "not-a-uuid", "quantity": 1}]});
        assert!(parse_items(&v, "t").is_err());
    }
}
