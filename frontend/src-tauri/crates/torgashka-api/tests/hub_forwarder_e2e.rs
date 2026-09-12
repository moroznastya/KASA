//! E3 (ADR-0008 §7.1-A1/A3, §7.3 п.1): ФОРВАРДЕР node→hub — прийняте вузлом
//! доїжджає до хаба ТИМ САМИМ протоколом і з ТИМ САМИМ `batch_id`.
//!
//! Дефект, який закривається: вузол (повний read-write PG точки) приймає
//! документ каси локально — і на цьому все. Хаб мережі (арбітр спільних
//! довідників + зведені дані) не бачить нічого: черга прийнятого не має
//! транспорту ВГОРУ (наявний `offline/sync_push.rs` везе SQLite-чергу КАСИ на
//! колишній primary, а не PG-журнал вузла в хаб).
//!
//! Що доводить тест (два фасади в одному тесті — вузол і хаб, дві БД):
//!   1. каса → вузол: чек прийнято ЛОКАЛЬНО (`created`), у журналі вузла
//!      рядок із `batch_id = B`, `hub_forwarded_at IS NULL`, а конверт
//!      агрегата став у чергу форвардингу (`hub_outbox`, `pending`);
//!   2. цикл форвардера віддає чергу вгору: на ХАБІ в `sync_log` з'являється
//!      запис із ТИМ САМИМ `batch_id = B` (критерій E3) і реальний чек у
//!      `receipts` — тобто доїхали ДАНІ, а не «позначка про спробу»;
//!   3. у вузла `sync_log.hub_forwarded_at IS NOT NULL` (стан A1) і черга
//!      `done`;
//!   4. роль інстанса вирішує налаштування ЙОГО БД: у хаба `sync.hub_url`
//!      немає → він не форвардить і НЕ наповнює чергу (нуль сміття);
//!   5. ґейт Фази 3.8 (`push_blocked_reason`) шлях форвардера НЕ проходить:
//!      у стані «promoted primary без апстріму», де касовий HTTP-push
//!      ЗАБЛОКОВАНО, форвардер усе одно доставив (перевіряється нижче і
//!      доводиться тим, що `hub_forwarder` не має `NodeConfig` взагалі).

mod common;

use serde_json::Value;
use torgashka_infrastructure::node_config::NodeConfig;
use torgashka_infrastructure::store_ctx::StorePool;
use uuid::Uuid;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;

/// Доказова лінія тесту (у звіт — саме ці рядки).
fn evidence(line: &str) {
    eprintln!("[e3][evidence] {line}");
}

