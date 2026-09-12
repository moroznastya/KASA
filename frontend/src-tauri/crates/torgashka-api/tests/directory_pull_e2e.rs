//! E5 «частина A» (ADR-0008 §7.1-C): довідники, придатні для pull (C1/C2/C5/C6).
//!
//! Дефект, що закривається: `barcodes`, `product_images`, `write_off_reasons`
//! не мали ні `server_version`, ні рядка в `sync_meta`, ні місця в
//! `ALLOWED_ENTITIES` → зміна на ХАБІ не доїжджала до вузлів узагалі
//! (немає версії → немає дельти; немає entity → pull віддає 400). Окремо:
//! `system_settings` не мав `is_deleted`, через що хендлер був ЗМУШЕНИЙ
//! видавати лише `op=upsert` (видалення не доїжджало), а рядок `stock_norms`
//! у `sync_meta`/`ALLOWED_ENTITIES` описував таблицю, якої не існує.
//!
//! Другий вузол тут — ОКРЕМИЙ інстанс (власна БД + власний фасад + апстрім на
//! хаб): дельту він тягне з хаба, як у проді. Застосування дельти у локальну
//! БД вузла — клієнтська частина (Tauri `offline/sync_pull.rs`), поза крейтом
//! API; тому «доїжджає на вузол» перевіряється на каналі хаба, який вузол тягне
//! (той самий підхід, що в `catalog_proposal_e2e.rs`).
//!
//! C4 (`print_templates`) — НЕ синхронізується: див. коментар
//! `// C4: локальний оверрайд — рішення Творця відсутнє (Б?)` у `sync.rs`
//! та обґрунтування в міграції `0023_directory_pull_versions.py`.

mod common;

use uuid::Uuid;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;

fn evidence(line: &str) {
    eprintln!("[e5a][evidence] {line}");
}

/// Хаб + вузол (окремі БД; вузол знає свій апстрім — саме це робить його
/// вузлом). Повертає: пул хаба, пул вузла, seed-товар, base хаба, base вузла.
async fn hub_and_node(tag: &str) -> (sqlx::PgPool, sqlx::PgPool, Uuid, String, String) {
    common::force_test_db();
    let hub = env::hub_pool(&env::pool_to(&env::test_db_url()).await, tag).await;
    let node = env::node_pool(&hub, &format!("{tag}n")).await;
    let product = Uuid::new_v4();
    env::seed(&hub, product, &format!("{tag}-hub")).await;
    env::seed(&node, product, &format!("{tag}-node")).await;

    // readdirs обов'язковий: `/api/v1/sync/master` реєструється лише за нього.
    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (node_base, _) = env::serve_any(env::app_state(&node)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&node_base).await;

    let token = env::owner_token();
    env::set_hub_upstream(&node, &hub_base, &token).await;
    (hub, node, product, hub_base, node_base)
}

/// Знайти change за id у дельті (паніка з тілом дельти, якщо немає).
fn find_change<'a>(delta: &'a serde_json::Value, id: &Uuid) -> &'a serde_json::Value {
    delta["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("дельта без changes: {delta}"))
        .iter()
        .find(|c| c["id"] == id.to_string())
        .unwrap_or_else(|| panic!("правки {id} немає в дельті: {delta}"))
}

