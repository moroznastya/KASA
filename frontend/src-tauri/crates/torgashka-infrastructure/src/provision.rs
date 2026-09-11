//! provision — бекенд «provisioning БД» (Етап 1 sync-offline, гілка sync-offline).
//!
//! Виконується за запитом ВЛАСНИКА мережі (POST /api/v1/admin/db-sources/provision)
//! на цільовому кластері PostgreSQL:
//!   1. підключення СУПЕРкористувачем до БД `postgres` (SELECT 1 — перевірка);
//!   2. CREATE DATABASE <database> TEMPLATE template0 (409, якщо вже існує);
//!   3. накатування ПОВНОЇ схеми (crate::db::SCHEMA_SQL — єдине джерело,
//!      жодних копій SQL; той самий механізм, що в repositories/setup.rs);
//!   4. створення/оновлення ролі `torgashka_app` (NOSUPERUSER NOBYPASSRLS
//!      NOCREATEDB NOCREATEROLE) + GRANT CONNECT/USAGE/таблиці/секвенси/
//!      default privileges НА НОВУ БД (семантика scripts/create_app_role.sql —
//!      через код, без psql-залежності);
//!   5. при частковому збої після CREATE DATABASE — DROP DATABASE IF EXISTS
//!      (cleanup), щоб повторна спроба була чистою.
//!
//! # Безпека
//! - суперкористувацькі кредити — ОДНОРАЗОВІ: приймаються в пам'ять, нікуди
//!   не зберігаються і не логуються (у повідомленнях про помилки URL немає);
//! - пароль ролі `torgashka_app` генерується випадково (24 байти → hex 48);
//!   повертається у [`ProvisionOutcome`] лише в пам'ять — API-шар шифрує його
//!   (AES-256-GCM) перед записом у db_sources.toml;
//! - для не-localhost host у URL додається `sslmode=require`.

use std::time::Duration;

use rand::RngCore;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::db::{database_exists, quote_ident, SCHEMA_SQL};

/// Роль додатка, під якою працює Rust-фасад (спільна з scripts/create_app_role.sql).
pub const APP_ROLE: &str = "torgashka_app";

/// Регулярка безпечного імені нової БД: ^[a-z_][a-z0-9_]{0,62}$ (PG limit 63).
fn db_name_valid(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }
    let mut n = 1usize;
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
        n += 1;
        if n > 63 {
            return false;
        }
    }
    !name.is_empty()
}

