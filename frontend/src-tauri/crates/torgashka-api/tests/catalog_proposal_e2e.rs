//! E5 (ADR-0008 §4.2 «hub-as-authority», §7.2 п.3/п.5): ПРОПОЗИЦІЯ з вузла →
//! арбітраж хабa → ЄДИНИЙ `server_version` → роздача вузлам НАЯВНИМ pull.
//!
//! Що доводиться (дефект «вузол не може писати каталог, а якщо пише — назавжди
//! локально»):
//!   1. `proposal_accepted_and_visible_on_second_node` — товар, створений на
//!      вузлі, хаб приймає як КАНОНІЧНИЙ: присвоює єдину версію наявним
//!      тригером (`sync_meta.products`), і ДРУГИЙ вузол бачить його через
//!      наявний `GET /api/v1/sync/master` (жодного нового каналу);
//!   2. `delete_proposal_sets_tombstone_and_visible_as_delete` — `op=delete`
//!      ставить tombstone `is_deleted=true` (механізм §4.2 п.3), і вузли
//!      отримують `op=delete` тією ж дельтою;
//!   3. `conflict_visible_in_admin_queue` — дві пропозиції на ТОЙ САМИЙ рядок:
//!      ОБИДВІ лишаються в журналі зі `status='conflict'`, видно в черзі
//!      оператора `/api/v1/admin/sync/conflicts`, і — головне — ЖОДНА не
//!      «злита мовчки»: до довідника не застосовано нічого нового.
//!
//!
//! Другий вузол тут — ОКРЕМИЙ інстанс (власна БД + власний фасад + апстрім на
//! хаб): дельту він тягне з хаба, як у проді. Застосування дельти у локальну
//! БД вузла — клієнтська частина (Tauri `offline/sync_pull.rs`), поза
//! крейтом API; тому видимість перевіряється на каналі хаба, який вузол тягне.

mod common;

use serde_json::json;
use uuid::Uuid;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;

fn evidence(line: &str) {
    eprintln!("[e5][evidence] {line}");
}

/// Три інстанси в одному тесті: хаб + вузол-автор + ДРУГИЙ вузол (споживач).
async fn three_instances(tag: &str) -> (sqlx::PgPool, sqlx::PgPool, sqlx::PgPool) {
    common::force_test_db();
    let hub = env::hub_pool(&env::pool_to(&env::test_db_url()).await, tag).await;
    let node1 = env::node_pool(&hub, &format!("{tag}1")).await;
    let node2 = env::node_pool(&hub, &format!("{tag}2")).await;
    let seed_product = Uuid::new_v4();
    env::seed(&hub, seed_product, &format!("{tag}-hub")).await;
    env::seed(&node1, seed_product, &format!("{tag}-n1")).await;
    env::seed(&node2, seed_product, &format!("{tag}-n2")).await;
    (hub, node1, node2)
}

