//! Контракт R2 (ADR-0008 «Варіант B») — наскрізний тест: ПЕРШИЙ ЗАПУСК ВУЗЛА
//! відновлює БД зі знімка хаба.
//!
//! Що доводить тест (усе — на РЕАЛЬНОМУ кластері PostgreSQL 17 і реальному
//! артефакті хаба, без жодної «тимчасової» процедури):
//!   1. офлайн-шлях провіжну (`local_dump_path`) відновлює
//!      `artifacts/hub_snapshot/pos_system_fresh_20260912.dump` у БД вузла
//!      через реальний `pg_restore` з прапорцями контракту;
//!   2. у ЦІЙ Ж БД з'являються `sync.hub_url` (нормалізований) і
//!      `sync.hub_token`, причому `system_settings.store_id IS NULL` — запис
//!      інстансний, не скоуп точки;
//!   3. дані справді відновлені: `users=4`, `products=4409`, `receipts≥264`,
//!      `stores=35` (фактичні `count(*)`, виведені в лог);
//!   4. HTTP-фасад на цій БД віддає `GET /api/v1/sync/status` з
//!      `"role":"node"` — тобто БД справді стала ВУЗЛОМ мережі (роль вирішує
//!      налаштування ЇЇ власної БД, а не прапорець запуску).
//!
//! # Кластер тесту
//! Тест піднімає ВЛАСНИЙ кластер PG 17 (`initdb` + `pg_ctl`) у тимчасовому
//! каталозі `/tmp`, щоб не торкатись робочих БД машини. Дані — у tmp, порт
//! вибирає [`resolve_port`]: (1) явний `TORGASHKA_PG_TEST_PORT` — беремо як є,
//! зайнятий = АНОМАЛІЯ вгору (паніка з доказовою вибіркою власника порту:
//! `pg_lsclusters` + `/proc/net/tcp`); (2) інакше — порт продукту
//! [`embedded_pg::EMBEDDED_PG_PORT`] (5433), якщо він вільний (реалізм: той
//! самий порт, що в проді); (3) інакше 5433 зайнятий (на цій машині його тримає
//! системний кластер PG 17/main) — АВТОМАТИЧНО беремо вільний порт
//! (5434..=5500) і друкуємо факт вибору рядком `[r2][info] …`.
//! Паніки в гілці 3 немає: зайнятий 5433 — не привід червонити тест,
//! а привід підняти кластер поруч.
//! Паралельні тести цього файла завжди отримують РІЗНІ порти (заявка на порт
//! у межах процесу), тому `cargo test --workspace --no-fail-fast` зелений і без
//! ручного переозначення порту.
//!
//! # Чому після відновлення застосовується sync-шар
//! Знімок хаба зафіксовано на `alembic_version = 0014` (див.
//! `artifacts/hub_snapshot/MANIFEST.md`), тому в ньому НЕМАЄ таблиць sync-шару
//! пізніших міграцій (`hub_outbox` — 0021, `catalog_change_requests` — 0022).
//! У проді цей шар додає міграція, Rust-`ensure_schema` створює лише базову
//! схему; тест застосовує його ТИМ САМИМ хелпером, що решта sync-e2e
//! (`common/sync_schema.rs`), і друкує факт відсутності таблиць у лог — без
//! цього `/api/v1/sync/status` не має на чому рахувати чергу форвардингу.

use std::path::{Path, PathBuf};
use std::time::Instant;

use uuid::Uuid;

use torgashka_api::auth::create_access_token;
use torgashka_infrastructure::embedded_pg::EMBEDDED_PG_PORT;

#[path = "common/hub_env.rs"]
mod hub_env;
#[path = "common/pg_cluster.rs"]
mod pg_cluster;