// ─────────────────────────────────────────────────────────────────────────────
// Помилки
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("Не вдалося підключитись до {host}:{port} суперкористувачем: {reason}")]
    Connect {
        host: String,
        port: u16,
        reason: String,
    },
    #[error("База даних '{0}' уже існує на цільовому сервері")]
    DatabaseExists(String),
    #[error("{0}")]
    Failed(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Вхід/вихід
// ─────────────────────────────────────────────────────────────────────────────

/// Одноразові суперкористувацькі кредити (ніколи не зберігаються).
pub struct SuperuserCreds {
    pub user: String,
    pub password: String,
}

pub struct ProvisionTarget {
    pub host: String,
    pub port: u16,
    /// Ім'я НОВОЇ БД (вже провалідоване на рівні API; тут — defense in depth).
    pub database: String,
    pub superuser: SuperuserCreds,
}

pub struct ProvisionOutcome {
    pub database: String,
    pub app_user: String,
    /// Згенерований пароль ролі torgashka_app (plaintext, ЛИШЕ в пам'яті).
    pub app_password: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Хелпери
// ─────────────────────────────────────────────────────────────────────────────

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn pct(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// URL підключення до вказаної БД цільового кластера суперкористувачем.
/// Для не-localhost host — обов'язково sslmode=require. Пароль — лише в URL
/// у пам'яті процесу; у помилки/логи URL ніколи не потрапляє.
fn super_url(target: &ProvisionTarget, database: &str) -> String {
    let host = if target.host.contains(':') && !target.host.starts_with('[') {
        format!("[{}]", target.host)
    } else {
        target.host.clone()
    };
    let mut url = format!(
        "postgresql://{}:{}@{}/{}",
        pct(&target.superuser.user),
        pct(&target.superuser.password),
        host,
        database
    );
    if !is_local_host(&target.host) {
        url.push_str("?sslmode=require");
    }
    url
}

fn generate_app_password() -> String {
    let mut bytes = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(48);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Підключення з жорстким таймаутом (6 c) — недосяжний host/невірні кредити
/// не підвішують запит на хвилини (дефолтний connect timeout sqlx).
async fn connect_pool(url: &str) -> Result<PgPool, String> {
    let fut = async {
        PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(4))
            .connect(url)
            .await
    };
    match tokio::time::timeout(Duration::from_secs(6), fut).await {
        Ok(Ok(p)) => Ok(p),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            Err("таймаут з'єднання (6 c): host/port недосяжні або не відповідають".to_string())
        }
    }
}

/// SELECT 1 на пулі з таймаутом.
async fn select_one(pool: &PgPool) -> Result<i32, String> {
    let fut = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(pool);
    match tokio::time::timeout(Duration::from_secs(5), fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("таймаут SELECT 1 (5 c)".to_string()),
    }
}

/// DROP DATABASE IF EXISTS — cleanup новоствореної БД при частковому збої,
/// щоб повторна спроба provision почалась з чистого аркуша.
async fn cleanup_drop(pool: &PgPool, db_name: &str) {
    let _ = sqlx::raw_sql(&format!("DROP DATABASE IF EXISTS {}", quote_ident(db_name)))
        .execute(pool)
        .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Основний вхід
// ─────────────────────────────────────────────────────────────────────────────

/// Створює НОВУ БД на цільовому кластері + повну схему + роль torgashka_app
/// з GRANT-ами. При будь-якому збої після CREATE DATABASE — БД прибирається.
pub async fn provision_database(
    target: &ProvisionTarget,
) -> Result<ProvisionOutcome, ProvisionError> {
    let db_name = target.database.trim();
    if !db_name_valid(db_name) {
        return Err(ProvisionError::Failed(format!(
            "Невірне ім'я нової БД '{db_name}' (дозволені: ^[a-z_][a-z0-9_]{{0,62}}$)"
        )));
    }
    if target.superuser.user.trim().is_empty() {
        return Err(ProvisionError::Failed(
            "superuser.user не може бути порожнім".to_string(),
        ));
    }

    // ── 1. Підключення суперкористувачем до БД postgres + SELECT 1 ─────────
    let admin_url = super_url(target, "postgres");
    let admin_pool = connect_pool(&admin_url)
        .await
        .map_err(|reason| ProvisionError::Connect {
            host: target.host.clone(),
            port: target.port,
            reason,
        })?;
    select_one(&admin_pool)
        .await
        .map_err(|reason| ProvisionError::Connect {
            host: target.host.clone(),
            port: target.port,
            reason,
        })?;

    // ── 2. Конфлікт імені → 409 ────────────────────────────────────────────
    let exists = database_exists(&admin_pool, db_name)
        .await
        .map_err(|e| ProvisionError::Failed(format!("перевірка pg_database: {e}")))?;
    if exists {
        admin_pool.close().await;
        return Err(ProvisionError::DatabaseExists(db_name.to_string()));
    }

    // ── 3. CREATE DATABASE ... TEMPLATE template0 ───────────────────────────
    // CREATE DATABASE неможливий у транзакції — окремий raw-запит.
    if let Err(e) = sqlx::raw_sql(&format!(
        "CREATE DATABASE {} TEMPLATE template0",
        quote_ident(db_name)
    ))
    .execute(&admin_pool)
    .await
    {
        admin_pool.close().await;
        return Err(ProvisionError::Failed(format!(
            "Не вдалося створити БД '{db_name}': {e}. Переконайтеся, що суперкористувач \
             має право CREATE DATABASE і кластер доступний"
        )));
    }

    // ── 4. Повна схема в нову БД (SCHEMA_SQL — спільне джерело з setup.rs) ──
    let new_db_url = super_url(target, db_name);
    let new_pool = match connect_pool(&new_db_url).await {
        Ok(p) => p,
        Err(reason) => {
            cleanup_drop(&admin_pool, db_name).await;
            admin_pool.close().await;
            return Err(ProvisionError::Failed(format!(
                "БД '{db_name}' створено, але не вдалося підключитись для схеми: {reason}"
            )));
        }
    };
    if let Err(e) = sqlx::raw_sql(SCHEMA_SQL).execute(&new_pool).await {
        new_pool.close().await;
        cleanup_drop(&admin_pool, db_name).await;
        admin_pool.close().await;
        return Err(ProvisionError::Failed(format!(
            "БД '{db_name}' створено, але схема не застосувалась (cleanup виконано): {e}"
        )));
    }

    // ── 5. Роль torgashka_app + автогенерований пароль (ідемпотентно) ───────
    let app_password = generate_app_password();
    let role_exists: bool =
        match sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(APP_ROLE)
            .fetch_one(&admin_pool)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                new_pool.close().await;
                cleanup_drop(&admin_pool, db_name).await;
                admin_pool.close().await;
                return Err(ProvisionError::Failed(format!(
                    "перевірка pg_roles (cleanup виконано): {e}"
                )));
            }
        };
    let role_sql = if role_exists {
        format!(
            "ALTER ROLE {} WITH LOGIN PASSWORD '{app_password}' \
             NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE",
            quote_ident(APP_ROLE)
        )
    } else {
        format!(
            "CREATE ROLE {} LOGIN PASSWORD '{app_password}' \
             NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE",
            quote_ident(APP_ROLE)
        )
    };
    // Пароль — hex (48 символів) → екранування не потрібне.
    if let Err(e) = sqlx::raw_sql(&role_sql).execute(&admin_pool).await {
        new_pool.close().await;
        cleanup_drop(&admin_pool, db_name).await;
        admin_pool.close().await;
        return Err(ProvisionError::Failed(format!(
            "Роль '{APP_ROLE}' не створено/оновлено (cleanup виконано): {e}"
        )));
    }

    // ── 6. GRANT-и (семантика scripts/create_app_role.sql, через код) ───────
    // CONNECT ON DATABASE — кластерний рівень (з'єднання до postgres).
    let connect_grant = format!(
        "GRANT CONNECT ON DATABASE {} TO {}",
        quote_ident(db_name),
        quote_ident(APP_ROLE)
    );
    // USAGE/таблиці/секвенси/default privileges — рівень БД (нова БД).
    let db_grants = format!(
        "GRANT USAGE ON SCHEMA public TO {};
         GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {};
         GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {};
         ALTER DEFAULT PRIVILEGES IN SCHEMA public
             GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {};
         ALTER DEFAULT PRIVILEGES IN SCHEMA public
             GRANT USAGE, SELECT ON SEQUENCES TO {};",
        quote_ident(APP_ROLE),
        quote_ident(APP_ROLE),
        quote_ident(APP_ROLE),
        quote_ident(APP_ROLE),
        quote_ident(APP_ROLE)
    );
    let grant_connect = sqlx::raw_sql(&connect_grant).execute(&admin_pool).await;
    if let Err(e) = grant_connect {
        new_pool.close().await;
        cleanup_drop(&admin_pool, db_name).await;
        admin_pool.close().await;
        return Err(ProvisionError::Failed(format!(
            "GRANT CONNECT на БД '{db_name}' не виконано (cleanup виконано): {e}"
        )));
    }
    if let Err(e) = sqlx::raw_sql(&db_grants).execute(&new_pool).await {
        new_pool.close().await;
        cleanup_drop(&admin_pool, db_name).await;
        admin_pool.close().await;
        return Err(ProvisionError::Failed(format!(
            "GRANT-и на БД '{db_name}' не виконано (cleanup виконано): {e}"
        )));
    }

    new_pool.close().await;
    admin_pool.close().await;
    Ok(ProvisionOutcome {
        database: db_name.to_string(),
        app_user: APP_ROLE.to_string(),
        app_password,
    })
}

/// Best-effort cleanup БД, створеної provision-ом (викликається API-шаром,
/// якщо після УСПІШНОГО провіжинінгу не вдалося зберегти джерело у файл —
/// щоб повторна спроба була чистою). Кредити — з того самого запиту.
pub async fn cleanup_provisioned_database(target: &ProvisionTarget) {
    let url = super_url(target, "postgres");
    if let Ok(pool) = connect_pool(&url).await {
        cleanup_drop(&pool, target.database.trim()).await;
        pool.close().await;
    }
}