#[tokio::test]
async fn proposal_accepted_and_visible_on_second_node() {
    let (hub, node1, node2) = three_instances("prop").await;

    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (n1_base, _) = env::serve_any(env::app_state(&node1)).await;
    let (n2_base, _) = env::serve_any(env::app_state(&node2)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&n1_base).await;
    env::wait_ready(&n2_base).await;

    let token = env::owner_token();
    // Обидва вузли знають свій апстрім — саме це робить їх вузлами.
    env::set_hub_upstream(&node1, &hub_base, &token).await;
    env::set_hub_upstream(&node2, &hub_base, &token).await;

    // ── 1. Вузол 1 (менеджер точки) створює товар → пропозиція хабові ─────
    let row_id = Uuid::new_v4();
    let cu = Uuid::new_v4();
    // Штрих-код унікальний на прогін: тестова БД спільна й переживає прогони
    // (`ix_products_barcode` UNIQUE), а повторний прогін не має падати.
    let barcode = format!("4820000{}", &row_id.simple().to_string()[..6]);
    let payload = json!({
        "name": "Кава E5 (вузол)",
        "barcode": barcode,
        "price": "185.00",
        "unit": "шт",
        "tax_rate": "20.00",
        "is_weight": false,
    });
    let (code, body) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            row_id,
            "upsert",
            cu,
            payload.clone(),
            0,
        )],
    )
    .await;
    assert_eq!(code, 200, "вузол → хаб: HTTP {code}, тіло {body}");
    assert_eq!(
        body["results"][0]["status"], "accepted",
        "хаб приймає пропозицію як канонічну: {body}"
    );
    let version = body["results"][0]["server_version"]
        .as_i64()
        .unwrap_or_else(|| panic!("хаб мусить присвоїти server_version: {body}"));
    assert!(version > 0, "версія присвоєна наявним тригером: {version}");

    // ── 2. Хаб: ЄДИНИЙ server_version (те саме джерело, що bump_sync_version) ─
    let (title, hub_version, is_deleted): (String, i64, bool) =
        sqlx::query_as("SELECT title, server_version, is_deleted FROM products WHERE id = $1")
            .bind(row_id)
            .fetch_one(&hub)
            .await
            .expect("товар на хабі");
    assert_eq!(title, "Кава E5 (вузол)");
    assert_eq!(hub_version, version, "версія рядка = версія з відповіді");
    assert!(!is_deleted);
    let meta: i64 = sqlx::query_scalar("SELECT version FROM sync_meta WHERE entity = 'products'")
        .fetch_one(&hub)
        .await
        .expect("sync_meta.products");
    assert_eq!(
        meta, version,
        "присвоєна версія = sync_meta.version (одне джерело канонічної версії)"
    );
    let (status, sv): (String, Option<i64>) = sqlx::query_as(
        "SELECT status, server_version FROM catalog_change_requests WHERE client_uuid = $1",
    )
    .bind(cu)
    .fetch_one(&hub)
    .await
    .expect("журнал пропозицій");
    assert_eq!(status, "accepted", "пропозиція в журналі — accepted");
    assert_eq!(sv, Some(version));
    evidence(&format!(
        "вузол→хаб: товар {row_id} прийнято, server_version={version} (= sync_meta.products), журнал accepted"
    ));

    // ── 3. ДРУГИЙ вузол — окремий інстанс із власною БД і апстрімом ────────
    let (code_st, st2) = env::sync_status(&n2_base, &token).await;
    assert_eq!(code_st, 200, "status другого вузла: {st2}");
    assert_eq!(
        st2["role"], "node",
        "другий вузол бачить себе вузлом: {st2}"
    );

    // ── 4. Роздача НАЯВНИМ pull-каналом: дельта хаба містить правку ────────
    let (code_d, delta) = env::master_delta(&hub_base, &token, "products", 0).await;
    assert_eq!(code_d, 200, "GET /api/v1/sync/master: {delta}");
    let changes = delta["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("дельта без changes: {delta}"));
    let found = changes
        .iter()
        .find(|c| c["id"] == row_id.to_string())
        .unwrap_or_else(|| panic!("правки немає в дельті хаба: {delta}"));
    assert_eq!(found["op"], "upsert");
    assert_eq!(
        found["version"].as_i64(),
        Some(version),
        "вузол бачить ТУ САМУ канонічну версію: {found}"
    );
    assert_eq!(found["data"]["name"], "Кава E5 (вузол)");
    // numeric(10,2) у PG може повернутись без хвостових нулів («185») —
    // звіряємо ЧИСЛО, а не рядкову форму.
    assert_eq!(
        found["data"]["price"]
            .as_str()
            .and_then(|p| p.parse::<f64>().ok()),
        Some(185.0),
        "ціна в дельті: {found}"
    );
    evidence(&format!(
        "другий вузол через наявний pull хаба: у дельті products є {row_id} з version={version} і даними пропозиції"
    ));

    // ── 5. Повторний pull із version уже отриманого — правка не дублюється ──
    let (_, again) = env::master_delta(&hub_base, &token, "products", version).await;
    let redelivered = again["changes"]
        .as_array()
        .map(|cs| cs.iter().any(|c| c["id"] == row_id.to_string()))
        .unwrap_or(true);
    assert!(
        !redelivered,
        "дельта не передручає вже отриману версію: {again}"
    );
    evidence("повторний pull із since_version=присвоєна версія → правка не дублюється");

    // ── 6. Повторна доставка ТІЄЇ САМОЇ пропозиції — ідемпотентно ─────────
    let (code_id, body_id) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products", row_id, "upsert", cu, payload, 0,
        )],
    )
    .await;
    assert_eq!(code_id, 200);
    assert_eq!(body_id["results"][0]["status"], "accepted");
    assert_eq!(
        body_id["results"][0]["server_version"].as_i64(),
        Some(version),
        "повтор віддає РАНІШЕ рішення, а не другу правку: {body_id}"
    );
    let rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM catalog_change_requests WHERE client_uuid = $1")
            .bind(cu)
            .fetch_one(&hub)
            .await
            .expect("журнал");
    assert_eq!(rows, 1, "повтор не створив другої пропозиції");
    evidence("повторна доставка тієї самої пропозиції: 1 рядок у журналі, та сама версія");

    // ── 7. Поле поза арбітрованою поверхнею — ВІДМОВА, а не тихе зникнення ──
    let (code_u, body_u) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            Uuid::new_v4(),
            "upsert",
            Uuid::new_v4(),
            json!({"name": "Товар з друкарською помилкою", "prise": "10.00"}),
            0,
        )],
    )
    .await;
    assert_eq!(code_u, 200);
    assert_eq!(
        body_u["results"][0]["status"], "rejected",
        "невідоме поле = відмова (вузол мусить знати, що не дійшло): {body_u}"
    );
    assert!(
        body_u["results"][0]["note"]
            .as_str()
            .unwrap_or_default()
            .contains("prise"),
        "у відмові названо поле: {body_u}"
    );
    evidence("payload із полем поза поверхнею → rejected із переліком полів");
}

