//! Тести перехоплювача «запис у read-only репліку» (`readonly_guard`).
//!
//! Два рівні доказу:
//!   1. ЮНІТ (без PG): розпізнавання за SQLSTATE, нормалізація, метрики;
//!   2. РЕАЛЬНИЙ PostgreSQL БЕЗ фізичної репліки: сесія з
//!      `-c default_transaction_read_only=on` дає справжній SQLSTATE 25006 на
//!      `CREATE TABLE` — саме той сигнал, який ловить фунел `StorePool` на
//!      standby-вузлі. Це доводить, що сигнал надійний і не залежить від
//!      наявності другого вузла.

use std::borrow::Cow;
use std::error::Error as StdError;

use sqlx::error::{DatabaseError, ErrorKind};
use sqlx::postgres::PgPoolOptions;
use sqlx::{Error, PgPool};
use torgashka_infrastructure::readonly_guard::{
    db_error_class, fingerprint, hits, is_read_only_replica, normalize, normalize_with_sql,
    sqlstate_of, ReadOnlyReplicaError, MARKER, SQLSTATE_READ_ONLY,
};

/// Фейкова помилка БД: перевіряємо розпізнавання КЛАСУ за кодом (без PG).
#[derive(Debug)]
struct FakeDb {
    /// `None` — фейкова помилка БД БЕЗ SQLSTATE (вигаданий клас без коду).
    code: Option<&'static str>,
    message: &'static str,
}

impl std::fmt::Display for FakeDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "db error {}: {}",
            self.code.unwrap_or("<без sqlstate>"),
            self.message
        )
    }
}
impl StdError for FakeDb {}

impl DatabaseError for FakeDb {
    fn message(&self) -> &str {
        self.message
    }
    fn code(&self) -> Option<Cow<'_, str>> {
        self.code.map(Cow::Borrowed)
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
    fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
        self
    }
    fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
        self
    }
    fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
        self
    }
}

fn fake(code: &'static str, message: &'static str) -> Error {
    Error::Database(Box::new(FakeDb {
        code: Some(code),
        message,
    }))
}

