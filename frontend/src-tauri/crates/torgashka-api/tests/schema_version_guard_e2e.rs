//! E9 (ADR-0008 §10 №8, блокер Б5; рішення Творця 2026-09-16 — **варіант A**):
//! протокол сумісності major-версії схеми хаб↔вузол.
//!
//! Що перевіряється (критерії прийняття E9):
//!   1. `push_with_foreign_major_is_rejected` — вузол оголосив чужу major →
//!      батч відхилено ЦІЛКОМ (409), `error_class='VALIDATION'`, у тілі ОБИДВІ
//!      версії, жодного агрегата не прийнято, спроба видима в `sync_batches`;
//!   2. `push_with_matching_major_is_accepted` — та сама major → прийнято
//!      (регресійний захист від хибного блокування);
//!   3. `status_exposes_hub_schema_major` — `/api/v1/sync/status` віддає
//!      `schema_major` хаба (з ЙОГО власної `schema_revision`, не з константи);
//!   4. `unversioned_push_accepted_on_baseline_hub` — вузол БЕЗ заголовка (до
//!      E9) на хабі базової major працює як раніше (жодного регресу парку);
//!   5. `unversioned_push_rejected_after_hub_major_bump` — після несумісного
//!      підняття major хаба той самий вузол дістає ЯВНУ відмову з обома
//!      версіями (`node_major_assumed=true`), а не тихе псування даних;
//!   6. `forwarder_marks_foreign_major_batch_failed_without_retry` — наскрізно:
//!      вузол (його схема — чужа для хаба) форвардить чергу → хаб відхиляє →
//!      агрегат у `hub_outbox` стає `failed` і НЕ крутиться в backoff (це і є
//!      вимога «відмова за версією — не retryable», наявним механізмом E2b).
//!
//! Механізм симуляції розбіжності: major хаба/вузла читається з ЙОГО ВЛАСНОЇ
//! `schema_revision` (варіант A), тому тест просто змінює це значення в БД —
//! жодних env-прапорців (урок дефекту E5: env процес-глобальний і протікає між
//! паралельними тестами одного бінаря).

mod common;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;
use sqlx::PgPool;
use uuid::Uuid;

fn evidence(line: &str) {
    eprintln!("[e9][evidence] {line}");
}

/// Хаб у ВЛАСНІЙ БД тесту (`tag`) + товар у ньому → (пул хаба, база URL).
async fn hub_fixture(tag: &str, product: Uuid) -> (PgPool, String) {
    let admin = env::pool_to(&env::test_db_url()).await;
    let hub = env::hub_pool(&admin, tag).await;
    // БД тестів ЖИВУТЬ між прогонами (idempotent ensure_schema), а цей тест
    // свідомо змінює major — тому на старті вертаємо базову: повторний прогін
    // мусить давати той самий результат.
    sqlx::query("UPDATE schema_revision SET major = 1, minor = 0 WHERE id = 1")
        .execute(&hub)
        .await
        .expect("нормалізація major хаба");
    env::seed(&hub, product, &format!("e9-{tag}")).await;
    let (base, _) = env::serve_any(env::app_state(&hub)).await;
    env::wait_ready(&base).await;
    (hub, base)
}