#[tokio::test]
async fn delete_proposal_sets_tombstone_and_visible_as_delete() {
    let (hub, node1, _node2) = three_instances("propdel").await;
    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (n1_base, _) = env::serve_any(env::app_state(&node1)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&n1_base).await;
    let token = env::owner_token();
    env::set_hub_upstream(&node1, &hub_base, &token).await;

    // Товар, який видалятиме вузол: канонічний, створений хабом (як у проді).
    let row_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, title, price, tax_rate) VALUES ($1, 'Списання E5', 10.00, 20.00)",
    )
    .bind(row_id)
    .execute(&hub)
    .await
    .expect("товар на хабі");

    let cu = Uuid::new_v4();
    let (code, body) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            row_id,
            "delete",
            cu,
            json!({}),
            0,
        )],
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["results"][0]["status"], "accepted", "{body}");

    let (is_deleted, version): (bool, i64) =
        sqlx::query_as("SELECT is_deleted, server_version FROM products WHERE id = $1")
            .bind(row_id)
            .fetch_one(&hub)
            .await
            .expect("товар на хабі");
    assert!(is_deleted, "op=delete ставить tombstone is_deleted=true");
    assert!(version > 0, "tombstone теж отримав канонічну версію");

    // Вузли отримують видалення ТІЄЮ Ж дельтою (op=delete, data=null).
    let (_, delta) = env::master_delta(&hub_base, &token, "products", 0).await;
    let change = delta["changes"]
        .as_array()
        .and_then(|cs| cs.iter().find(|c| c["id"] == row_id.to_string()))
        .unwrap_or_else(|| panic!("tombstone немає в дельті: {delta}"));
    assert_eq!(change["op"], "delete");
    assert_eq!(change["version"].as_i64(), Some(version));
    assert!(change["data"].is_null(), "delete без data: {change}");
    evidence(&format!(
        "op=delete: tombstone is_deleted=true, version={version}, вузли бачать op=delete тією ж дельтою"
    ));

    // Пропозиція видалення НЕІСНУЮЧОГО рядка — чесна відмова, не тихий accepted.
    let cu2 = Uuid::new_v4();
    let (_, body2) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            Uuid::new_v4(),
            "delete",
            cu2,
            json!({}),
            0,
        )],
    )
    .await;
    assert_eq!(body2["results"][0]["status"], "rejected", "{body2}");
    assert!(
        body2["results"][0]["note"]
            .as_str()
            .unwrap_or_default()
            .contains("немає"),
        "причина відмови видима: {body2}"
    );
    evidence("delete неіснуючого рядка → rejected із причиною (жодного тихого accepted)");
}

