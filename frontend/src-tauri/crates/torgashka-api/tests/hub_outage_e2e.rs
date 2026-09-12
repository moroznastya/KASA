//! E4 (ADR-0008 §3, план §4.1): E2E НОВОГО СВІТУ — «хаб недоступний» замість
//! «вузол read-only».
//!
//! У моделі ADR-0007 (фізична реплікація) недоступність СЕРВЕРА означала
//! деградацію ВУЗЛА (історично: read-only репліка й ґейт запису — видалені E7)
//! перехоплюють записи, `/api/v1/setup/status` віддає 503 — точка без мережі
//! не могла навіть завести касира. У моделі ADR-0008 вузол — ПОВНИЙ
//! read-write PG, а недоступний хаб — це лише «дані поїдуть пізніше».
//!
//! Що доводить тест (вузол і хаб — дві БД, два фасади; хаб спершу ЛЕЖИТЬ):
//!   1. хаб недоступний (порт закрито) → вузол ПИШЕ ЛОКАЛЬНО: чек каси
//!      прийнято (`created`), дані в PG вузла;
//!   2. `/api/v1/setup/status` вузла НЕ 503 — вузол не read-only (критерій E4);
//!   3. черга форвардингу РОСТЕ (1 → 2 агрегати), у журналі вузла
//!      `hub_forwarded_at IS NULL` — нічого не загублено й не позначено
//!      доставленим;
//!   4. цикл форвардера проти мертвого хаба: транспортна помилка, черга
//!      ЛИШАЄТЬСЯ pending (жодного мовчазного `failed`);
//!   5. хаб ПІДНЯТО → той самий цикл доставляє ОБИДВА документи: на хабі
//!      `receipts` = 2, `sync_log` має обидва `batch_id` вузла; у вузла
//!      `hub_forwarded_at IS NOT NULL` — «офлайн-точка» стала «точкою, що
//!      синхронізувалась», без жодного read-only.

mod common;

use std::time::Duration;

use torgashka_infrastructure::store_ctx::StorePool;
use uuid::Uuid;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;

fn evidence(line: &str) {
    eprintln!("[e4][evidence] {line}");
}