/// C1: `barcodes` — зміна на хабі стає версією й доїжджає до вузла pull-ом.
#[tokio::test]
async fn barcodes_change_visible_on_node() {
    let (hub, node, product, hub_base, node_base) = hub_and_node("dirbc").await;
    let token = env::owner_token();

    // (а) рядок sync_meta існує — тригер bump має куди писати версію (C1).
    let meta: i64 = sqlx::query_scalar("SELECT version FROM sync_meta WHERE entity = 'barcodes'")
        .fetch_one(&hub)
        .await
        .expect("sync_meta.barcodes — рядок C1 мусить існувати");
    evidence(&format!("sync_meta.barcodes існує, version={meta} (C1)"));

    // (б) інстанс-2 — справжній вузол (тягне з хаба, а не пише сам).
    let (code_st, st) = env::sync_status(&node_base, &token).await;
    assert_eq!(code_st, 200, "status вузла: {st}");
    assert_eq!(st["role"], "node", "другий інстанс — вузол: {st}");

    // (в) зміна НА ХАБІ: новий штрих-код товару в точці (store_id точки).
    let bc_id = Uuid::new_v4();
    let barcode = format!("4823{}", &bc_id.simple().to_string()[..9]);
    sqlx::query(
        "INSERT INTO barcodes (id, product_id, barcode, is_primary, store_id)
         VALUES ($1, $2, $3, false, $4)",
    )
    .bind(bc_id)
    .bind(product)
    .bind(&barcode)
    .bind(env::store_id())
    .execute(&hub)
    .await
    .expect("INSERT barcodes на хабі");

    let (row_v, meta_v): (i64, i64) = sqlx::query_as(
        "SELECT b.server_version, m.version FROM barcodes b, sync_meta m \
         WHERE b.id = $1 AND m.entity = 'barcodes'",
    )
    .bind(bc_id)
    .fetch_one(&hub)
    .await
    .expect("версії barcodes/sync_meta");
    assert!(row_v > 0, "тригер проставив server_version: {row_v}");
    assert_eq!(
        row_v, meta_v,
        "server_version рядка = sync_meta.barcodes (одне джерело версії)"
    );
    // На вузлі цього штрих-коду ще немає — зміна приходить САМЕ з хаба.
    let on_node: i64 = sqlx::query_scalar("SELECT count(*) FROM barcodes WHERE id = $1")
        .bind(bc_id)
        .fetch_one(&node)
        .await
        .expect("count barcodes на вузлі");
    assert_eq!(on_node, 0, "на вузлі штрих-коду ще немає");
    evidence(&format!(
        "хаб: штрих-код {barcode} (товар {product}) version={row_v} → trg_barcodes_bump"
    ));

    // (г) вузол тягне дельту хаба НАЯВНИМ pull-каналом.
    let (code_d, delta) = env::master_delta(&hub_base, &token, "barcodes", 0).await;
    assert_eq!(
        code_d, 200,
        "entity 'barcodes' тепер у ALLOWED_ENTITIES (до C1 — 400): {delta}"
    );
    let found = find_change(&delta, &bc_id);
    assert_eq!(found["op"], "upsert");
    assert_eq!(found["version"].as_i64(), Some(row_v));
    assert_eq!(found["data"]["barcode"], barcode.as_str());
    assert_eq!(found["data"]["product_id"], product.to_string());
    assert_eq!(found["data"]["is_primary"], false);
    evidence(&format!(
        "вузол через наявний pull: у дельті barcodes є {bc_id} з version={row_v} і payload-ом"
    ));

    // (д) повторний pull із уже отриманої версії — правка не дублюється.
    let (_, again) = env::master_delta(&hub_base, &token, "barcodes", row_v).await;
    let redelivered = again["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .any(|c| c["id"] == bc_id.to_string());
    assert!(!redelivered, "повторна видача тієї самої версії: {again}");
    evidence("повторний pull із since_version=отримана версія → правка не дублюється");
}

/// C1 (друга таблиця): `product_images` — той самий шлях, що `barcodes`.
#[tokio::test]
async fn product_images_change_visible_on_node() {
    let (hub, _node, product, hub_base, _node_base) = hub_and_node("dirimg").await;
    let token = env::owner_token();

    let img_id = Uuid::new_v4();
    let url = format!("/uploads/e5a-{img_id}.jpg");
    sqlx::query(
        "INSERT INTO product_images (id, product_id, url, is_main, sort_order, store_id)
         VALUES ($1, $2, $3, true, 3, $4)",
    )
    .bind(img_id)
    .bind(product)
    .bind(&url)
    .bind(env::store_id())
    .execute(&hub)
    .await
    .expect("INSERT product_images на хабі");

    let (row_v, meta_v): (i64, i64) = sqlx::query_as(
        "SELECT i.server_version, m.version FROM product_images i, sync_meta m \
         WHERE i.id = $1 AND m.entity = 'product_images'",
    )
    .bind(img_id)
    .fetch_one(&hub)
    .await
    .expect("версії product_images/sync_meta");
    assert!(row_v > 0 && row_v == meta_v, "{row_v} != {meta_v}");

    let (code, delta) = env::master_delta(&hub_base, &token, "product_images", 0).await;
    assert_eq!(code, 200, "entity 'product_images' доступна pull: {delta}");
    let found = find_change(&delta, &img_id);
    assert_eq!(found["op"], "upsert");
    assert_eq!(found["version"].as_i64(), Some(row_v));
    assert_eq!(found["data"]["url"], url.as_str());
    assert_eq!(found["data"]["is_main"], true);
    assert_eq!(found["data"]["sort_order"], 3);
    evidence(&format!(
        "хаб: зображення {url} version={row_v} → trg_product_images_bump; вузол бачить його в дельті"
    ));
}

/// C2: tombstone-делеція довідника причин списання ДОЇЖДЖАЄ (не зникає мовчки).
#[tokio::test]
async fn write_off_reason_tombstone_propagates() {
    let (hub, _node, _product, hub_base, _node_base) = hub_and_node("dirwo").await;
    let token = env::owner_token();

    let reason_id = Uuid::new_v4();
    let name = format!("E5A причина {reason_id}");
    sqlx::query("INSERT INTO write_off_reasons (id, name, is_active) VALUES ($1, $2, true)")
        .bind(reason_id)
        .bind(&name)
        .execute(&hub)
        .await
        .expect("INSERT write_off_reasons (хаб)");

    let v1: i64 = sqlx::query_scalar("SELECT server_version FROM write_off_reasons WHERE id = $1")
        .bind(reason_id)
        .fetch_one(&hub)
        .await
        .expect("server_version після INSERT");
    assert!(v1 > 0, "тригер bump на INSERT: {v1}");

    // До видалення вузол бачить upsert (контроль: довідник справді в дельті).
    let (code0, d0) = env::master_delta(&hub_base, &token, "write_off_reasons", 0).await;
    assert_eq!(
        code0, 200,
        "entity 'write_off_reasons' у ALLOWED_ENTITIES: {d0}"
    );
    let before = find_change(&d0, &reason_id);
    assert_eq!(before["op"], "upsert");
    assert_eq!(before["data"]["name"], name.as_str());

    // ── Tombstone-делеція довідника (не фізичне видалення рядка) ──────────
    sqlx::query("UPDATE write_off_reasons SET is_deleted = true WHERE id = $1")
        .bind(reason_id)
        .execute(&hub)
        .await
        .expect("tombstone write_off_reasons");
    let v2: i64 = sqlx::query_scalar("SELECT server_version FROM write_off_reasons WHERE id = $1")
        .bind(reason_id)
        .fetch_one(&hub)
        .await
        .expect("server_version після tombstone");
    assert!(v2 > v1, "видалення — це нова версія: {v1} → {v2}");

    // Вузол, що тягне З НУЛЯ, бачить довідник як `delete` (не порожньо!).
    let (code1, d1) = env::master_delta(&hub_base, &token, "write_off_reasons", 0).await;
    assert_eq!(code1, 200, "{d1}");
    let after = find_change(&d1, &reason_id);
    assert_eq!(after["op"], "delete", "tombstone їде як op=delete: {after}");
    assert_eq!(after["version"].as_i64(), Some(v2));
    assert!(
        after["data"].is_null(),
        "delete не тягне payload (рядок локально позначається видаленим): {after}"
    );

    // Вузол, що вже мав v1, теж отримує саме видалення (дельта не «мовчить»).
    let (_, d2) = env::master_delta(&hub_base, &token, "write_off_reasons", v1).await;
    let delta_on_known = find_change(&d2, &reason_id);
    assert_eq!(delta_on_known["op"], "delete");
    assert_eq!(delta_on_known["version"].as_i64(), Some(v2));
    evidence(&format!(
        "tombstone: причина {reason_id} v{v1}→v{v2}, вузол бачить op=delete (з нуля і з since=v1) — не зникає мовчки"
    ));
}

/// C5: `system_settings.is_deleted` → видалення налаштування РЕАЛЬНО їде
/// (`op=delete`), а не підмінюється `upsert`-ом, як було до міграції 0023.
#[tokio::test]
async fn system_settings_delete_propagates() {
    let (hub, _node, _product, hub_base, _node_base) = hub_and_node("dirset").await;
    let token = env::owner_token();

    let set_id = Uuid::new_v4();
    let key = format!("e5a.{}", &set_id.simple().to_string()[..8]);
    sqlx::query(
        "INSERT INTO system_settings \
            (id, module, key, value, value_type, label, is_active, created_at, updated_at, store_id) \
         VALUES ($1, 'e5a', $2, 'v1', 'string', 'E5A', true, now(), now(), $3)",
    )
    .bind(set_id)
    .bind(&key)
    .bind(env::store_id())
    .execute(&hub)
    .await
    .expect("INSERT system_settings (точка)");

    let (v1, del1): (i64, bool) =
        sqlx::query_as("SELECT server_version, is_deleted FROM system_settings WHERE id = $1")
            .bind(set_id)
            .fetch_one(&hub)
            .await
            .expect("system_settings після INSERT");
    assert!(v1 > 0, "тригер trg_system_settings_bump: {v1}");
    assert!(!del1, "C5: колонка is_deleted існує і стартує false");

    let (code0, d0) = env::master_delta(&hub_base, &token, "settings", 0).await;
    assert_eq!(code0, 200, "pull settings: {d0}");
    assert_eq!(
        find_change(&d0, &set_id)["op"],
        "upsert",
        "налаштування точки видно в дельті точки"
    );

    // ── Видалення налаштування (soft) ─────────────────────────────────────
    sqlx::query("UPDATE system_settings SET is_deleted = true WHERE id = $1")
        .bind(set_id)
        .execute(&hub)
        .await
        .expect("soft-delete system_settings");
    let v2: i64 = sqlx::query_scalar("SELECT server_version FROM system_settings WHERE id = $1")
        .bind(set_id)
        .fetch_one(&hub)
        .await
        .expect("server_version після видалення");
    assert!(v2 > v1, "видалення — нова версія: {v1} → {v2}");

    let (code1, d1) = env::master_delta(&hub_base, &token, "settings", 0).await;
    assert_eq!(code1, 200, "{d1}");
    let after = find_change(&d1, &set_id);
    assert_eq!(
        after["op"], "delete",
        "C5: після міграції 0023 видалення їде як op=delete (до неї — завжди upsert): {after}"
    );
    assert_eq!(after["version"].as_i64(), Some(v2));
    assert!(after["data"].is_null(), "delete без payload: {after}");

    // Контроль: живі налаштування точки і далі їдуть як upsert (не зламано).
    let live_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO system_settings \
            (id, module, key, value, value_type, label, is_active, created_at, updated_at, store_id) \
         VALUES ($1, 'e5a', $2, 'v2', 'string', 'E5A live', true, now(), now(), $3)",
    )
    .bind(live_id)
    .bind(format!("e5a.live.{}", &live_id.simple().to_string()[..8]))
    .bind(env::store_id())
    .execute(&hub)
    .await
    .expect("INSERT живого налаштування");
    let (_, d2) = env::master_delta(&hub_base, &token, "settings", 0).await;
    assert_eq!(find_change(&d2, &live_id)["op"], "upsert");
    evidence(&format!(
        "settings: {set_id} v{v1}→v{v2} їде як op=delete; живе налаштування {live_id} — upsert (контроль)"
    ));
}