/// Фейкова помилка БД, у якої PostgreSQL не дав SQLSTATE (`code() == None`).
fn fake_no_code(message: &'static str) -> Error {
    Error::Database(Box::new(FakeDb {
        code: None,
        message,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// (i) ЮНІТ: 25006 vs інший код, нормалізація, маркер, метрики
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn sqlstate_25006_is_read_only_other_codes_are_not() {
    let ro = fake(
        SQLSTATE_READ_ONLY,
        "cannot execute INSERT in a read-only transaction",
    );
    assert!(
        is_read_only_replica(&ro),
        "SQLSTATE 25006 мусить розпізнаватись як «запис у репліку»"
    );

    let unique = fake("23505", "duplicate key value violates unique constraint");
    assert!(
        !is_read_only_replica(&unique),
        "23505 (unique violation) — НЕ read-only репліка"
    );

    assert!(
        !is_read_only_replica(&Error::RowNotFound),
        "не-БД помилка не є read-only реплікою"
    );
}

#[test]
fn normalize_replaces_plain_pg_error_and_keeps_marker_without_pg_text() {
    let before = hits();
    let raw = fake(
        SQLSTATE_READ_ONLY,
        "cannot execute UPDATE in a read-only transaction",
    );
    // Сигнал сирої помилки PG (те, що бачить користувач СЬОГОДНІ).
    assert!(
        format!("{raw}").contains("cannot execute"),
        "сирий текст PG у вихідній помилці"
    );

    let e = normalize_with_sql(raw, Some("UPDATE prro_queue_items SET status = $1"));
    assert!(
        is_read_only_replica(&e),
        "після normalize клас зберігається (типізований маркер)"
    );
    let msg = format!("{e}");
    assert!(msg.contains(MARKER), "Display мусить містити маркер: {msg}");
    assert!(
        !msg.contains("cannot execute") && !msg.contains("read-only transaction"),
        "жодного фрагмента тексту PostgreSQL у Display: {msg}"
    );
    assert!(
        msg.contains("standby") && msg.contains(SQLSTATE_READ_ONLY),
        "людський текст українською + код: {msg}"
    );
    assert!(
        hits() > before,
        "лічильник перехоплень мусить зрости: {} → {}",
        before,
        hits()
    );
    println!("[readonly_guard] людський текст: {msg}");
    println!("[readonly_guard] hits()={}", hits());
}

#[test]
fn normalize_leaves_other_errors_untouched() {
    let other = fake("42P01", "relation \"nope\" does not exist");
    let e = normalize(other);
    assert!(!is_read_only_replica(&e));
    assert!(
        format!("{e}").contains("does not exist"),
        "не-read-only помилка не підмінюється"
    );
    // Відсутній SQL → порожній fingerprint, але клас і маркер зберігаються.
    let e = normalize(fake(SQLSTATE_READ_ONLY, "read only"));
    assert!(is_read_only_replica(&e));
    assert!(format!("{e}").contains(MARKER));
}

#[test]
fn typed_error_exposes_sqlstate_and_marker() {
    let typed = ReadOnlyReplicaError {
        fingerprint: fingerprint("INSERT INTO outbox_write (a) VALUES ($1)"),
    };
    assert_eq!(
        typed.code().as_deref(),
        Some(SQLSTATE_READ_ONLY),
        "code() → SQLSTATE 25006 (стабільний код, не текст)"
    );
    assert_eq!(typed.kind(), ErrorKind::Other);
    assert!(typed.to_string().contains(MARKER));
    let boxed: Box<dyn DatabaseError> = Box::new(typed);
    assert!(
        boxed.try_downcast_ref::<ReadOnlyReplicaError>().is_some(),
        "типізований маркер видно через dyn DatabaseError"
    );
    let e = Error::Database(boxed);
    assert!(is_read_only_replica(&e));
}

#[test]
fn fingerprint_groups_instances_of_the_same_query() {
    let a = fingerprint("INSERT INTO receipts (id, total) VALUES ($1, $2)");
    let b = fingerprint("INSERT INTO receipts (id, total) VALUES ($11, $12)");
    assert_eq!(a, b);
    assert!(a.len() <= 80);
}

// ─────────────────────────────────────────────────────────────────────────────
// (i-b) СЛУЖБОВІ ХЕЛПЕРИ ШАРУ ОБРОБНИКІВ: машинний код помилки БД без тексту PG
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn db_error_class_25006_contains_sqlstate_without_pg_text() {
    let e = fake(
        SQLSTATE_READ_ONLY,
        "cannot execute CREATE TABLE in a read-only transaction",
    );
    assert_eq!(sqlstate_of(&e).as_deref(), Some(SQLSTATE_READ_ONLY));
    assert_eq!(db_error_class(&e), "[DB_ERROR 25006]");
    assert!(
        !db_error_class(&e).contains("cannot execute"),
        "у машинному коді немає тексту PostgreSQL"
    );
    println!(
        "[readonly_guard] db_error_class(25006) = {}",
        db_error_class(&e)
    );
}

#[test]
fn db_error_class_without_sqlstate_is_bare() {
    let e = fake_no_code("вигадана помилка БД без коду");
    assert_eq!(sqlstate_of(&e), None);
    assert_eq!(db_error_class(&e), "[DB_ERROR]");
    println!(
        "[readonly_guard] db_error_class(без коду) = {}",
        db_error_class(&e)
    );
}

#[test]
fn sqlstate_of_non_pg_error_is_none() {
    let e = Error::Protocol("вигадана помилка протоколу (не PG)".to_string());
    assert_eq!(sqlstate_of(&e), None);
    assert_eq!(db_error_class(&e), "[DB_ERROR]");
    println!(
        "[readonly_guard] sqlstate_of(не-PG) = {:?}",
        sqlstate_of(&e)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (ii) РЕАЛЬНИЙ PG: read-only СЕСІЯ дає 25006 (доказ без фізичної репліки)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn real_postgres_read_only_session_raises_25006_and_is_normalized() {
    let base: PgPool = torgashka_infrastructure::db::connect_test_pool(2)
        .await
        .expect("тестова БД (TEST_DATABASE_URL або <db>_test)");
    // Прибираємо залишок попередніх прогонів writable-пулом.
    let _ = sqlx::query("DROP TABLE IF EXISTS torgashka_readonly_probe")
        .execute(&base)
        .await;

    // ТА САМА БД, але сесія — read-only (як репліка/standby-вузол).
    // ТА САМА БД, але сесія — read-only (як репліка/standby-вузол):
    // startup-опція `-c default_transaction_read_only=on`.
    let opts = (*base.connect_options())
        .clone()
        .options([("default_transaction_read_only", "on")]);
    let ro = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect_with(opts)
        .await
        .expect("read-only сесія");

    let session_ro: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(&ro)
        .await
        .expect("SHOW transaction_read_only");
    assert_eq!(session_ro, "on", "сесія мусить бути read-only");

    let raw = sqlx::query("CREATE TABLE torgashka_readonly_probe(x int)")
        .execute(&ro)
        .await
        .expect_err("CREATE TABLE у read-only сесії мусить впасти");
    let code = raw
        .as_database_error()
        .and_then(|d| d.code())
        .map(|c| c.to_string());
    println!("[readonly_guard] сира помилка PG: {raw}");
    println!("[readonly_guard] SQLSTATE: {code:?}");
    assert_eq!(
        code.as_deref(),
        Some(SQLSTATE_READ_ONLY),
        "PostgreSQL мусить віддати саме 25006: {raw}"
    );
    assert!(
        is_read_only_replica(&raw),
        "клас розпізнано за SQLSTATE (без парсингу тексту)"
    );

    let before = hits();
    let e = normalize_with_sql(raw, Some("CREATE TABLE torgashka_readonly_probe(x int)"));
    let msg = format!("{e}");
    assert!(is_read_only_replica(&e));
    assert!(msg.contains(MARKER), "маркер у Display: {msg}");
    assert!(
        !msg.contains("cannot execute"),
        "текст PostgreSQL не потрапляє у Display: {msg}"
    );
    assert!(hits() > before, "метрика перехоплення зросла");
    println!("[readonly_guard] нормалізовано у: {msg}");
    println!(
        "[readonly_guard] top fingerprints: {:?}",
        torgashka_infrastructure::readonly_guard::hits_by_fingerprint()
    );
    println!(
        "[readonly_guard] last_hit: {:?}",
        torgashka_infrastructure::readonly_guard::last_hit()
    );
}