/// Порт власного кластера тесту можна переозначити ЛИШЕ явно (див. шапку).
const PORT_ENV: &str = "TORGASHKA_PG_TEST_PORT";
/// Каталог тестового кластера (дані — у tmp, не в робочих шляхах застосунку).
const DATA_DIR_NAME: &str = "torgashka_contract2_pgdata";
/// Токен вузла, який мусить доїхати до БД вузла як `sync.hub_token`.
const NODE_TOKEN: &str = "contract2-node-token-0123456789";
/// Очікувані характеристики артефакта (MANIFEST.md хаба).
const DUMP_FILENAME: &str = "pos_system_fresh_20260912.dump";
const DUMP_BYTES: u64 = 716_167;
const DUMP_SHA256: &str = "3d2d17c2f1592dccd39d824da047cbf5c07f9e4ac50a19a7fade75616fabb25a";
/// Очікувані `count(*)` у відновленій БД (з MANIFEST.md, перевіряються фактом).
const EXPECTED_USERS: i64 = 4;
const EXPECTED_PRODUCTS: i64 = 4_409;
const EXPECTED_RECEIPTS_MIN: i64 = 264;
const EXPECTED_STORES: i64 = 35;

fn evidence(line: &str) {
    eprintln!("[r2][evidence] {line}");
}

