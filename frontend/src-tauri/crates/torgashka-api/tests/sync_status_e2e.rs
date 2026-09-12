//! E6 (ADR-0008 §7.2 п.4): `GET /api/v1/sync/status` — лаг, розмір черги,
//! відкриті конфлікти. Моніторинг мусить працювати й тоді, коли хаб НЕ
//! доступний (це його головний сценарій, E4), тому всі величини локальні:
//!   * лаг = вік найстарішого НЕдоставленого агрегата в `hub_outbox`;
//!   * черга = pending/failed у `hub_outbox` точки;
//!   * конфлікти = відкриті пропозиції довідників цієї точки (E5);
//!   * версії довідників = `sync_meta` (докуди дійшли дані).
//!
//! Ретеншн — свідомо НЕ ввімкнено (блокер Б6) і це ВИДНО в API
//! (`retention.configured=false` + причина), а не заховано в TODO.

mod common;

use serde_json::json;
use uuid::Uuid;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;

fn evidence(line: &str) {
    eprintln!("[e6][evidence] {line}");
}

#[tokio::test]
async fn status_reports_lag_queue_conflicts() {
    common::force_test_db();
    let hub = env::hub_pool(&env::pool_to(&env::test_db_url()).await, "status").await;
    let node = env::node_pool(&hub, "status").await;
    let product = Uuid::new_v4();
    env::seed(&hub, product, "status-hub").await;
    env::seed(&node, product, "status-node").await;

    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (node_base, _) = env::serve_any(env::app_state(&node)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&node_base).await;
    let token = env::owner_token();
    env::set_hub_upstream(&node, &hub_base, &token).await;

    // Чистий стан для повторних прогонів у тій самій тестовій БД.
    for q in [
        "DELETE FROM hub_outbox WHERE store_id = $1",
        "DELETE FROM sync_log WHERE store_id = $1 AND direction = 'push'",
    ] {
        sqlx::query(q)
            .bind(env::store_id())
            .execute(&node)
            .await
            .expect("чистка стану вузла");
    }
    sqlx::query("DELETE FROM catalog_change_requests WHERE store_id = $1")
        .bind(env::store_id())
        .execute(&hub)
        .await
        .expect("чистка журналу хаба");

    // ── 1. Хаб лежить → вузол пише локально, черга росте ──────────────────
    let b1 = Uuid::new_v4();
    let b2 = Uuid::new_v4();
    for (batch, note) in [(b1, "E6 офлайн 1"), (b2, "E6 офлайн 2")] {
        let (code, body, _) = env::push(
            &node_base,
            &token,
            batch,
            &[env::receipt_env(Uuid::new_v4(), product, note)],
        )
        .await;
        assert_eq!(code, 200, "чек прийнято локально: {body}");
        assert_eq!(body[0]["status"], "created", "{body}");
    }

    // Черга «відлежалась» — лаг мусить бути ВИМІРЯНИЙ, а не константою.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let (code_st, st) = env::sync_status(&node_base, &token).await;
    assert_eq!(code_st, 200, "GET /api/v1/sync/status: {st}");
    assert_eq!(st["role"], "node", "вузол бачить свою роль: {st}");
    assert_eq!(st["hub_url"], hub_base, "{st}");
    assert_eq!(
        st["queue"]["pending"], 2,
        "черга форвардингу показує 2 недоставлені агрегати: {st}"
    );
    assert_eq!(st["queue"]["failed"], 0, "{st}");
    assert!(
        st["lag_seconds"].as_i64().unwrap_or(-1) >= 1,
        "лаг = вік найстарішого недоставленого агрегата (виміряний, не константа): {st}"
    );
    assert!(
        st["queue"]["oldest_pending_at"].is_string(),
        "вік найстарішого недоставленого видно: {st}"
    );
    assert_eq!(st["conflicts"]["open"], 0, "конфліктів ще немає: {st}");
    assert!(
        st["versions"]
            .as_array()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        "версії довідників (sync_meta) присутні: {st}"
    );
    assert_eq!(
        st["retention"]["configured"], false,
        "ретеншн свідомо не ввімкнено (блокер Б6): {st}"
    );
    assert!(
        st["retention"]["blocker"]
            .as_str()
            .unwrap_or_default()
            .contains("Б6"),
        "причина видима в API: {st}"
    );
    evidence(&format!(
        "вузол (хаб лежить): role={}, queue.pending={}, lag_seconds={}, conflicts.open={}, retention.blocker=Б6",
        st["role"], st["queue"]["pending"], st["lag_seconds"], st["conflicts"]["open"]
    ));

    // ── 2. Конфлікти на хабі: дві пропозиції на один рядок ────────────────
    let row_id = Uuid::new_v4();
    let (c1, body1) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            row_id,
            "upsert",
            Uuid::new_v4(),
            json!({"name": "E6 конфлікт A", "price": "10.00"}),
            0,
        )],
    )
    .await;
    assert_eq!(body1["results"][0]["status"], "accepted", "{c1}: {body1}");
    let (c2, body2) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "products",
            row_id,
            "upsert",
            Uuid::new_v4(),
            json!({"name": "E6 конфлікт B", "price": "12.00"}),
            0,
        )],
    )
    .await;
    assert_eq!(body2["results"][0]["status"], "conflict", "{c2}: {body2}");

    let (code_h, hub_st) = env::sync_status(&hub_base, &token).await;
    assert_eq!(code_h, 200, "{hub_st}");
    assert_eq!(
        hub_st["role"], "hub",
        "інстанс без апстріму — хаб: {hub_st}"
    );
    assert_eq!(
        hub_st["conflicts"]["open"], 2,
        "дві сторони конфлікту видно в стані хаба: {hub_st}"
    );
    assert_eq!(
        hub_st["queue"]["pending"], 0,
        "хаб НЕ накопичує чергу форвардингу (він і є гора): {hub_st}"
    );
    evidence(&format!(
        "хаб: role={}, conflicts.open={}, queue.pending={}",
        hub_st["role"], hub_st["conflicts"]["open"], hub_st["queue"]["pending"]
    ));

    // ── 3. Хаб піднято → черга доїжджає, лаг падає до нуля ────────────────
    let summary = torgashka_api::hub_forwarder::forward_pending(
        &torgashka_infrastructure::store_ctx::StorePool::new(node.clone()),
        &reqwest::Client::new(),
        &torgashka_api::hub_forwarder::HubForwardConfig::from_pool(&node)
            .await
            .expect("налаштування")
            .expect("вузол налаштований"),
        torgashka_api::hub_forwarder::BATCHES_PER_CYCLE,
    )
    .await
    .expect("цикл форвардингу");
    assert_eq!(summary.accepted, 2, "{summary:?}");

    let (_, st_after) = env::sync_status(&node_base, &token).await;
    assert_eq!(
        st_after["queue"]["pending"], 0,
        "після доставки черга порожня: {st_after}"
    );
    assert_eq!(
        st_after["lag_seconds"], 0,
        "порожня черга = нульовий лаг: {st_after}"
    );
    evidence(&format!(
        "після форвардингу: queue.pending={}, lag_seconds={}",
        st_after["queue"]["pending"], st_after["lag_seconds"]
    ));

    // ── 4. Стан — у скоупі точки (без X-Store-Id → 400, не «вся мережа») ──
    let r = reqwest::Client::new()
        .get(format!("{node_base}/api/v1/sync/status"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("запит без X-Store-Id");
    assert_eq!(
        r.status().as_u16(),
        400,
        "без скоупу точки стан не віддається"
    );
    evidence("без X-Store-Id → 400 (агрегати всієї мережі не течуть)");
}