#[tokio::test]
async fn push_with_foreign_major_is_rejected() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (hub, base) = hub_fixture("majorreject", product).await;
    let token = env::owner_token();

    let client_uuid = Uuid::new_v4();
    let batch = Uuid::new_v4();
    // Хаб — базова major 1; вузол оголошує 2 (несумісна схема).
    let (code, body, _, resp_major) = env::push_e9(
        &base,
        &token,
        batch,
        &[env::receipt_env(client_uuid, product, "E9 чужа major")],
        Some("2"),
    )
    .await;

    assert_eq!(code, 409, "чужа major → 409 Conflict: {body}");
    assert_eq!(
        body["error_class"], "VALIDATION",
        "клас помилки — наявний перелік E2b: {body}"
    );
    assert_eq!(body["schema_major"]["node"], 2, "версія вузла: {body}");
    assert_eq!(body["schema_major"]["hub"], 1, "версія хаба: {body}");
    assert_eq!(body["action"], "update_node", "що робити: {body}");
    assert_eq!(
        body["node_major_assumed"], false,
        "версія оголошена явно: {body}"
    );
    let detail = body["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("вузол=2") && detail.contains("хаб=1"),
        "у тексті обидві версії: {detail}"
    );

    // Нічого не прийнято: чека в БД хаба немає (відмова ДО обробки items).
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid = $1")
        .bind(client_uuid)
        .fetch_one(&hub)
        .await
        .expect("лік чеків хаба");
    assert_eq!(receipts, 0, "жодного агрегата не прийнято");
    let sync_log: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log WHERE store_id = $1 AND client_uuid = $2",
    )
    .bind(env::store_id())
    .bind(client_uuid)
    .fetch_one(&hub)
    .await
    .expect("журнал хаба");
    assert_eq!(sync_log, 0, "sync_log хаба чистий — агрегат не оброблявся");

    // Спроба ВИДИМА (не «загублено»): батч у sync_batches зі статусом failed.
    let (batch_status, items): (String, i32) =
        sqlx::query_as("SELECT status, items FROM sync_batches WHERE id = $1")
            .bind(batch)
            .fetch_one(&hub)
            .await
            .expect("батч відмови мусить бути в журналі");
    assert_eq!(batch_status, "failed", "батч невзятий");
    assert_eq!(items, 1, "кількість агрегатів у батчі зафіксована");
    evidence(&format!(
        "чужа major: батч {batch} відхилено цілком (409, VALIDATION, вузол=2 хаб=1), \
         чеків у хаба 0, sync_log чистий, батч видимий як failed"
    ));
    evidence(&format!(
        "хаб оголосив свою major у відмові: X-Schema-Major={resp_major:?}"
    ));
}

#[tokio::test]
async fn push_with_matching_major_is_accepted() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (hub, base) = hub_fixture("majoraccept", product).await;
    let token = env::owner_token();

    let client_uuid = Uuid::new_v4();
    let (code, body, batch, resp_major) = env::push_e9(
        &base,
        &token,
        Uuid::new_v4(),
        &[env::receipt_env(client_uuid, product, "E9 сумісна major")],
        Some("1"),
    )
    .await;

    assert_eq!(code, 200, "сумісна major → прийнято: {body}");
    assert_eq!(body[0]["status"], "created", "чек створено: {body}");
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid = $1")
        .bind(client_uuid)
        .fetch_one(&hub)
        .await
        .expect("лік чеків хаба");
    assert_eq!(receipts, 1, "чек у БД хаба");
    assert_eq!(
        resp_major.as_deref(),
        Some("1"),
        "хаб оголошує свою major у прийнятій відповіді"
    );
    evidence(&format!(
        "сумісна major (вузол=1, хаб=1): чек {client_uuid} прийнято (created), \
         батч {batch}, X-Schema-Major={resp_major:?} — хибного блокування немає"
    ));
}

#[tokio::test]
async fn status_exposes_hub_schema_major() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (hub, base) = hub_fixture("majorstatus", product).await;
    let token = env::owner_token();

    let (code, st) = env::sync_status(&base, &token).await;
    assert_eq!(code, 200, "GET /api/v1/sync/status: {st}");
    assert_eq!(st["role"], "hub", "інстанс без sync.hub_url — хаб: {st}");
    assert_eq!(st["schema_major"], 1, "хаб бачить свою major: {st}");

    // Версія — ВЛАСНА `schema_revision` інстанса (варіант A: джерело істини —
    // БД, а не константа бінарника). Піднімаємо в БД → видно в статусі.
    sqlx::query("UPDATE schema_revision SET major = 7 WHERE id = 1")
        .execute(&hub)
        .await
        .expect("зміна major хаба (симуляція несумісної міграції)");
    let (code2, st2) = env::sync_status(&base, &token).await;
    assert_eq!(code2, 200, "{st2}");
    assert_eq!(
        st2["schema_major"], 7,
        "статус віддає major хаба з ЙОГО БД: {st2}"
    );

    // Вузол старої схеми тепер дістає явну відмову з обома версіями.
    let (code3, body3, _, _) = env::push_e9(
        &base,
        &token,
        Uuid::new_v4(),
        &[env::receipt_env(Uuid::new_v4(), product, "E9 хаб 7")],
        Some("1"),
    )
    .await;
    assert_eq!(code3, 409, "вузол=1 проти хаба=7: {body3}");
    assert_eq!(body3["schema_major"]["node"], 1, "{body3}");
    assert_eq!(body3["schema_major"]["hub"], 7, "{body3}");
    evidence(
        "статус хаба: schema_major=1 → після зміни в БД schema_major=7; push вузла=1 → 409 з обома версіями",
    );
}

