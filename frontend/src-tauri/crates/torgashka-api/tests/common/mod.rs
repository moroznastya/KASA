//! Спільний хелпер e2e-тестів torgashka-api: ІЗОЛЬОВАНА тестова БД.
//!
//! QA §5.2 (гігієна тестів): e2e підключались через `resolve_database_url()`
//! → робоча dev-БД `pos_system_fresh` (тестові дані забруднювали робочу БД).
//! Кожен e2e викликає [`force_test_db`] НАЙПЕРШИМ рядком: env `DATABASE_URL`
//! перемикається на `<dbname>_test` (патерн `connect_test_pool` з
//! torgashka-infrastructure: TEST_DATABASE_URL або робочий URL + _test).
//! Жоден e2e не пише в робочу БД.

use std::sync::Once;

static ONCE: Once = Once::new();

fn dbname_from_url(url: &str) -> Option<String> {
    let before = url.split('?').next().unwrap_or(url);
    let idx = before.rfind('/')?;
    let name = &before[idx + 1..];
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// URL тестової БД: TEST_DATABASE_URL → інакше робочий URL з ім'ям + "_test".
fn test_db_url() -> String {
    if let Ok(u) = std::env::var("TEST_DATABASE_URL") {
        if !u.trim().is_empty() {
            return u;
        }
    }
    let work = torgashka_infrastructure::db::resolve_database_url()
        .expect("resolve_database_url: задайте DATABASE_URL або DB_* у backend/.env");
    let name = dbname_from_url(&work).expect("робочий URL без імені БД");
    if name.contains("test") {
        work
    } else {
        // postgresql://u:p@host:port/pos_system_fresh → .../pos_system_fresh_test
        let idx = work.rfind('/').expect("слеш");
        let (head, _) = work.split_at(idx + 1);
        format!("{head}{name}_test")
    }
}

/// Перемкнути процес e2e на тестову БД (один раз на тест-бінар).
pub fn force_test_db() {
    ONCE.call_once(|| {
        let url = test_db_url();
        let final_name = dbname_from_url(&url).expect("url");
        assert!(
            final_name.contains("test"),
            "e2e заборонено проти робочої БД '{final_name}'"
        );
        // resolve_database_url читає env DATABASE_URL ПЕРШИМ пріоритетом —
        // run_facade і всі пули процесу тепер на тестовій БД.
        std::env::set_var("DATABASE_URL", url.clone());
        eprintln!("[e2e] БД: {url}");
    });
}