#[tokio::test]
async fn conflict_visible_in_admin_queue() {
    let (hub, node1, node2) = three_instances("propcf").await;
    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (n1_base, _) = env::serve_any(env::app_state(&node1)).await;
    let (n2_base, _) = env::serve_any(env::app_state(&node2)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&n1_base).await;
    env::wait_ready(&n2_base).await;
    let token = env::owner_token();
    env::set_hub_upstream(&node1, &hub_base, &token).await;
    env::set_hub_upstream(&node2, &hub_base, &token).await;

    // Чистий журнал для повторних прогонів (той самий store_id у тестовій БД).
    sqlx::query("DELETE FROM catalog_change_requests WHERE store_id = $1")
        .bind(env::store_id())
        .execute(&hub)
        .await
        .expect("чистка журналу");

    let row_id = Uuid::new_v4();
    let cu1 = Uuid::new_v4();
    let cu2 = Uuid::new_v4();
    let payload1 = json!({"name": "Ціна від вузла 1", "price": "100.00"});
    let payload2 = json!({"name": "Ціна від вузла 2", "price": "120.00"});

    // ── Вузол 1: пропозиція прийнята (канонічне значення = його) ──────────
    let (code1, body1) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            row_id,
            "upsert",
            cu1,
            payload1.clone(),
            0,
        )],
    )
    .await;
    assert_eq!(code1, 200, "{body1}");
    assert_eq!(body1["results"][0]["status"], "accepted", "{body1}");
    let v1 = body1["results"][0]["server_version"].as_i64().unwrap();

    // ── Вузол 2: ТА САМИЙ рядок, та сама базова версія → КОНФЛІКТ ─────────
    let (code2, body2) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            row_id,
            "upsert",
            cu2,
            payload2.clone(),
            0,
        )],
    )
    .await;
    assert_eq!(code2, 200, "конфлікт — не HTTP-помилка: {body2}");
    assert_eq!(
        body2["results"][0]["status"], "conflict",
        "друга пропозиція на той самий рядок = конфлікт: {body2}"
    );
    assert!(
        body2["results"][0]["server_version"].is_null(),
        "конфліктна пропозиція не отримує версії (нічого не застосовано): {body2}"
    );

    // ── КРИТЕРІЙ 5: ОБИДВІ пропозиції в журналі зі status='conflict' ──────
    let rows: Vec<(Uuid, String, Option<i64>)> = sqlx::query_as(
        "SELECT client_uuid, status, server_version FROM catalog_change_requests \
         WHERE entity = 'products' AND row_id = $1 ORDER BY id",
    )
    .bind(row_id)
    .fetch_all(&hub)
    .await
    .expect("журнал конфлікту");
    assert_eq!(
        rows.len(),
        2,
        "обидві пропозиції лишились у журналі: {rows:?}"
    );
    for (cu, status, _) in &rows {
        assert_eq!(
            status, "conflict",
            "пропозиція {cu} має status=conflict (нічого не злито мовчки)"
        );
    }
    let cus: Vec<Uuid> = rows.iter().map(|(cu, _, _)| *cu).collect();
    assert!(cus.contains(&cu1) && cus.contains(&cu2), "{rows:?}");
    evidence(&format!(
        "журнал (жодного злиття): {}",
        rows.iter()
            .map(|(cu, st, _)| format!("{cu}={st}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    // ── НІЧОГО не застосовано: канонічним лишився payload вузла 1 ─────────
    let (title, price, version_now): (String, String, i64) =
        sqlx::query_as("SELECT title, price::text, server_version FROM products WHERE id = $1")
            .bind(row_id)
            .fetch_one(&hub)
            .await
            .expect("рядок довідника");
    assert_eq!(
        title, "Ціна від вузла 1",
        "друга пропозиція НЕ перезаписала рядок (немає last-write-wins)"
    );
    assert_eq!(price, "100.00");
    assert_eq!(version_now, v1, "версія рядка не змінилась конфліктом");

    // ── Черга оператора: обидві сторони видно ────────────────────────────
    let (code_q, queue) = env::admin_conflicts(&hub_base, &token).await;
    assert_eq!(code_q, 200, "GET /api/v1/admin/sync/conflicts: {queue}");
    assert!(
        queue["count"].as_u64().unwrap_or(0) >= 2,
        "у черзі щонайменше 2 пропозиції: {queue}"
    );
    let group = queue["groups"]
        .as_array()
        .and_then(|gs| gs.iter().find(|g| g["row_id"] == row_id.to_string()))
        .unwrap_or_else(|| panic!("немає групи конфлікту для {row_id}: {queue}"));
    let props = group["proposals"].as_array().unwrap();
    assert_eq!(props.len(), 2, "обидві сторони конфлікту в черзі: {group}");
    let queue_cus: Vec<String> = props
        .iter()
        .map(|p| p["client_uuid"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(queue_cus.contains(&cu1.to_string()) && queue_cus.contains(&cu2.to_string()));
    assert!(
        group["recommended"]["client_uuid"].is_string(),
        "детермінована рекомендація правила ADR §4.2 п.5 присутня: {group}"
    );
    assert!(
        queue["rule"]
            .as_str()
            .unwrap_or_default()
            .contains("base_version"),
        "правило видиме в API: {queue}"
    );
    assert!(
        queue["resolution"]
            .as_str()
            .unwrap_or_default()
            .contains("оператора"),
        "рішення — за оператором (автозлиття немає): {queue}"
    );
    evidence(&format!(
        "черга оператора: count={}, група {row_id} = 2 пропозиції, recommended={}, rule={}",
        queue["count"], group["recommended"]["client_uuid"], queue["rule"]
    ));

    // ── Черга конфліктів — лише admin|owner|store_manager ────────────────
    let cashier = torgashka_api::auth::create_access_token(
        &env::owner_id().to_string(),
        "cashier",
        &[],
        env::SECRET,
    )
    .expect("JWT касира");
    let (code_403, _) = env::admin_conflicts(&hub_base, &cashier).await;
    assert_eq!(code_403, 403, "касир не бачить черги конфліктів");
    evidence("доступ до черги конфліктів: касир → 403 (лише admin|owner|store_manager)");
}

// ─────────────────────────────────────────────────────────────────────────────
// E5-частина B (Б1/B5/C3): ціна точки як СПІЛЬНА сутність МЕРЕЖІ
// ─────────────────────────────────────────────────────────────────────────────

/// `store_price_proposal_accepted_and_visible_on_second_node` (критерій E5-Б1).
///
/// Рішення Творця Б1 (2026-09-12, ADR §10 №1): ціна товару в точці —
/// атрибут МЕРЕЖІ; хаб — авторитет, арбітрує ЄДИНИЙ `server_version` і роздає
/// всім вузлам (механізм той самий, що `products.price`).
///
/// Що доводиться:
///   1. вузол створив ціну ЛОКАЛЬНО (offline-first, рядок у своїй БД) і надіслав
///      ПРОПОЗИЦІЮ — жодного нового ендпоінта/kind (наявний арбітраж §4.2);
///   2. хаб присвоїв ЄДИНИЙ `server_version` (наявний тригер + `sync_meta`),
///      і той самий номер ДРУГИЙ вузол бачить у дельті `store_product_prices`;
///   3. два вузли незалежно створили ціну для ОДНІЄЇ пари `(store, product)`:
///      хаб застосовує правку до КАНОНІЧНОГО рядка (природний ключ) і каже про
///      це вузлу — жодного другого рядка (UNIQUE) і жодної втраченої правки;
///   4. `op=delete` їде як `op=delete` (tombstone, ADR §4.2 п.3).
#[tokio::test]
async fn store_price_proposal_accepted_and_visible_on_second_node() {
    common::force_test_db();
    let admin = env::pool_to(&env::test_db_url()).await;
    let hub = env::hub_pool(&admin, "priceB").await;
    let node1 = env::node_pool(&hub, "priceB1").await;
    let node2 = env::node_pool(&hub, "priceB2").await;
    let product = Uuid::new_v4();
    env::seed(&hub, product, "priceB-hub").await;
    env::seed(&node1, product, "priceB-n1").await;
    env::seed(&node2, product, "priceB-n2").await;

    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (n1_base, _) = env::serve_any(env::app_state(&node1)).await;
    let (n2_base, _) = env::serve_any(env::app_state(&node2)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&n1_base).await;
    env::wait_ready(&n2_base).await;

    let token = env::owner_token();
    env::set_hub_upstream(&node1, &hub_base, &token).await;
    env::set_hub_upstream(&node2, &hub_base, &token).await;

    // ── 1. Вузол 1 створює ціну ЛОКАЛЬНО (offline-first) ──────────────────
    // Це і є «менеджер точки поставив мережеву ціну»: жодного звернення до
    // хаба, рядок живе у власній БД вузла і несе локальний `server_version`.
    let price1 = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO store_product_prices (id, store_id, product_id, price) \
         VALUES ($1, $2, $3, $4::numeric)",
    )
    .bind(price1)
    .bind(env::store_id())
    .bind(product)
    .bind("99.50")
    .execute(&node1)
    .await
    .expect("локальна ціна на вузлі 1");
    let local_version: i64 =
        sqlx::query_scalar("SELECT server_version FROM store_product_prices WHERE id = $1")
            .bind(price1)
            .fetch_one(&node1)
            .await
            .expect("локальний рядок ціни");
    assert!(
        local_version > 0,
        "локальний тригер bump працює на вузлі: {local_version}"
    );

    // ── 2. Пропозиція хабові (НАЯВНИЙ арбітраж §4.2, без нового kind) ──────
    let cu1 = Uuid::new_v4();
    let payload1 = json!({
        "store_id": env::store_id(),
        "product_id": product,
        "price": "99.50",
    });
    let (code1, body1) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "store_product_prices",
            price1,
            "upsert",
            cu1,
            payload1,
            0,
        )],
    )
    .await;
    assert_eq!(code1, 200, "вузол → хаб (пропозиція ціни): {body1}");
    assert_eq!(
        body1["results"][0]["status"], "accepted",
        "хаб приймає ціну як КАНОНІЧНУ (рішення Б1): {body1}"
    );
    let version = body1["results"][0]["server_version"]
        .as_i64()
        .unwrap_or_else(|| panic!("хаб мусить присвоїти server_version: {body1}"));

    // ── 3. Хаб: ЄДИНИЙ server_version + одиниця виміру `sync_meta` ─────────
    let (hub_id, price_text, hub_deleted, hub_version): (Uuid, String, bool, i64) = sqlx::query_as(
        "SELECT id, price::text, is_deleted, server_version FROM store_product_prices \
         WHERE id = $1",
    )
    .bind(price1)
    .fetch_one(&hub)
    .await
    .expect("рядок ціни на хабі");
    assert_eq!(hub_id, price1);
    assert_eq!(price_text, "99.50", "ціна з пропозиції стала канонічною");
    assert!(!hub_deleted);
    assert_eq!(hub_version, version, "версія рядка = версія з відповіді");
    let meta: i64 =
        sqlx::query_scalar("SELECT version FROM sync_meta WHERE entity = 'store_product_prices'")
            .fetch_one(&hub)
            .await
            .expect("sync_meta.store_product_prices");
    assert_eq!(
        meta, version,
        "присвоєна версія = sync_meta.version (одне джерело канонічної версії)"
    );
    evidence(&format!(
        "вузол1→хаб: ціна {price1} ({product}) прийнята, server_version={version} \
         (= sync_meta.store_product_prices)"
    ));

    // ── 4. Другий вузол бачить ціну ТІЄЮ Ж дельтою (наявний pull) ──────────
    let (code_st, st2) = env::sync_status(&n2_base, &token).await;
    assert_eq!(code_st, 200, "status другого вузла: {st2}");
    assert_eq!(st2["role"], "node", "другий вузол — окремий інстанс: {st2}");

    let (code_d, delta) = env::master_delta(&hub_base, &token, "store_product_prices", 0).await;
    assert_eq!(code_d, 200, "GET /api/v1/sync/master: {delta}");
    let found = delta["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("дельта без changes: {delta}"))
        .iter()
        .find(|c| c["id"] == price1.to_string())
        .unwrap_or_else(|| panic!("ціни немає в дельті хаба: {delta}"));
    assert_eq!(found["op"], "upsert");
    assert_eq!(found["version"].as_i64(), Some(version));
    assert_eq!(
        found["data"]["price"]
            .as_str()
            .and_then(|p| p.parse::<f64>().ok()),
        Some(99.5),
        "ціна в дельті: {found}"
    );
    assert_eq!(
        found["data"]["product_id"],
        product.to_string(),
        "вузол бачить, до якого товару ціна: {found}"
    );
    evidence(&format!(
        "другий вузол через наявний pull хаба: у дельті store_product_prices є {price1} \
         з version={version} і ціною 99.50"
    ));

    // ── 5. Два вузли, ОДНА пара (store, product): канонікалізація ──────────
    // Вузол 2 теж створив свою ціну локально (свій uuid!) для тієї ж пари —
    // саме так виникає реальна гонка: UNIQUE у схемі на (store_id, product_id).
    let price2 = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO store_product_prices (id, store_id, product_id, price) \
         VALUES ($1, $2, $3, $4::numeric)",
    )
    .bind(price2)
    .bind(env::store_id())
    .bind(product)
    .bind("105.00")
    .execute(&node2)
    .await
    .expect("локальна ціна на вузлі 2");

    let cu2 = Uuid::new_v4();
    let (code2, body2) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "store_product_prices",
            price2,
            "upsert",
            cu2,
            json!({
                "store_id": env::store_id(),
                "product_id": product,
                "price": "105.00",
            }),
            0,
        )],
    )
    .await;
    assert_eq!(code2, 200, "друга пропозиція ціни: {body2}");
    assert_eq!(
        body2["results"][0]["status"], "accepted",
        "правка вузла2 не втрачається: {body2}"
    );
    let note = body2["results"][0]["note"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        note.contains(&price1.to_string()),
        "вузол2 бачить, що застосовано до КАНОНІЧНОГО рядка {price1}: {body2}"
    );
    let rows_for_pair: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM store_product_prices WHERE store_id = $1 AND product_id = $2",
    )
    .bind(env::store_id())
    .bind(product)
    .fetch_one(&hub)
    .await
    .expect("рядки пари на хабі");
    assert_eq!(
        rows_for_pair, 1,
        "другого рядка для пари (store, product) немає — хаб канонікалізував"
    );
    let canonical_price: String =
        sqlx::query_scalar("SELECT price::text FROM store_product_prices WHERE id = $1")
            .bind(price1)
            .fetch_one(&hub)
            .await
            .expect("канонічна ціна");
    assert_eq!(canonical_price, "105.00", "канонічна ціна оновлена");
    evidence(&format!(
        "два вузли, одна пара (store,product): хаб застосував правку до канонічного {price1} \
         (note вузлу2 містить його id), рядків для пари = {rows_for_pair}"
    ));

    // ── 6. Інформативно: послідовна правка ТОГО САМОГО рядка ──────────────
    // Наявне правило E5-ядра (частина A): конкурент — будь-яка інша НЕвирішена
    // АБО вже застосована (`accepted`) пропозиція на той самий рядок. Тому друга
    // правка того самого рядка дає `conflict` і чекає оператора. Для мережевої
    // ціни (Б1) це питання рішення Творця → ескаловано у звіті; тут ЛИШЕ
    // фіксуємо фактичну поведінку (без ассерту, щоб не «узаконити» її).
    let (code_seq, body_seq) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "store_product_prices",
            price1,
            "upsert",
            Uuid::new_v4(),
            json!({
                "store_id": env::store_id(),
                "product_id": product,
                "price": "97.00",
            }),
            version,
        )],
    )
    .await;
    assert_eq!(
        code_seq, 200,
        "послідовна правка того самого рядка: {body_seq}"
    );
    evidence(&format!(
        "послідовна правка того самого рядка {price1} → status={} (наявне правило E5-ядра: \
         прийнята пропозиція лишається конкурентом; рішення Творця потрібне для Б1)",
        body_seq["results"][0]["status"].as_str().unwrap_or("?")
    ));

    // ── 7. Tombstone: зняте перевизначення їде як op=delete ────────────────
    // Рядок УЖЕ канонічний на хабі (його створив адмін хаба/інший вузол
    // раніше) і це ПЕРША пропозиція на нього — інакше крок перевіряв би не
    // tombstone, а правило конкурентів вище.
    let product2 = Uuid::new_v4();
    env::seed(&hub, product2, "priceB-hub-2").await;
    env::seed(&node1, product2, "priceB-n1-2").await;
    let price_del = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO store_product_prices (id, store_id, product_id, price) \
         VALUES ($1, $2, $3, $4::numeric)",
    )
    .bind(price_del)
    .bind(env::store_id())
    .bind(product2)
    .bind("42.00")
    .execute(&hub)
    .await
    .expect("канонічна ціна на хабі (предмет зняття)");
    let (code3, body3) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "store_product_prices",
            price_del,
            "delete",
            Uuid::new_v4(),
            json!({"store_id": env::store_id(), "product_id": product2}),
            0,
        )],
    )
    .await;
    assert_eq!(code3, 200, "пропозиція видалення ціни: {body3}");
    assert_eq!(
        body3["results"][0]["status"], "accepted",
        "зняття перевизначення приймається (перша пропозиція на рядок): {body3}"
    );
    let del_version = body3["results"][0]["server_version"]
        .as_i64()
        .unwrap_or_else(|| panic!("версія tombstone: {body3}"));
    let (code_t, tomb) =
        env::master_delta(&hub_base, &token, "store_product_prices", del_version - 1).await;
    assert_eq!(code_t, 200, "дельта після видалення: {tomb}");
    let tomb_change = tomb["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("дельта без changes: {tomb}"))
        .iter()
        .find(|c| c["id"] == price_del.to_string())
        .unwrap_or_else(|| panic!("tombstone у дельті: {tomb}"));
    assert_eq!(
        tomb_change["op"], "delete",
        "зняте перевизначення доїжджає вузлам як delete: {tomb_change}"
    );
    evidence(&format!(
        "op=delete: ціну {price_del} знято tombstone'ом (is_deleted=true) і вузли бачать op=delete \
         на version={del_version}"
    ));
}