#[tokio::test]
async fn unversioned_push_accepted_on_baseline_hub() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (_hub, base) = hub_fixture("majorassume", product).await;
    let token = env::owner_token();

    // Заголовка НЕМАЄ — так шле вузол, випущений до E9.
    let client_uuid = Uuid::new_v4();
    let (code, body, _, _) = env::push_e9(
        &base,
        &token,
        Uuid::new_v4(),
        &[env::receipt_env(client_uuid, product, "E9 без заголовка")],
        None,
    )
    .await;
    assert_eq!(code, 200, "вузол до E9 на базовій major працює: {body}");
    assert_eq!(body[0]["status"], "created", "{body}");
    evidence("вузол БЕЗ заголовка X-Schema-Major + хаб major=1 → прийнято (жодного регресу парку)");
}

#[tokio::test]
async fn unversioned_push_rejected_after_hub_major_bump() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (hub, base) = hub_fixture("majorbump", product).await;
    let token = env::owner_token();

    // Несумісна міграція хаба: major 1 → 9 (симуляція через БД, варіант A).
    sqlx::query("UPDATE schema_revision SET major = 9 WHERE id = 1")
        .execute(&hub)
        .await
        .expect("підняття major хаба");

    let client_uuid = Uuid::new_v4();
    let (code, body, _, _) = env::push_e9(
        &base,
        &token,
        Uuid::new_v4(),
        &[env::receipt_env(client_uuid, product, "E9 старий вузол")],
        None,
    )
    .await;
    assert_eq!(
        code, 409,
        "старий вузол дістає ЯВНУ відмову, а не тихе псування: {body}"
    );
    assert_eq!(body["error_class"], "VALIDATION", "{body}");
    assert_eq!(
        body["schema_major"]["node"], 1,
        "вузлу приписано major базового протоколу: {body}"
    );
    assert_eq!(body["schema_major"]["hub"], 9, "{body}");
    assert_eq!(
        body["node_major_assumed"], true,
        "видно, що версію вузла приписано (заголовка не було): {body}"
    );

    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid = $1")
        .bind(client_uuid)
        .fetch_one(&hub)
        .await
        .expect("лік чеків");
    assert_eq!(receipts, 0, "нічого не прийнято");
    evidence(
        "хаб major=9 + вузол без заголовка → 409 (node_major_assumed=true), обидві версії в тілі — несумісна міграція більше НЕ ламає вузли мовчки",
    );
}

