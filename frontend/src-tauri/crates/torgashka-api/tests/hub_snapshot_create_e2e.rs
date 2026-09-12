//! Контракт R3 (ADR-0008 «Варіант B»): ХАБ САМ СТВОРЮЄ ЗНІМОК своєї активної
//! БД (`pg_dump -Fc` у каталог знімків) — і вузол забирає його ТИМ САМИМ
//! процесом, без перезапуску («створив → одразу видно»).
//!
//! Що доводить тест (усе на РЕАЛЬНОМУ кластері PostgreSQL 17 і реальному
//! `pg_dump`/`pg_restore` з каталогу бінарників):
//!   1. `POST /api/v1/admin/hub-snapshot` (роль admin) → 200 і файл
//!      `<database>_YYYYMMDD_HHMMSS.dump` у каталозі `TORGASHKA_HUB_SNAPSHOT_DIR`
//!      (він же каталог R1-ендпоінта), `bytes` = `metadata().len()`,
//!      `sha256` — перерахований із ЗАПИСАНОГО файла;
//!   2. `GET /api/v1/sync/snapshot` (роль device) у ТОМУ САМОМУ процесі віддає
//!      саме цей файл: `x-snapshot-filename` / `x-snapshot-sha256` /
//!      `Content-Length` збігаються з відповіддю POST — інваріант провіжну;
//!   3. знімок справді є дампом БД: `pg_restore -l` показує TOC (custom-формат),
//!      а `pg_restore` у чисту БД того ж кластера дає ТІ САМІ `count(*)`
//!      (users/products/receipts/stores) — обидві таблиці в лог;
//!   4. негативи: без токена → 401, роль `device` на POST → 403 (може забирати,
//!      не створювати), недосяжне активне джерело → 400 і каталог знімків
//!      НЕ змінився (жодного обрізаного/порожнього файла);
//!   5. два POST'и в межах ОДНІЄЇ секунди → два РІЗНІ файли (суфікс колізії
//!      `-01`), обидва валідні; наявний файл ніколи не перезаписується.
//!
//! # Кластер тесту
//! Власний кластер PG 17 (initdb + pg_ctl) у `/tmp`; порт вибирає СПІЛЬНИЙ
//! хелпер [`pg_cluster::resolve_port`] (див. `tests/common/pg_cluster.rs`):
//! явний `TORGASHKA_PG_TEST_PORT` → як є (зайнятий = аномалія вгору), інакше
//! порт продукту 5433, інакше автоматично вільний 5434..=5500. На цій машині
//! 5433 тримає системний кластер PG 17/main, тому тест піде на власний порт.
//!
//! # `pg_dump` береться з PATH (і це перевіряється фактом)
//! Хендлер кличе `db_sources::find_binary("pg_dump")` — пошук у PATH (як
//! `export-dump`). На цій машині `/usr/bin/pg_dump` — це pg_wrapper системного
//! кластера **16.15**, а сервер тесту — **17.6**; `pg_dump` 16 відмовляється
//! дампити сервер 17. Тому тест (як і оточення CI) ставить на початок PATH
//! каталог 17-х бінарників (той самий, що `TORGASHKA_PG_DIR`), і ДРУКУЄ обидві
//! версії: «до» і «після». Розбіжність версій — аномалія вгору (див. звіт), а
//! не тиха підміна: якщо після налаштування PATH `pg_dump` усе ще не 17.x —
//! тест падає з ANOMALY.
//!
//! # Роль `store_manager` на POST (факт, не припущення)
//! `actor_claims` = `auth_routes::require_admin`, який дозволяє
//! `owner|store_manager|admin` (1:1 з `export-dump`). Тому `store_manager`
//! отримує 200, а не 403 — це ФАКТ, який тест фіксує явним рядком
//! `[r3][ANOMALY]` (розходження з очікуванням контракту — вгору).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use uuid::Uuid;

use torgashka_api::auth::create_access_token;
use torgashka_infrastructure::db_sources::{self, DbSource, DbSourcesFile};
use torgashka_infrastructure::embedded_pg::EMBEDDED_PG_PORT;

#[path = "common/hub_env.rs"]
mod hub_env;
#[path = "common/pg_cluster.rs"]
mod pg_cluster;