fn artifact_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../artifacts/hub_snapshot")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_start_restores_db_from_hub_snapshot_and_becomes_node() {
    let started = Instant::now();
    let artifact = artifact_dir().join(DUMP_FILENAME);
    assert!(
        artifact.is_file(),
        "немає артефакта знімка хаба: {}",
        artifact.display()
    );

    // ── 0. Бінарники PG 17 і власний кластер у /tmp ─────────────────────────
    let bin_dir = pg_cluster::locate_binaries("[r2]", "17.");
    let port = pg_cluster::resolve_port("[r2]", PORT_ENV, EMBEDDED_PG_PORT);
    let cluster = pg_cluster::TestCluster::start("[r2]", DATA_DIR_NAME, bin_dir, port);
    evidence(&format!(
        "тестовий кластер: {} (порт {}), БД вузла {}",
        cluster.data_dir.display(),
        cluster.port,
        cluster.url()
    ));

    // ── 1. Провіжн: офлайн-шлях (дамп із файлу — «перший запуск із USB») ────
    // hub_url навмисно БЕЗ схеми й з хвостовим слешем: перевіряємо нормалізацію.
    let hub_url_input = format!("127.0.0.1:{}/", cluster.port);
    let expected_hub_url = format!("http://127.0.0.1:{}", cluster.port);
    let dump_dir = std::env::temp_dir().join(format!("{DATA_DIR_NAME}_dumps"));
    let mut cfg = torgashka_infrastructure::provision_from_hub::HubProvisionConfig::new(
        hub_url_input.clone(),
        NODE_TOKEN,
        Some(artifact.clone()),
    );
    cfg.target_db_url = cluster.url();
    cfg.ensure_local_db = false; // кластер тесту вже піднятий — ін'єкція цілі
    cfg.dump_dir = Some(dump_dir.clone());

    let outcome =
        torgashka_infrastructure::provision_from_hub::run(cfg, |step, status, message| {
            eprintln!("[r2][progress] {step} {} — {message}", status.as_str());
        })
        .await;

    evidence(&format!(
        "провіжн: ok={} class={:?} source={:?} dump_bytes={:?} dump_sha256={:?}",
        outcome.ok, outcome.class, outcome.source, outcome.dump_bytes, outcome.dump_sha256
    ));
    evidence(&format!("провіжн message: {}", outcome.message));
    if let Some(tail) = &outcome.stderr_tail {
        evidence(&format!(
            "pg_restore (хвіст виводу):\n    {}",
            tail.replace('\n', "\n    ")
        ));
    }
    assert!(
        outcome.ok,
        "провіжн мусить пройти: class={:?} message={} steps={:#?}",
        outcome.class, outcome.message, outcome.steps
    );
    assert_eq!(outcome.class, None);
    assert_eq!(outcome.source.as_deref(), Some("file"));
    assert_eq!(outcome.hub_url.as_deref(), Some(expected_hub_url.as_str()));
    assert_eq!(outcome.dump_bytes, Some(DUMP_BYTES));
    assert_eq!(outcome.dump_sha256.as_deref(), Some(DUMP_SHA256));
    let step_names: Vec<&str> = outcome.steps.iter().map(|s| s.step.as_str()).collect();
    assert_eq!(
        step_names,
        vec![
            "validate_url",
            "hub_reachable",
            "download",
            "restore",
            "configure"
        ]
    );
    let restore_step = outcome
        .steps
        .iter()
        .find(|s| s.step == "restore")
        .expect("крок restore");
    assert!(restore_step.ok, "restore: {}", restore_step.detail);

    // ── 2. Факти БД вузла: налаштування + дані (psql, доказово у лог) ───────
    cluster.psql_evidence(
        "schema: alembic_version і наявність sync-шару у відновленій БД",
        "SELECT (SELECT version_num FROM alembic_version) AS alembic, \
         to_regclass('public.hub_outbox') IS NOT NULL AS has_hub_outbox, \
         (SELECT count(*) FROM information_schema.tables WHERE table_schema='public') AS public_tables;",
    );
    cluster.psql_evidence(
        "налаштування вузла у ВЛАСНІЙ БД вузла",
        "SELECT key, value, (store_id IS NULL) AS store_is_null FROM system_settings \
         WHERE key IN ('sync.hub_url','sync.hub_token') ORDER BY key;",
    );
    cluster.psql_evidence(
        "count(*) відновлених даних",
        "SELECT (SELECT count(*) FROM users) AS users, (SELECT count(*) FROM products) AS products, \
         (SELECT count(*) FROM receipts) AS receipts, \
         (SELECT count(*) FROM stores) AS stores, (SELECT count(*) FROM user_stores) AS user_stores;",
    );

    let pool = hub_env::pool_to(&cluster.url()).await;
    let configured: Option<(String, Option<String>, Option<bool>)> = sqlx::query_as(
        "SELECT value, (SELECT value FROM system_settings WHERE key = 'sync.hub_token' LIMIT 1), \
                (store_id IS NULL) FROM system_settings WHERE key = 'sync.hub_url' LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("читання sync.hub_url");
    let (hub_url_written, token_written, store_is_null) =
        configured.expect("sync.hub_url мусить бути записаний");
    assert_eq!(
        hub_url_written, expected_hub_url,
        "записано НОРМАЛІЗОВАНИЙ URL (вхід був {hub_url_input})"
    );
    assert_eq!(
        token_written.as_deref(),
        Some(NODE_TOKEN),
        "sync.hub_token мусить доїхати до БД вузла"
    );
    assert_eq!(
        store_is_null,
        Some(true),
        "запис мусить бути інстансним (store_id IS NULL), а не в скоупі точки"
    );

    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM products), \
                (SELECT count(*) FROM receipts), (SELECT count(*) FROM stores)",
    )
    .fetch_one(&pool)
    .await
    .expect("count(*) після відновлення");
    assert_eq!(counts.0, EXPECTED_USERS, "users");
    assert_eq!(counts.1, EXPECTED_PRODUCTS, "products");
    assert!(
        counts.2 >= EXPECTED_RECEIPTS_MIN,
        "receipts: {} < {EXPECTED_RECEIPTS_MIN}",
        counts.2
    );
    assert_eq!(counts.3, EXPECTED_STORES, "stores");

    // ── 3. Фасад на цій БД: роль інстанса = node ────────────────────────────
    // sync-шар: у проді його додає міграція (тут — хелпер решти sync-e2e), бо
    // знімок хаба зафіксовано на alembic 0014 і таблиць 0021+ у ньому немає.
    hub_env::ensure_schema_on(&pool).await;
    evidence("sync-шар застосовано (ensure_schema + sync_schema::apply) — як у решті sync-e2e");

    let pair: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT us.user_id, us.store_id FROM user_stores us \
         JOIN stores s ON s.id = us.store_id ORDER BY us.created_at, us.user_id LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("користувач із доступом до точки з дампа");
    let (user_id, store_id) = pair.expect("у знімку хаба є user_stores (39 рядків)");
    evidence(&format!(
        "точка з дампа для перевірки: store_id={store_id}, користувач={user_id}"
    ));

    let (base, api_port) = hub_env::serve_any(hub_env::app_state(&pool)).await;
    hub_env::wait_ready(&base).await;
    let token = create_access_token(&user_id.to_string(), "owner", &[], hub_env::SECRET)
        .expect("JWT для запиту статусу");

    let response = reqwest::Client::new()
        .get(format!("{base}/api/v1/sync/status"))
        .bearer_auth(&token)
        .header("x-store-id", store_id.to_string())
        .send()
        .await
        .expect("GET /api/v1/sync/status");
    let status_code = response.status().as_u16();
    let body: serde_json::Value = response.json().await.expect("JSON статусу");
    evidence(&format!(
        "GET /api/v1/sync/status → HTTP {status_code} (фасад на 127.0.0.1:{api_port}): {body}"
    ));
    assert_eq!(status_code, 200, "статус вузла: {body}");
    assert_eq!(
        body["role"], "node",
        "роль інстанса мусить бути node (є sync.hub_url у ВЛАСНІЙ БД): {body}"
    );

    evidence(&format!(
        "ЗАГАЛЬНО: провіжн+перевірки завершено за {:?}",
        started.elapsed()
    ));
}