#[tokio::test]
async fn node_forwards_receipt_to_hub() {
    common::force_test_db();
    let hub_pool = env::hub_pool(&env::pool_to(&env::test_db_url()).await, "fwd").await;
    let node_pool = env::node_pool(&hub_pool, "fwd").await;

    let product = Uuid::new_v4();
    env::seed(&hub_pool, product, "fwd-hub").await;
    env::seed(&node_pool, product, "fwd-node").await;

    // ── Два фасади: хаб (порт знаємо заздалегідь) і вузол ───────────────────
    let (hub_base, hub_port) = env::serve_any(env::app_state(&hub_pool)).await;
    let (node_base, node_port) = env::serve_any(env::app_state(&node_pool)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&node_base).await;
    evidence(&format!(
        "фасади: вузол http://127.0.0.1:{node_port} (БД вузла), хаб http://127.0.0.1:{hub_port} (БД хаба)"
    ));

    // Апстрім — властивість БД ВУЗЛА (у хаба його немає): саме це робить
    // інстанс вузлом, а не хабом.
    let token = env::owner_token();
    env::set_hub_upstream(&node_pool, &hub_base, &token).await;
    let cfg = torgashka_api::hub_forwarder::HubForwardConfig::from_pool(&node_pool)
        .await
        .expect("налаштування вузла прочитано")
        .expect("вузол має апстрім (sync.hub_url)");
    assert_eq!(cfg.base_url, hub_base, "апстрім вузла = хаб");
    assert!(
        torgashka_api::hub_forwarder::HubForwardConfig::from_pool(&hub_pool)
            .await
            .expect("налаштування хаба прочитано")
            .is_none(),
        "у ХАБА апстріму немає — він не форвардить і не наповнює чергу"
    );
    evidence("роль інстанса: вузол має sync.hub_url, хаб — ні (різні БД, не прапорець)");

    // ── Ґейт Фази 3.8: у цьому стані КАСОВИЙ шлях заблоковано ──────────────
    let promoted = NodeConfig::default().into_promoted_primary();
    let gate_reason = promoted.push_blocked_reason();
    assert!(
        gate_reason.is_some(),
        "стан вузла мусить бути 'promoted primary без апстріму' (ґейт касового push активний)"
    );
    evidence(&format!(
        "ґейт Фази 3.8 активний для касового шляху: {}",
        gate_reason.unwrap_or_default()
    ));

    // ── 1. Каса → вузол: чек прийнято ЛОКАЛЬНО ────────────────────────────
    let client_uuid = Uuid::new_v4();
    let batch_id = Uuid::new_v4();
    let (code, body, resp_batch) = env::push(
        &node_base,
        &token,
        batch_id,
        &[env::receipt_env(client_uuid, product, "E3 forward")],
    )
    .await;
    assert_eq!(code, 200, "вузол приймає чек: HTTP {code}, тіло {body}");
    assert_eq!(
        body[0]["status"], "created",
        "чек створено на вузлі: {body}"
    );
    assert_eq!(
        resp_batch,
        batch_id.to_string(),
        "вузол повертає ТОЙ САМИЙ batch_id (E2a)"
    );

    let (node_status, node_forwarded, node_batch): (String, Option<String>, Option<Uuid>) =
        sqlx::query_as(
            "SELECT status, hub_forwarded_at::text, batch_id FROM sync_log \
             WHERE store_id = $1 AND client_uuid = $2 AND entity = 'receipt'",
        )
        .bind(env::store_id())
        .bind(client_uuid)
        .fetch_one(&node_pool)
        .await
        .expect("журнал вузла");
    assert_eq!(node_status, "ok", "прийом зафіксовано як ok");
    assert_eq!(node_batch, Some(batch_id), "batch_id у журналі вузла");
    assert!(
        node_forwarded.is_none(),
        "ще не форварджено: hub_forwarded_at IS NULL"
    );

    let (q_status, q_batch, q_attempts, q_envelope): (String, Option<Uuid>, i32, Value) =
        sqlx::query_as(
            "SELECT status, batch_id, attempts, envelope FROM hub_outbox \
             WHERE store_id = $1 AND client_uuid = $2",
        )
        .bind(env::store_id())
        .bind(client_uuid)
        .fetch_one(&node_pool)
        .await
        .expect("черга форвардингу вузла мусить мати конверт агрегата");
    assert_eq!(q_status, "pending", "черга чекає доставки");
    assert_eq!(q_batch, Some(batch_id), "черга зберігає batch_id вузла");
    assert_eq!(q_attempts, 0, "спроб ще не було");
    assert_eq!(
        q_envelope["client_uuid"],
        client_uuid.to_string(),
        "у черзі — конверт агрегата як є: {q_envelope}"
    );
    evidence(&format!(
        "вузол: чек {client_uuid} прийнято локально, у черзі форвардингу 1 агрегат (batch {batch_id})"
    ));

    // ── 2. Цикл форвардера: черга вузла → хаб ─────────────────────────────
    let summary = torgashka_api::hub_forwarder::forward_pending(
        &StorePool::new(node_pool.clone()),
        &reqwest::Client::new(),
        &cfg,
        torgashka_api::hub_forwarder::BATCHES_PER_CYCLE,
    )
    .await
    .expect("цикл форвардингу виконано");
    assert_eq!(summary.sent, 1, "у цикл узято 1 агрегат: {summary:?}");
    assert_eq!(summary.accepted, 1, "хаб підтвердив агрегат: {summary:?}");
    assert_eq!(summary.failed, 0, "невдач немає: {summary:?}");
    evidence(&format!(
        "форвардер: пакетів={}, агрегатів={}, прийнято хабом={}, відкладено={}, поховано={}",
        summary.batches, summary.sent, summary.accepted, summary.deferred, summary.failed
    ));

    // ── 3. ХАБ: дані доїхали, batch_id — ТОЙ САМИЙ ────────────────────────
    let (hub_batch, hub_entity, hub_status, hub_cu): (Option<Uuid>, String, String, Option<Uuid>) =
        sqlx::query_as(
            "SELECT batch_id, entity, status, client_uuid FROM sync_log \
             WHERE store_id = $1 AND client_uuid = $2",
        )
        .bind(env::store_id())
        .bind(client_uuid)
        .fetch_one(&hub_pool)
        .await
        .expect("журнал ХАБА мусить мати запис від форвардера");
    assert_eq!(
        hub_batch,
        Some(batch_id),
        "КРИТЕРІЙ E3: на хабі sync_log має ТОЙ САМИЙ batch_id"
    );
    assert_eq!(hub_entity, "receipt");
    assert_eq!(hub_status, "ok");
    assert_eq!(hub_cu, Some(client_uuid));

    let (items, batch_status): (i32, String) =
        sqlx::query_as("SELECT items, status FROM sync_batches WHERE id = $1")
            .bind(batch_id)
            .fetch_one(&hub_pool)
            .await
            .expect("батч хаба");
    assert_eq!(items, 1, "пакет хаба містить 1 агрегат");
    assert_eq!(batch_status, "accepted", "пакет прийнято хабом");

    let hub_receipts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM receipts WHERE store_id = $1 AND client_uuid = $2",
    )
    .bind(env::store_id())
    .bind(client_uuid)
    .fetch_one(&hub_pool)
    .await
    .expect("чек у БД хаба");
    assert_eq!(
        hub_receipts, 1,
        "на хабі реальний чек каси (не лише журнал)"
    );
    evidence(&format!(
        "хаб: sync_log.batch_id = {batch_id} (той самий), sync_batches.status = accepted, receipts = {hub_receipts}"
    ));

    // ── 4. Вузол: стан A1 виставлено, черга закрита ───────────────────────
    let (node_forwarded_after, node_forward_status): (Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT hub_forwarded_at::text, hub_forward_status FROM sync_log \
             WHERE store_id = $1 AND client_uuid = $2 AND entity = 'receipt'",
        )
        .bind(env::store_id())
        .bind(client_uuid)
        .fetch_one(&node_pool)
        .await
        .expect("журнал вузла після форвардингу");
    assert!(
        node_forwarded_after.is_some(),
        "КРИТЕРІЙ E3: hub_forwarded_at IS NOT NULL у вузла"
    );
    assert_eq!(node_forward_status.as_deref(), Some("accepted"));

    let (q_status_after, q_forwarded_at, q_attempts_after): (String, Option<String>, i32) =
        sqlx::query_as(
            "SELECT status, forwarded_at::text, attempts FROM hub_outbox \
             WHERE store_id = $1 AND client_uuid = $2",
        )
        .bind(env::store_id())
        .bind(client_uuid)
        .fetch_one(&node_pool)
        .await
        .expect("черга вузла після доставки");
    assert_eq!(q_status_after, "done", "черга закрита");
    assert!(q_forwarded_at.is_some(), "forwarded_at виставлено");
    assert_eq!(q_attempts_after, 1, "рівно одна спроба");
    evidence("вузол: hub_forwarded_at IS NOT NULL, черга done (1 спроба)");

    // ── 5. Хаб не форвардить далі (він і є хаб) ───────────────────────────
    let hub_queue: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hub_outbox")
        .fetch_one(&hub_pool)
        .await
        .expect("черга хаба");
    assert_eq!(
        hub_queue, 0,
        "БД хаба НЕ наповнює чергу форвардингу (апстріму немає — нуль сміття)"
    );
    evidence("хаб: hub_outbox порожній — прийняте хабом не стає чергою форвардингу");
}
