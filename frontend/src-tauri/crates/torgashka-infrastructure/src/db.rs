//! PostgreSQL-підключення (етап 1 — довідники READ).
//!
//! ТІЛЬКИ читання: пул `sqlx::PgPool`, жодних міграцій і жодних write-операцій.
//! DSN береться з env `DATABASE_URL`; fallback — компоненти `DB_*` з
//! `backend/.env` (спільна конфігурація з Python-бекендом).

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Помилки підключення до БД.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("DATABASE_URL не знайдено: задайте DATABASE_URL або DB_* у backend/.env")]
    MissingUrl,
    #[error("помилка читання конфігурації: {0}")]
    Io(#[from] std::io::Error),
    #[error("помилка підключення до PostgreSQL: {0}")]
    Sqlx(#[from] sqlx::Error),
}

/// Кандидати шляхів до `backend/.env` (залежно від CWD запуску).
fn env_file_candidates() -> Vec<std::path::PathBuf> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        // CWD = torgashka/ (репозиторій проєкту)
        std::path::PathBuf::from("backend/.env"),
        // CWD = crates/ (запуск тестів з кореня workspace)
        std::path::PathBuf::from("../../backend/.env"),
        // CWD = frontend/src-tauri (запуск фасаду через Tauri / cargo run)
        std::path::PathBuf::from("../../../backend/.env"),
        // CWD = crates/torgashka-infrastructure (запуск тестів цього крейта)
        std::path::PathBuf::from("../../../../backend/.env"),
        // Абсолютний шлях від маніфесту цього крейта (torgashka-infrastructure).
        manifest.join("../../../../backend/.env"),
    ]
}

/// Примітивний парсер значення з .env-файлу (без зовнішніх залежностей).
pub(crate) fn parse_env_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        if k.trim() == key {
            Some(v.trim().trim_matches('"').trim_matches('\'').to_string())
        } else {
            None
        }
    })
}

/// Резолв DATABASE_URL: env `DATABASE_URL` → `backend/.env` (DB_*) → Err.
pub fn resolve_database_url() -> Result<String, DbError> {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.trim().is_empty() {
            return Ok(url);
        }
    }
    for candidate in env_file_candidates() {
        if let Ok(content) = std::fs::read_to_string(&candidate) {
            let get = |k: &str| parse_env_value(&content, k);
            if let (Some(host), Some(port), Some(user), Some(pass), Some(db)) = (
                get("DB_HOST"),
                get("DB_PORT"),
                get("DB_USER"),
                get("DB_PASSWORD"),
                get("DB_NAME"),
            ) {
                return Ok(format!("postgresql://{user}:{pass}@{host}:{port}/{db}"));
            }
        }
    }
    Err(DbError::MissingUrl)
}

/// Створює пул читання PostgreSQL.
///
/// - `max_connections` — розмір пулу (за замовчуванням 5 — read-навантаження легке).
/// - `acquire_timeout` — 5 секунд, щоб фасад швидко віддав помилку, а не висів.
pub async fn connect_readonly_pool(max_connections: u32) -> Result<PgPool, DbError> {
    let url = resolve_database_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await?;
    Ok(pool)
}