#[tokio::test]
async fn node_writes_offline_hub_down_then_syncs() {
    common::force_test_db();
    let hub_pool = env::hub_pool(&env::pool_to(&env::test_db_url()).await, "outage").await;
    let node_pool = env::node_pool(&hub_pool, "outage").await;

    let product = Uuid::new_v4();
    env::seed(&hub_pool, product, "outage-hub").await;
    env::seed(&node_pool, product, "outage-node").await;

    // Хаб ЛЕЖИТЬ: порт зарезервовано, але ніхто на ньому не слухає
    // (з'єднання відхиляється — детермінована «мережі немає»).
    let hub_port = env::free_port().await;
    let hub_base = format!("http://127.0.0.1:{hub_port}");

    // Вузол піднято — і тільки він.
    let (node_base, node_port) = env::serve_any(env::app_state(&node_pool)).await;
    env::wait_ready(&node_base).await;
    let token = env::owner_token();
    env::set_hub_upstream(&node_pool, &hub_base, &token).await;
    let cfg = torgashka_api::hub_forwarder::HubForwardConfig::from_pool(&node_pool)
        .await
        .expect("налаштування вузла")
        .expect("вузол налаштований на хаб (який зараз лежить)");
    evidence(&format!(
        "вузол http://127.0.0.1:{node_port} увімкнено; хаб {hub_base} НЕ піднято (з'єднання відхиляється)"
    ));

    // ── 1. Вузол пише локально при недоступному хабі ──────────────────────
    let cu1 = Uuid::new_v4();
    let b1 = Uuid::new_v4();
    let (code1, body1, _) = env::push(
        &node_base,
        &token,
        b1,
        &[env::receipt_env(cu1, product, "E4 офлайн 1")],
    )
    .await;
    assert_eq!(
        code1, 200,
        "вузол приймає чек БЕЗ хаба (не read-only): HTTP {code1}, тіло {body1}"
    );
    assert_eq!(
        body1[0]["status"], "created",
        "чек створено на вузлі: {body1}"
    );

    // ── 2. Критерій E4: вузол не read-only — setup/status НЕ 503 ──────────
    let client = reqwest::Client::new();
    let status_resp = client
        .get(format!("{node_base}/api/v1/setup/status"))
        .send()
        .await
        .expect("setup/status вузла");
    let status_code = status_resp.status().as_u16();
    let status_body: serde_json::Value = status_resp.json().await.unwrap_or_default();
    assert_ne!(
        status_code, 503,
        "вузол не read-only навіть без хаба: setup/status = {status_body}"
    );
    assert_eq!(status_code, 200, "setup/status вузла: {status_body}");
    evidence(&format!(
        "вузол без хаба: /api/v1/setup/status = {status_code} {status_body} (НЕ 503, не read-only)"
    ));

    // ── 3. Черга росте: другий чек при недоступному хабі ──────────────────
    let cu2 = Uuid::new_v4();
    let b2 = Uuid::new_v4();
    let (code2, body2, _) = env::push(
        &node_base,
        &token,
        b2,
        &[env::receipt_env(cu2, product, "E4 офлайн 2")],
    )
    .await;
    assert_eq!(code2, 200, "другий чек: HTTP {code2}, тіло {body2}");
    assert_eq!(body2[0]["status"], "created");

    let own = vec![cu1, cu2];
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM hub_outbox WHERE status = 'pending' AND client_uuid = ANY($1)",
    )
    .bind(&own)
    .fetch_one(&node_pool)
    .await
    .expect("черга вузла");
    assert_eq!(queued, 2, "черга форвардингу виросла до 2 агрегатів");
    let not_sent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log WHERE hub_forwarded_at IS NULL \
         AND direction = 'push' AND status = 'ok' AND client_uuid = ANY($1)",
    )
    .bind(&own)
    .fetch_one(&node_pool)
    .await
    .expect("журнал вузла");
    assert_eq!(not_sent, 2, "жоден документ не позначено доставленим");
    evidence("вузол: 2 чеки локально, черга форвардингу = 2, hub_forwarded_at IS NULL у обох");

    // ── 4. Цикл проти мертвого хаба: помилка, черга не втрачена ───────────
    let err = torgashka_api::hub_forwarder::forward_pending(
        &StorePool::new(node_pool.clone()),
        &client,
        &cfg,
        torgashka_api::hub_forwarder::BATCHES_PER_CYCLE,
    )
    .await
    .expect_err("мертвий хаб мусить дати транспортну помилку");
    assert!(
        err.contains("недоступний") || err.contains("connect"),
        "причина — недоступність хаба: {err}"
    );
    let still_queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM hub_outbox WHERE status = 'pending' AND client_uuid = ANY($1)",
    )
    .bind(&own)
    .fetch_one(&node_pool)
    .await
    .expect("черга вузла");
    assert_eq!(still_queued, 2, "невдалий цикл НЕ втрачає чергу");
    let still_unsent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log WHERE hub_forwarded_at IS NULL \
         AND direction = 'push' AND status = 'ok' AND client_uuid = ANY($1)",
    )
    .bind(&own)
    .fetch_one(&node_pool)
    .await
    .expect("журнал вузла");
    assert_eq!(still_unsent, 2, "без хаба — жодного 'доставлено'");
    evidence(&format!(
        "цикл проти мертвого хаба: {err} — черга лишилась pending (2), статуси не збрехали"
    ));

    // ── 5. Хаба піднято → дані доїжджають ─────────────────────────────────
    let hub_pool_for_facade = hub_pool.clone();
    env::serve_on(hub_port, env::app_state(&hub_pool_for_facade));
    env::wait_ready(&hub_base).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let summary = torgashka_api::hub_forwarder::forward_pending(
        &StorePool::new(node_pool.clone()),
        &client,
        &cfg,
        torgashka_api::hub_forwarder::BATCHES_PER_CYCLE,
    )
    .await
    .expect("цикл після підйому хаба");
    assert_eq!(
        summary.sent, 2,
        "у цикл узято обидва документи: {summary:?}"
    );
    assert_eq!(summary.accepted, 2, "хаб прийняв обидва: {summary:?}");
    assert_eq!(summary.failed, 0, "{summary:?}");
    evidence(&format!(
        "хаб піднято: пакетів={}, агрегатів={}, прийнято={}, відкладено={}, поховано={}",
        summary.batches, summary.sent, summary.accepted, summary.deferred, summary.failed
    ));

    let hub_receipts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM receipts WHERE store_id = $1 AND client_uuid = ANY($2)",
    )
    .bind(env::store_id())
    .bind(vec![cu1, cu2])
    .fetch_one(&hub_pool)
    .await
    .expect("чеки на хабі");
    assert_eq!(hub_receipts, 2, "обидва чеки доїхали в PG хаба");

    let hub_batches: Vec<Uuid> = sqlx::query_scalar(
        "SELECT batch_id FROM sync_log WHERE store_id = $1 AND client_uuid = ANY($2) \
         AND direction = 'push' ORDER BY batch_id",
    )
    .bind(env::store_id())
    .bind(vec![cu1, cu2])
    .fetch_all(&hub_pool)
    .await
    .expect("журнал хаба");
    let mut expected = vec![b1, b2];
    expected.sort();
    assert_eq!(
        hub_batches, expected,
        "на хабі — ОБИДВА batch_id вузла (ідентичність пакетів доїхала)"
    );

    let forwarded_marks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log WHERE store_id = $1 AND client_uuid = ANY($2) \
         AND hub_forwarded_at IS NOT NULL",
    )
    .bind(env::store_id())
    .bind(vec![cu1, cu2])
    .fetch_one(&node_pool)
    .await
    .expect("журнал вузла");
    assert_eq!(
        forwarded_marks, 2,
        "у вузла обидва документи позначені доставленими (hub_forwarded_at)"
    );
    evidence(&format!(
        "після підйому хаба: receipts на хабі = {hub_receipts}, batch_id {b1} + {b2}, у вузла hub_forwarded_at IS NOT NULL × {forwarded_marks}"
    ));
}