/// C6: `stock_norms` прибрано з `sync_meta`/`ALLOWED_ENTITIES` — entity
/// відсутня, pull не падає (400 з переліком дозволених, решта сутностей жива).
#[tokio::test]
async fn stock_norms_absent_from_pull() {
    let (hub, _node, _product, hub_base, _node_base) = hub_and_node("dirstock").await;
    let token = env::owner_token();

    // ── Фактичний вміст sync_meta (критерій 5) ────────────────────────────
    // УВАГА до арифметики: у контракті E5 «частина A» очікувалось 7 рядків
    // (5 бізнесових + 2 нових C1), але ADR §7.1-C2 вимагає для
    // `write_off_reasons` не лише `server_version`+`is_deleted`, а й **entity**
    // — тобто ВЛАСНИЙ рядок sync_meta. Тому фактично 8:
    //   5 бізнесових (categories, products, suppliers, employees, settings)
    //   + C1 barcodes + C1 product_images + C2 write_off_reasons
    //   − C6 stock_norms.
    // Розбіжність 7↔8 зафіксована в звіті; рядок C2 обов'язковий: без нього
    // тригер `trg_write_off_reasons_bump` не має куди інкрементувати версію.
    let entities: Vec<String> = sqlx::query_scalar("SELECT entity FROM sync_meta ORDER BY entity")
        .fetch_all(&hub)
        .await
        .expect("sync_meta");
    evidence(&format!(
        "sync_meta після C1/C2/C6 + B5/C3: {} рядків — {}",
        entities.len(),
        entities.join(", ")
    ));
    assert_eq!(
        entities.len(),
        9,
        "5 бізнесових + C1(2) + C2(1) + B5/C3(1) − C6(1): {entities:?}"
    );
    assert_eq!(
        entities,
        vec![
            "barcodes",
            "categories",
            "employees",
            "product_images",
            "products",
            "settings",
            "store_product_prices",
            "suppliers",
            "write_off_reasons"
        ],
        "5 бізнесових + barcodes/product_images (C1) + write_off_reasons (C2) \
         + store_product_prices (E5-B5/C3, ціна мережі); stock_norms прибрано (C6)"
    );
    assert!(
        !entities.iter().any(|e| e == "stock_norms"),
        "C6: рядка stock_norms у sync_meta бути не повинно"
    );

    // ── Pull цієї сутності: 400 (а не 500/паніка), з переліком дозволених ──
    let (code, body) = env::master_delta(&hub_base, &token, "stock_norms", 0).await;
    assert_eq!(
        code, 400,
        "entity 'stock_norms' більше не сутність pull (: {body}"
    );
    let detail = body["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("невідома сутність") && detail.contains("barcodes"),
        "400 з переліком дозволених сутностей: {body}"
    );
    evidence(&format!("pull stock_norms → 400: {detail}"));

    // ── Решта сутностей працює (pull не зламано) ──────────────────────────
    for entity in [
        "products",
        "categories",
        "suppliers",
        "employees",
        "settings",
    ] {
        let (c, d) = env::master_delta(&hub_base, &token, entity, 0).await;
        assert_eq!(c, 200, "pull {entity} живий: {d}");
    }
    evidence("products/categories/suppliers/employees/settings — pull 200 (жодного зламаного)");
}