/// Префікс доказових рядків цього тесту.
const TAG: &str = "[r3]";
/// Env із ЯВНИМ портом кластера тесту (переозначення — лише явне).
const PORT_ENV: &str = "TORGASHKA_PG_TEST_PORT";
/// Каталог кластера тесту (дані — у tmp).
const DATA_DIR_PREFIX: &str = "torgashka_hub_snap_pgdata";
/// Каталог знімків тесту (`TORGASHKA_HUB_SNAPSHOT_DIR`).
const SNAP_DIR_NAME: &str = "torgashka_hub_snap_snapshots";
/// Каталог конфіга джерел тесту (`TORGASHKA_DB_SOURCES`).
const SRC_DIR_NAME: &str = "torgashka_hub_snap_sources";
/// БД, у яку відновлюємо знімок для перевірки даних (c).
const RESTORED_DB: &str = "r3_restored";
/// Детерміновані дані джерела (числа зафіксовані в звіті й перевіряються фактом).
const USERS: i64 = 3;
const PRODUCTS: i64 = 7;
const RECEIPTS: i64 = 2;
const STORES: i64 = 1;
/// bcrypt('admin123') — спільний seed-пароль e2e torgashka-api (не для входу тут).
const PWD: &str = "$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e";

fn evidence(line: &str) {
    pg_cluster::evidence(TAG, line);
}

/// Детермінований UUID: `0000…-0000-00000000NNNN`.
fn det_uuid(n: u16) -> Uuid {
    Uuid::from_u128(n as u128)
}

// ─────────────────────────────────────────────────────────────────────────────
// Хелпери тесту
// ─────────────────────────────────────────────────────────────────────────────

/// Каталог знімків = те, що бачить і хендлер (`sync_snapshot::snapshot_dir`).
fn snapshots_dir() -> PathBuf {
    torgashka_api::sync_snapshot::snapshot_dir()
}

/// Вміст каталогу знімків: `(ім'я, байти)`, відсортовано за ім'ям.
fn dir_listing(dir: &Path) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().to_string(),
                        e.metadata().map(|m| m.len()).unwrap_or(0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// sha256 файла тим самим кодом, що в хендлері (не «з повітря»).
fn file_sha256(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("читання файла знімка");
    torgashka_api::sync_snapshot::sha256_hex(&bytes)
}

/// `POST /api/v1/admin/hub-snapshot` → (HTTP-код, тіло JSON).
async fn post_snapshot(
    client: &reqwest::Client,
    base: &str,
    role: &str,
    token: Option<&str>,
) -> (u16, Value) {
    let mut req = client
        .post(format!("{base}/api/v1/admin/hub-snapshot"))
        .json(&json!({}));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let started = Instant::now();
    let resp = req.send().await.expect("POST hub-snapshot");
    let code = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    evidence(&format!(
        "POST /api/v1/admin/hub-snapshot (роль {role}) → HTTP {code} за {:?}",
        started.elapsed()
    ));
    (code, body)
}

/// `GET /api/v1/sync/snapshot` → (код, заголовки, тіло).
async fn get_snapshot(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> (u16, reqwest::header::HeaderMap, Vec<u8>) {
    let resp = client
        .get(format!("{base}/api/v1/sync/snapshot"))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET sync/snapshot");
    let code = resp.status().as_u16();
    let headers = resp.headers().clone();
    let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
    (code, headers, body)
}

/// Чекає до `<межа секунди> + 80 мс`: два послідовні POST'и тоді напевно
/// порахують мітку часу в ОДНІЙ секунді (перевірка колізійного суфікса).
async fn wait_just_after_second_boundary() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ms = now.as_millis() % 1000;
    let wait_ms = if ms < 80 { 80 - ms } else { 1080 - ms };
    if wait_ms > 0 {
        tokio::time::sleep(Duration::from_millis(wait_ms as u64 + 20)).await;
    }
}

/// Чи це ім'я знімка замороженого формату: `<db>_YYYYMMDD_HHMMSS[-NN].dump`.
fn name_matches_format(name: &str, db: &str) -> bool {
    let Some(rest) = name.strip_prefix(&format!("{db}_")) else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(".dump") else {
        return false;
    };
    let (ts, suffix) = match rest.split_once('-') {
        Some((ts, sfx)) => (ts, Some(sfx)),
        None => (rest, None),
    };
    let digits = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_digit());
    let (date, time) = match ts.split_once('_') {
        Some((d, t)) => (d, t),
        None => return false,
    };
    if !digits(date, 8) || !digits(time, 6) {
        return false;
    }
    match suffix {
        None => true,
        Some(s) => digits(s, 2),
    }
}