#[tokio::test]
async fn forwarder_marks_foreign_major_batch_failed_without_retry() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (hub, hub_base) = hub_fixture("majorfwd", product).await;

    let admin = env::pool_to(&env::test_db_url()).await;
    let node = env::node_pool(&admin, "majorfwd_node").await;
    sqlx::query("UPDATE schema_revision SET major = 1, minor = 0 WHERE id = 1")
        .execute(&node)
        .await
        .expect("нормалізація major вузла");
    env::seed(&node, product, "e9-forward").await;
    let (node_base, _) = env::serve_any(env::app_state(&node)).await;
    env::wait_ready(&node_base).await;
    let token = env::owner_token();
    env::set_hub_upstream(&node, &hub_base, &token).await;

    // Вузол приймає чек ЛОКАЛЬНО (каса→вузол) → агрегат стає в чергу форвардингу.
    let client_uuid = Uuid::new_v4();
    let (code, body, _) = env::push(
        &node_base,
        &token,
        Uuid::new_v4(),
        &[env::receipt_env(client_uuid, product, "E9 форвардер")],
    )
    .await;
    assert_eq!(code, 200, "вузол приймає чек: {body}");

    // Несумісна схема ВУЗЛА: вузол оголосить хабу major 2 (хаб — 1).
    sqlx::query("UPDATE schema_revision SET major = 2 WHERE id = 1")
        .execute(&node)
        .await
        .expect("зміна major вузла");

    let cfg = torgashka_api::hub_forwarder::HubForwardConfig::from_pool(&node)
        .await
        .expect("читання налаштувань хаба")
        .expect("вузол мусить мати sync.hub_url");
    let summary = torgashka_api::hub_forwarder::forward_pending(
        &torgashka_infrastructure::store_ctx::StorePool::new(node.clone()),
        &reqwest::Client::new(),
        &cfg,
        torgashka_api::hub_forwarder::BATCHES_PER_CYCLE,
    )
    .await
    .expect("цикл форвардингу");
    assert_eq!(
        summary.failed, 1,
        "відмова за версією → failed: {summary:?}"
    );

    let (status, attempts, error): (String, i32, Option<String>) = sqlx::query_as(
        "SELECT status, attempts, error FROM hub_outbox WHERE store_id = $1 AND client_uuid = $2",
    )
    .bind(env::store_id())
    .bind(client_uuid)
    .fetch_one(&node)
    .await
    .expect("черга вузла");
    assert_eq!(status, "failed", "термінальний статус, не pending");
    assert_eq!(attempts, 1, "рівно одна спроба");
    let error = error.unwrap_or_default();
    assert!(
        error.contains("409") && error.contains("VALIDATION"),
        "причина видима оператору: {error}"
    );

    // Другий цикл: рядок НЕ береться (failed ≠ pending) — жодного backoff-циклу.
    let summary2 = torgashka_api::hub_forwarder::forward_pending(
        &torgashka_infrastructure::store_ctx::StorePool::new(node.clone()),
        &reqwest::Client::new(),
        &cfg,
        torgashka_api::hub_forwarder::BATCHES_PER_CYCLE,
    )
    .await
    .expect("другий цикл");
    assert_eq!(
        summary2.sent, 0,
        "відмовлений батч не надсилається повторно"
    );
    let attempts2: i32 = sqlx::query_scalar(
        "SELECT attempts FROM hub_outbox WHERE store_id = $1 AND client_uuid = $2",
    )
    .bind(env::store_id())
    .bind(client_uuid)
    .fetch_one(&node)
    .await
    .expect("спроби після другого циклу");
    assert_eq!(attempts2, 1, "жодного нескінченного backoff");

    // Хаб нічого не прийняв.
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM receipts WHERE client_uuid = $1")
        .bind(client_uuid)
        .fetch_one(&hub)
        .await
        .expect("лік чеків хаба");
    assert_eq!(receipts, 0, "хаб не прийняв агрегат чужої major");
    evidence(&format!(
        "наскрізно: вузол major=2 → хаб major=1 → 409; hub_outbox=failed, attempts=1, \
         повторний цикл sent=0 (не retryable), у хаба чеків 0; помилка: {error}"
    ));
}

/// Службова перевірка: `/api/v1/sync/status` віддає `schema_major` і на ВУЗЛІ
/// (власна версія), тож вузол порівнює два значення (своє з БД + версію хаба з
/// заголовка/статусу хаба), не вгадуючи.
#[tokio::test]
async fn node_status_reports_its_own_schema_major() {
    common::force_test_db();
    let product = Uuid::new_v4();
    let (hub, hub_base) = hub_fixture("majornodest", product).await;
    let admin = env::pool_to(&env::test_db_url()).await;
    let node = env::node_pool(&admin, "majornodest_node").await;
    sqlx::query("UPDATE schema_revision SET major = 1, minor = 0 WHERE id = 1")
        .execute(&node)
        .await
        .expect("нормалізація major вузла");
    env::seed(&node, product, "e9-node-status").await;
    let (node_base, _) = env::serve_any(env::app_state(&node)).await;
    env::wait_ready(&node_base).await;
    let token = env::owner_token();
    env::set_hub_upstream(&node, &hub_base, &token).await;

    let (code, st) = env::sync_status(&node_base, &token).await;
    assert_eq!(code, 200, "{st}");
    assert_eq!(st["role"], "node", "{st}");
    assert_eq!(st["schema_major"], 1, "вузол бачить свою major: {st}");

    let hub_major: i32 = sqlx::query_scalar("SELECT major FROM schema_revision WHERE id = 1")
        .fetch_one(&hub)
        .await
        .expect("major хаба");
    evidence(&format!(
        "вузол: status.schema_major=1 (власна), хаба major={hub_major} — розбіжність виявляється порівнянням двох полів"
    ));
}