/// Мережевий шлях провіжну: вузол тягне знімок із ХАБА (реальний HTTP-фасад з
/// ендпоінтом R1 `GET /api/v1/sync/snapshot`), звіряє `X-Snapshot-Sha256`,
/// зберігає знімок у тимчасовий файл і лише потім відновлює БД.
///
/// Тут же — негативна гілка (канал аномалій контракту): невалідний токен
/// мусить дати `DownloadFailed` з явним «хаб відкинув токен», і жоден
/// наступний крок (restore/configure) виконаний бути не може.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hub_path_downloads_snapshot_with_sha_check_and_rejects_bad_token() {
    // Той самий файл, який «хаб» віддаватиме ендпоінтом R1 (той самий каталог).
    let artifact = artifact_dir().join(DUMP_FILENAME);
    assert!(
        artifact.is_file(),
        "немає артефакта знімка хаба: {}",
        artifact.display()
    );
    let bin_dir = pg_cluster::locate_binaries("[r2]", "17.");
    // Кожен тест файла заявляє СВІЙ вільний порт (див. `resolve_port`): тести
    // можуть іти паралельно, спільного номера між ними немає.
    let port = pg_cluster::resolve_port("[r2]", PORT_ENV, EMBEDDED_PG_PORT);
    assert!(
        !pg_cluster::port_open(port),
        "[r2][ANOMALY] порт {port} зайнятий — тесту потрібен вільний порт"
    );
    let cluster = pg_cluster::TestCluster::start("[r2]", DATA_DIR_NAME, bin_dir, port);
    evidence(&format!(
        "другий тест: власний кластер {} (порт {}), БД вузла {}",
        cluster.data_dir.display(),
        cluster.port,
        cluster.url()
    ));
    // БД вузла (ціль відновлення) лишається незайманою до самого провіжну.
    let pool = hub_env::pool_to(&cluster.url()).await;

    // ── «Хаб»: справжній фасад зі справжнім ендпоінтом R1 на артефакті репо ──
    // Фасад потребує РОБОЧОЇ БД (readiness = /setup/status), тому йому даємо
    // ОКРЕМУ БД цього ж кластера — не БД вузла: інакше перевірка готовності
    // фасаду торкалася б тієї самої БД, яку от-от має відновити pg_restore.
    cluster.create_db("hub_control");
    let hub_pool = hub_env::pool_to(&cluster.url_for("hub_control")).await;
    hub_env::ensure_schema_on(&hub_pool).await;
    // Назва env — КОНСТАНТА модуля R1 (жодного літерала-дубля в тесті).
    std::env::set_var(
        torgashka_api::sync_snapshot::SNAPSHOT_DIR_ENV,
        artifact_dir().to_string_lossy().to_string(),
    );
    let (hub_base, hub_port) = hub_env::serve_any(hub_env::app_state(&hub_pool)).await;
    hub_env::wait_ready(&hub_base).await;
    evidence(&format!(
        "хаб (фасад із R1-ендпоінтом знімка) слухає 127.0.0.1:{hub_port}; каталог знімків {}",
        artifact_dir().display()
    ));

    // ── Негативна гілка: токен невалідний → 401 → DownloadFailed ────────────
    let mut bad = torgashka_infrastructure::provision_from_hub::HubProvisionConfig::new(
        format!("127.0.0.1:{hub_port}"),
        "не-токен-вузла",
        None,
    );
    bad.target_db_url = cluster.url();
    bad.ensure_local_db = false;
    let failed = torgashka_infrastructure::provision_from_hub::run(bad, |_, _, _| {}).await;
    evidence(&format!(
        "негативна гілка (сміттєвий токен): ok={} class={:?} message={}",
        failed.ok, failed.class, failed.message
    ));
    assert!(!failed.ok, "невалідний токен не може давати успіх");
    assert_eq!(failed.class.as_deref(), Some("DownloadFailed"));
    assert!(
        failed.message.contains("401") || failed.message.contains("403"),
        "у повідомленні мусить бути фактичний статус хаба: {}",
        failed.message
    );
    let failed_steps: Vec<(String, bool)> = failed
        .steps
        .iter()
        .map(|s| (s.step.clone(), s.ok))
        .collect();
    assert_eq!(
        failed_steps,
        vec![
            ("validate_url".to_string(), true),
            ("hub_reachable".to_string(), true),
            ("download".to_string(), false),
        ],
        "після невдачі завантаження жоден наступний крок не виконується"
    );

    // ── Позитивна гілка: реальний JWT (ендпоінт R1 приймає device/admin/owner) ─
    let node_token =
        create_access_token(&Uuid::new_v4().to_string(), "owner", &[], hub_env::SECRET)
            .expect("JWT хаба");
    let dump_dir = std::env::temp_dir().join(format!("{DATA_DIR_NAME}_hub_dumps"));
    let mut cfg = torgashka_infrastructure::provision_from_hub::HubProvisionConfig::new(
        format!("127.0.0.1:{hub_port}"),
        node_token,
        None,
    );
    cfg.target_db_url = cluster.url();
    cfg.ensure_local_db = false;
    cfg.dump_dir = Some(dump_dir.clone());

    let outcome =
        torgashka_infrastructure::provision_from_hub::run(cfg, |step, status, message| {
            eprintln!("[r2][progress] {step} {} — {message}", status.as_str());
        })
        .await;
    evidence(&format!(
        "мережевий провіжн: ok={} source={:?} dump_bytes={:?} dump_sha256={:?}",
        outcome.ok, outcome.source, outcome.dump_bytes, outcome.dump_sha256
    ));
    assert!(
        outcome.ok,
        "провіжн із хаба мусить пройти: class={:?} message={} steps={:#?}",
        outcome.class, outcome.message, outcome.steps
    );
    assert_eq!(outcome.source.as_deref(), Some("hub"));
    assert_eq!(outcome.dump_bytes, Some(DUMP_BYTES));
    assert_eq!(outcome.dump_sha256.as_deref(), Some(DUMP_SHA256));
    let saved = dump_dir.join(DUMP_FILENAME);
    assert!(
        saved.is_file(),
        "знімок мусить лежати у тимчасовому файлі {} (ім'я з X-Snapshot-Filename)",
        saved.display()
    );
    evidence(&format!(
        "знімок із хаба збережено: {} ({} Б)",
        saved.display(),
        std::fs::metadata(&saved).map(|m| m.len()).unwrap_or(0)
    ));

    // Дані у відновленій з мережі БД + налаштування вузла.
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM products),                 (SELECT count(*) FROM receipts)",
    )
    .fetch_one(&pool)
    .await
    .expect("count(*) після мережевого відновлення");
    assert_eq!(counts.0, EXPECTED_USERS);
    assert_eq!(counts.1, EXPECTED_PRODUCTS);
    assert!(counts.2 >= EXPECTED_RECEIPTS_MIN);
    let hub_setting: (String, Option<bool>) = sqlx::query_as(
        "SELECT value, (store_id IS NULL) FROM system_settings WHERE key = 'sync.hub_url' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("sync.hub_url після мережевого провіжну");
    assert_eq!(hub_setting.0, format!("http://127.0.0.1:{hub_port}"));
    assert_eq!(
        hub_setting.1,
        Some(true),
        "запис інстансний (store_id IS NULL)"
    );
    evidence(&format!(
        "після мережевого провіжну: users={} products={} receipts={} sync.hub_url={} (store_id IS NULL: {:?})",
        counts.0, counts.1, counts.2, hub_setting.0, hub_setting.1
    ));
}