/// Мітка часу з імені знімка (`YYYYMMDD_HHMMSS`, без суфікса).
fn ts_of(name: &str, db: &str) -> String {
    let rest = name
        .strip_prefix(&format!("{db}_"))
        .and_then(|r| r.strip_suffix(".dump"))
        .unwrap_or(name);
    rest.split_once('-')
        .map(|(ts, _)| ts)
        .unwrap_or(rest)
        .to_string()
}

/// Детерміновані рядки джерела (3 users / 7 products / 2 receipts / 1 store).
async fn seed_deterministic(pool: &sqlx::PgPool) -> Vec<Uuid> {
    let store = det_uuid(0x0a01);
    sqlx::query("INSERT INTO stores (id, name) VALUES ($1, 'R3 Тестова точка')")
        .bind(store)
        .execute(pool)
        .await
        .expect("seed store");

    let mut user_ids = Vec::new();
    for (i, role) in ["owner", "admin", "cashier"].iter().enumerate() {
        let id = det_uuid(0x0b00 + i as u16);
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
             VALUES ($1, $2, $3, $4, $5::public.user_role, true, now(), now(), true)",
        )
        .bind(id)
        .bind(format!("R3 Користувач {i}"))
        .bind(format!("r3_user_{i}"))
        .bind(PWD)
        .bind(role)
        .execute(pool)
        .await
        .expect("seed user");
        user_ids.push(id);
    }

    for i in 0..PRODUCTS {
        sqlx::query(
            "INSERT INTO products (id, barcode, title, price, tax_rate, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 20.00, now(), now())",
        )
        .bind(det_uuid(0x0c00 + i as u16))
        .bind(format!("48200000000{i:02}"))
        .bind(format!("R3 Товар {i}"))
        .bind(10.0 + i as f64)
        .execute(pool)
        .await
        .expect("seed product");
    }

    for i in 0..RECEIPTS {
        sqlx::query(
            "INSERT INTO receipts (id, receipt_number, cashier_id, total_amount, store_id, created_at)
             VALUES ($1, $2, $3, $4, $5, now())",
        )
        .bind(det_uuid(0x0d00 + i as u16))
        .bind(format!("R3-CHK-{i:03}"))
        .bind(user_ids[i as usize])
        .bind(50.0 + i as f64)
        .bind(store)
        .execute(pool)
        .await
        .expect("seed receipt");
    }
    user_ids
}

/// `count(*)` чотирьох таблиць — (users, products, receipts, stores).
async fn counts(pool: &sqlx::PgPool) -> (i64, i64, i64, i64) {
    let mut out = [0i64; 4];
    for (idx, table) in ["users", "products", "receipts", "stores"]
        .iter()
        .enumerate()
    {
        out[idx] = sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap_or_else(|e| panic!("count(*) {table}: {e}"));
    }
    (out[0], out[1], out[2], out[3])
}

/// Конфіг джерел: активне джерело = кластер тесту (trust-auth, без пароля).
fn write_sources_config(cfg_path: &Path, active: &str, host: &str, port: u16, database: &str) {
    let cfg = DbSourcesFile {
        active: Some(active.to_string()),
        sources: vec![(
            active.to_string(),
            DbSource {
                label: Some("R3 тестовий кластер".to_string()),
                host: host.to_string(),
                port,
                database: database.to_string(),
                user: "postgres".to_string(),
                password_encrypted: None,
                status: None,
            },
        )],
    };
    db_sources::save_to(cfg_path, &cfg).expect("запис db_sources.toml");
}

/// Make sure PATH містить `pg_dump` САМЕ тієї версії, що сервер: друкує
/// «до» і «після» і падає, якщо підмінити не вдалося (жодної тихої надії).
fn ensure_pg_dump_in_path(bin_dir: &Path, required_major: &str) {
    let before = db_sources::find_binary("pg_dump")
        .map(|p| format!("{} ({})", p.display(), pg_cluster::tool_version(&p)))
        .unwrap_or_else(|e| format!("НЕ ЗНАЙДЕНО: {e}"));
    evidence(&format!("pg_dump у PATH (до налаштування): {before}"));
    if !before.contains(required_major) {
        let old = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{old}", bin_dir.display()));
        let after = db_sources::find_binary("pg_dump")
            .map(|p| format!("{} ({})", p.display(), pg_cluster::tool_version(&p)))
            .unwrap_or_else(|e| format!("НЕ ЗНАЙДЕНО: {e}"));
        evidence(&format!(
            "pg_dump у PATH (після додавання {} — як у CI/оточенні з PG 17): {after}",
            bin_dir.display()
        ));
        assert!(
            after.contains(required_major),
            "[r3][ANOMALY] хендлер кличе find_binary(\"pg_dump\") (пошук у PATH), а в PATH немає \
             pg_dump {required_major}: {after}. Сервер тесту — {required_major}, старіший pg_dump \
             відмовляється його дампити."
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Тест
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hub_creates_custom_format_snapshot_and_node_takes_it_immediately() {
    let started = Instant::now();
    // ── 0. Бінарники PG 17 + власний кластер у /tmp ─────────────────────────
    let bin_dir = pg_cluster::locate_binaries(TAG, "17.");
    ensure_pg_dump_in_path(&bin_dir, "17.");
    let port = pg_cluster::resolve_port(TAG, PORT_ENV, EMBEDDED_PG_PORT);
    let cluster = pg_cluster::TestCluster::start(TAG, DATA_DIR_PREFIX, bin_dir.clone(), port);
    evidence(&format!(
        "тестовий кластер: {} (порт {}), БД джерела {}",
        cluster.data_dir.display(),
        cluster.port,
        cluster.url()
    ));

    // ── Каталоги тесту в tmp + env, які читає хендлер ───────────────────────
    let tmp = std::env::temp_dir().join(format!("{SRC_DIR_NAME}_{}", std::process::id()));
    let src_dir = tmp.clone();
    std::fs::create_dir_all(&src_dir).expect("каталог конфіга");
    let cfg_path = src_dir.join("db_sources.toml");
    let snap_dir = std::env::temp_dir().join(format!("{SNAP_DIR_NAME}_{}", std::process::id()));
    std::fs::create_dir_all(&snap_dir).expect("каталог знімків");
    std::env::set_var(db_sources::DB_SOURCES_ENV, &cfg_path);
    std::env::set_var(torgashka_api::sync_snapshot::SNAPSHOT_DIR_ENV, &snap_dir);
    write_sources_config(
        &cfg_path,
        "hub",
        "127.0.0.1",
        cluster.port,
        pg_cluster::MAIN_DB,
    );
    evidence(&format!(
        "конфіг джерел: {} (active=hub → 127.0.0.1:{}); каталог знімків: {}",
        cfg_path.display(),
        cluster.port,
        snapshot_dir_string()
    ));

    // ── 1. Схема (наявний механізм міграцій) + детерміновані дані ───────────
    let pool = hub_env::pool_to(&cluster.url()).await;
    hub_env::ensure_schema_on(&pool).await;
    evidence("схему застосовано: infrastructure::db::ensure_schema + sync_schema::apply");
    seed_deterministic(&pool).await;
    let src_counts = counts(&pool).await;
    evidence(&format!(
        "дані ДЖЕРЕЛА: users={} products={} receipts={} stores={}",
        src_counts.0, src_counts.1, src_counts.2, src_counts.3
    ));
    assert_eq!(
        src_counts,
        (USERS, PRODUCTS, RECEIPTS, STORES),
        "детерміновані дані джерела"
    );

    // ── 2. Фасад (той самий роутер, що в проді) на цій БД ───────────────────
    let state = hub_env::app_state(&pool);
    let (base, api_port) = hub_env::serve_any(state).await;
    hub_env::wait_ready(&base).await;
    evidence(&format!("фасад слухає {base} (порт {api_port})"));

    let client = reqwest::Client::new();
    let admin = create_access_token(&Uuid::new_v4().to_string(), "admin", &[], hub_env::SECRET)
        .expect("JWT admin");
    let device = create_access_token(&Uuid::new_v4().to_string(), "device", &[], hub_env::SECRET)
        .expect("JWT device");
    let store_manager = create_access_token(
        &Uuid::new_v4().to_string(),
        "store_manager",
        &[],
        hub_env::SECRET,
    )
    .expect("JWT store_manager");

    // ── (a) POST як admin → 200 + файл + sha256 із ЗАПИСАНОГО файла ─────────
    let before_a = dir_listing(&snapshots_dir());
    let (code, body) = post_snapshot(&client, &base, "admin", Some(&admin)).await;
    assert_eq!(code, 200, "POST hub-snapshot: {body}");
    assert_eq!(body["ok"], json!(true), "тіло: {body}");
    let file_a = body["file_name"].as_str().expect("file_name").to_string();
    let path_a = PathBuf::from(body["path"].as_str().expect("path"));
    let bytes_a = body["bytes"].as_u64().expect("bytes");
    let sha_a = body["sha256"].as_str().expect("sha256").to_string();
    evidence(&format!(
        "(a) admin POST → ok=true, file_name={file_a}, bytes={bytes_a}, sha256={sha_a}\n    path={}",
        path_a.display()
    ));
    assert!(
        path_a.is_file(),
        "файл мусить існувати: {}",
        path_a.display()
    );
    assert_eq!(
        path_a.parent(),
        Some(snapshots_dir().as_path()),
        "каталог = snapshot_dir()"
    );
    assert!(bytes_a > 0, "bytes > 0");
    assert_eq!(bytes_a, std::fs::metadata(&path_a).expect("metadata").len());
    assert_eq!(
        sha_a,
        file_sha256(&path_a),
        "sha256 мусить бути від ЗАПИСАНОГО файла"
    );
    assert!(
        name_matches_format(&file_a, pg_cluster::MAIN_DB),
        "ім'я мусить бути <database>_YYYYMMDD_HHMMSS.dump: {file_a}"
    );
    assert_eq!(
        dir_listing(&snapshots_dir()).len(),
        before_a.len() + 1,
        "у каталозі рівно +1 файл"
    );

    // ── (b) ІНВАРІАНТ: той самий процес, без перезапуску → device забирає ───
    let (code, headers, body_bytes) = get_snapshot(&client, &base, &device).await;
    let h_name = headers
        .get("x-snapshot-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let h_sha = headers
        .get("x-snapshot-sha256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    evidence(&format!(
        "(b) device GET /api/v1/sync/snapshot → HTTP {code}, x-snapshot-filename={h_name}, \
         x-snapshot-sha256={h_sha}, content-length={}",
        headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("—")
    ));
    assert_eq!(code, 200, "GET знімка роллю device");
    assert_eq!(h_name, file_a, "вузол мусить бачити щойно створений знімок");
    assert_eq!(h_sha, sha_a, "sha256 у заголовку = sha256 створеного файла");
    assert_eq!(body_bytes.len() as u64, bytes_a, "тіло = bytes");
    assert_eq!(
        torgashka_api::sync_snapshot::sha256_hex(&body_bytes),
        sha_a,
        "тіло, яке забрав вузол, збігається зі знімком"
    );

    // ── (e) Два POST'и в межах ОДНІЄЇ секунди → колізійний суфікс ───────────
    let mut pair: Option<(String, String)> = None;
    for attempt in 1..=3 {
        wait_just_after_second_boundary().await;
        let (c1, b1) = post_snapshot(&client, &base, "admin", Some(&admin)).await;
        let (c2, b2) = post_snapshot(&client, &base, "admin", Some(&admin)).await;
        assert_eq!(
            (c1, c2),
            (200, 200),
            "обидва POST'и мусять бути 200: {b1} / {b2}"
        );
        let n1 = b1["file_name"].as_str().unwrap_or_default().to_string();
        let n2 = b2["file_name"].as_str().unwrap_or_default().to_string();
        assert_ne!(n1, n2, "два POST'и не можуть дати один файл");
        if ts_of(&n1, pg_cluster::MAIN_DB) == ts_of(&n2, pg_cluster::MAIN_DB) {
            evidence(&format!(
                "(e) спроба {attempt}: обидва POST'и в одну секунду → {n1} і {n2}"
            ));
            pair = Some((n1, n2));
            break;
        }
        evidence(&format!(
            "(e) спроба {attempt}: POST'и роз'їхались по секундах ({n1} / {n2}) — повторюю \
             вирівнювання до межі секунди"
        ));
    }
    let (n1, n2) = pair.expect(
        "[r3][ANOMALY] не вдалося отримати два знімки в межах однієї секунди за 3 спроби — \
         перевірка колізійного суфікса не відбулася (не пропускаю мовчки)",
    );
    let ts = ts_of(&n1, pg_cluster::MAIN_DB);
    assert_eq!(
        n1,
        format!("{}_{ts}.dump", pg_cluster::MAIN_DB),
        "перший — без суфікса"
    );
    assert_eq!(
        n2,
        format!("{}_{ts}-01.dump", pg_cluster::MAIN_DB),
        "другий — із суфіксом колізії (наявний файл не перезаписано)"
    );
    for name in [&n1, &n2] {
        let p = snapshots_dir().join(name);
        assert!(p.is_file(), "файл у каталозі: {}", p.display());
        assert!(
            name_matches_format(name, pg_cluster::MAIN_DB),
            "формат імені: {name}"
        );
    }
    // Найновіший = другий файл пари → вузол його ж і забирає.
    let (code, headers, body_bytes) = get_snapshot(&client, &base, &device).await;
    let h_name = headers
        .get("x-snapshot-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    evidence(&format!(
        "(e) після колізійної пари device GET → HTTP {code}, віддано {h_name} \
         ({} байт)",
        body_bytes.len()
    ));
    assert_eq!(code, 200);
    assert_eq!(h_name, n2, "хаб віддає НАЙНОВІШИЙ знімок");

    // ── (c) pg_restore цього знімка в чисту БД → ті самі count(*) ────────────
    let pg_restore = pg_cluster::tool(&bin_dir, "pg_restore");
    let toc = std::process::Command::new(&pg_restore)
        .arg("-l")
        .arg(&path_a)
        .output()
        .expect("pg_restore -l");
    let toc_text = String::from_utf8_lossy(&toc.stdout).to_string();
    let toc_entries = toc_text.lines().filter(|l| l.contains(";")).count();
    evidence(&format!(
        "(c) pg_restore -l {} → код {:?}, записів TOC: {toc_entries}\n    {}",
        file_a,
        toc.status.code(),
        toc_text
            .lines()
            .filter(|l| l.contains("TABLE DATA") || l.contains("TYPE") || l.contains("SCHEMA"))
            .take(6)
            .collect::<Vec<_>>()
            .join("\n    ")
    ));
    assert!(
        toc.status.success(),
        "pg_restore -l мусить читати custom-дамп: {}",
        String::from_utf8_lossy(&toc.stderr)
    );
    assert!(
        toc_entries > 50,
        "TOC мусить бути змістовним (custom-формат): {toc_entries}"
    );
    assert!(
        toc_text.contains("TABLE DATA"),
        "TOC мусить містити дані таблиць (це дамп БД, а не порожній файл)"
    );

    cluster.create_db(RESTORED_DB);
    let restore = std::process::Command::new(&pg_restore)
        .arg("--no-owner")
        .arg("--no-privileges")
        .arg("-d")
        .arg(cluster.url_for(RESTORED_DB))
        .arg(&path_a)
        .output()
        .expect("pg_restore у чисту БД");
    evidence(&format!(
        "(c) pg_restore -d {} {} → код {:?}",
        RESTORED_DB,
        file_a,
        restore.status.code()
    ));
    assert!(
        restore.status.success(),
        "pg_restore мусить відновити знімок: {}",
        String::from_utf8_lossy(&restore.stderr)
    );
    let restored_pool = hub_env::pool_to(&cluster.url_for(RESTORED_DB)).await;
    let rest_counts = counts(&restored_pool).await;
    evidence(&format!(
        "(c) count(*) — ДЖЕРЕЛО (users={} products={} receipts={} stores={}) | \
         ВІДНОВЛЕНО (users={} products={} receipts={} stores={})",
        src_counts.0,
        src_counts.1,
        src_counts.2,
        src_counts.3,
        rest_counts.0,
        rest_counts.1,
        rest_counts.2,
        rest_counts.3
    ));
    assert_eq!(
        rest_counts, src_counts,
        "count(*) відновленої БД мусять дорівнювати джерелу"
    );

    // ── (d) Негативи ────────────────────────────────────────────────────────
    let (code, _) = post_snapshot(&client, &base, "—", None).await;
    evidence(&format!(
        "(d) POST без токена → HTTP {code} (очікується 401)"
    ));
    assert_eq!(code, 401, "без токена — 401");

    let (code, _) = post_snapshot(&client, &base, "device", Some(&device)).await;
    evidence(&format!(
        "(d) POST роллю device → HTTP {code} (очікується 403)"
    ));
    assert_eq!(code, 403, "device може забирати знімок, але не створювати");

    let (code, body_sm) =
        post_snapshot(&client, &base, "store_manager", Some(&store_manager)).await;
    evidence(&format!(
        "(d) POST роллю store_manager → HTTP {code} (контракт очікував 403)\n    \
         [r3][ANOMALY] actor_claims = auth_routes::require_admin, який дозволяє \
         owner|store_manager|admin (1:1 з export-dump) → 200. Розходження з \
         очікуванням контракту — вгору, не приховую."
    ));
    assert_eq!(
        code, 200,
        "ФАКТ: store_manager проходить require_admin (як в export-dump); отримано {code}, тіло {body_sm}"
    );

    // Недосяжне активне джерело → 400 і каталог НЕ змінився.
    let dead_port = hub_env::free_port().await;
    write_sources_config(
        &cfg_path,
        "dead",
        "127.0.0.1",
        dead_port,
        pg_cluster::MAIN_DB,
    );
    evidence(&format!(
        "(d) активне джерело перемкнуто на недосяжне 127.0.0.1:{dead_port} — POST мусить дати 400"
    ));
    let before_d = dir_listing(&snapshots_dir());
    let (code, body_dead) = post_snapshot(&client, &base, "admin", Some(&admin)).await;
    evidence(&format!(
        "(d) POST із недосяжним активним джерелом → HTTP {code}: {body_dead}"
    ));
    assert_eq!(
        code, 400,
        "недосяжне джерело — 400 (зрозуміла помилка), тіло: {body_dead}"
    );
    let after_d = dir_listing(&snapshots_dir());
    assert_eq!(
        after_d, before_d,
        "каталог знімків мусить лишитись ІДЕНТИЧНИМ (жодного обрізаного файла)"
    );
    write_sources_config(
        &cfg_path,
        "hub",
        "127.0.0.1",
        cluster.port,
        pg_cluster::MAIN_DB,
    );

    // ── Підсумок ────────────────────────────────────────────────────────────
    evidence(&format!(
        "каталог знімків після всіх перевірок ({} файлів):",
        dir_listing(&snapshots_dir()).len()
    ));
    for (name, bytes) in dir_listing(&snapshots_dir()) {
        evidence(&format!("    {name}  {bytes} Б"));
    }
    evidence(&format!(
        "УСЕ ПРОЙДЕНО за {:?} (файлів: {}, джерело: users={} products={} receipts={} stores={})",
        started.elapsed(),
        dir_listing(&snapshots_dir()).len(),
        src_counts.0,
        src_counts.1,
        src_counts.2,
        src_counts.3
    ));

    // Прибирання: дані тесту — у tmp (кластер знімає власний Drop; каталоги
    // конфіга й знімків — тут). Політика ретеншну знімків НЕ вигадується:
    // це прибирання ТЕСТОВОГО tmp, а не живої інсталяції.
    pool.close().await;
    restored_pool.close().await;
    let _ = std::fs::remove_dir_all(&snap_dir);
    let _ = std::fs::remove_dir_all(&src_dir);
    evidence("тестові каталоги в /tmp прибрано (кластер, конфіг джерел, знімки)");
}

/// Каталог знімків рядком (для доказового рядка, без зайвих рухів).
fn snapshot_dir_string() -> String {
    snapshots_dir().display().to_string()
}
