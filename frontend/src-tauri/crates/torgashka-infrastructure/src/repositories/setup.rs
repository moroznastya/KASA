//! SQL-репозиторій setup (Частина 1 + Частина 2): перший власник.
//!
//! Повторює патерн `repositories/stores.rs` та `repositories/auth.rs`:
//! `Sqlx*Service` в infrastructure + trait у torgashka-domain.
//!
//! ## Частина 1 — перший власник (fresh install)
//! `POST /api/v1/setup` в ОДНІЙ транзакції мета-БД:
//!   - users      (role='owner', onboarding_completed=true, is_active=true)
//!   - stores     (перша точка)
//!   - user_stores(owner, role='owner', is_default=true)
//!     Потім видає JWT через API-шар (torgashka-api/setup.rs) — одразу авторизує.
//!
//! ## Частина 2 — окрема чиста БД кожного власника
//! При першому setup створюється шаблонна БД `torgashka_template`
//! (CREATE DATABASE ... TEMPLATE template0 + повна схема), наступні власники
//! отримують `torgashka_owner_<first8>`, клоновану через
//! `CREATE DATABASE ... TEMPLATE torgashka_template`. Запис — у owners_db.
//!
//! ⚠️ TODO (мінімально-життєздатна реалізація): повна маршрутизація
//!    бізнес-запитів у персональну БД власника НЕ реалізована — фасад поки
//!    працює з мета-БД (як раніше). Персональні БД створюються й реєструються.
//!
//! ⚠️ RLS: `stores`/`user_stores` мають RLS-політики (0004_rls), але таблиці
//!    належать користувачу з'єднання (postgres) → власник таблиці оминає RLS.
//!    При підключенні НЕ-власником (напр. kasa у docker) INSERT у stores був
//!    би заблокований політикою — це відоме обмеження (поточна конфігурація
//!    локально працює як postgres).

use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::db::{replace_dbname, resolve_database_url, SCHEMA_SQL};
use crate::store_ctx::StorePool;
use torgashka_domain::{
    owner_permissions, owner_user_dto, LoginResult, SetupError, SetupRequest, SetupService,
    SetupStatusDto,
};

/// Назва шаблонної БД (джерело для клонування персональних БД власників).
pub const TEMPLATE_DB_NAME: &str = "torgashka_template";

/// SQLx-реалізація setup-сервісу поверх спільного пулу мета-БД.
#[derive(Clone)]
pub struct SqlxSetupService {
    pool: StorePool,
}

impl SqlxSetupService {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }
}

/// Map sqlx::Error → SetupError.
trait SqlxResultExt<T> {
    fn se(self) -> Result<T, SetupError>;
}

impl<T> SqlxResultExt<T> for Result<T, sqlx::Error> {
    fn se(self) -> Result<T, SetupError> {
        self.map_err(|e| SetupError::Infrastructure(e.to_string()))
    }
}

/// Quote SQL-ідентифікатора (для db_name — генерується з hex UUID, але
/// захищаємось від ін'єкцій на майбутнє).
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Чи існує БД у PostgreSQL.
async fn database_exists(pool: &StorePool, db_name: &str) -> Result<bool, SetupError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(db_name)
            .fetch_one(pool)
            .await
            .se()?;
    Ok(exists)
}

/// Створює шаблонну БД torgashka_template (TEMPLATE template0) і застосовує
/// повну схему. Ідемпотентно: якщо вже існує — нічого не робить.
async fn ensure_template_database(pool: &StorePool) -> Result<(), SetupError> {
    if database_exists(pool, TEMPLATE_DB_NAME).await? {
        return Ok(());
    }
    // CREATE DATABASE неможливий у транзакції — окремий запит на з'єднанні пулу.
    sqlx::raw_sql(&format!(
        "CREATE DATABASE {} TEMPLATE template0",
        quote_ident(TEMPLATE_DB_NAME)
    ))
    .execute(pool)
    .await
    .map_err(|e| {
        SetupError::Infrastructure(format!(
            "Не вдалося створити шаблонну БД '{TEMPLATE_DB_NAME}': {e}. \
             Переконайтеся, що користувач БД має право CREATE DATABASE"
        ))
    })?;

    // Застосувати схему до шаблону (окреме з'єднання на БД torgashka_template).
    let base = resolve_database_url().map_err(|e| SetupError::Infrastructure(e.to_string()))?;
    let tpl_url = replace_dbname(&base, TEMPLATE_DB_NAME)
        .map_err(|e| SetupError::Infrastructure(e.to_string()))?;
    let tpl_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&tpl_url)
        .await
        .map_err(|e| {
            SetupError::Infrastructure(format!(
                "Не вдалося підключитись до шаблонної БД '{TEMPLATE_DB_NAME}': {e}"
            ))
        })?;
    let schema_result = sqlx::raw_sql(SCHEMA_SQL).execute(&tpl_pool).await;
    tpl_pool.close().await;
    if let Err(e) = schema_result {
        // Прибрати частковий шаблон, щоб наступна спроба почалась з чистого аркуша.
        let _ = sqlx::raw_sql(&format!(
            "DROP DATABASE IF EXISTS {}",
            quote_ident(TEMPLATE_DB_NAME)
        ))
        .execute(pool)
        .await;
        return Err(SetupError::Infrastructure(format!(
            "Не вдалося застосувати схему до шаблонної БД '{TEMPLATE_DB_NAME}': {e}"
        )));
    }
    eprintln!(
        "[torgashka-infrastructure] шаблонна БД '{TEMPLATE_DB_NAME}' створена (схема застосована)"
    );
    Ok(())
}

/// Створює персональну БД власника через шаблон + GRANT поточному користувачу.
async fn create_owner_database(pool: &StorePool, owner_id: Uuid) -> Result<String, SetupError> {
    ensure_template_database(pool).await?;
    let short = owner_id.simple().to_string();
    let db_name = format!("torgashka_owner_{}", &short[..8]);
    if !database_exists(pool, &db_name).await? {
        sqlx::raw_sql(&format!(
            "CREATE DATABASE {} TEMPLATE {}",
            quote_ident(&db_name),
            quote_ident(TEMPLATE_DB_NAME)
        ))
        .execute(pool)
        .await
        .map_err(|e| {
            SetupError::Infrastructure(format!(
                "Не вдалося створити персональну БД '{db_name}': {e}. \
                 Переконайтеся, що користувач БД має право CREATE DATABASE"
            ))
        })?;
        // Дозволи: GRANT ALL поточному користувачу БД (best-effort — superuser
        // і так має всі права; для обмеженого користувача це обов'язково).
        let current_user: String = sqlx::query_scalar("SELECT current_user")
            .fetch_one(pool)
            .await
            .se()?;
        let _ = sqlx::raw_sql(&format!(
            "GRANT ALL ON DATABASE {} TO {}",
            quote_ident(&db_name),
            quote_ident(&current_user)
        ))
        .execute(pool)
        .await;
    }
    Ok(db_name)
}

#[async_trait::async_trait]
impl SetupService for SqlxSetupService {
    async fn status(&self) -> Result<SetupStatusDto, SetupError> {
        let has_users: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users LIMIT 1)")
            .fetch_one(&self.pool)
            .await
            .se()?;
        Ok(SetupStatusDto {
            status: if has_users {
                "initialized".to_string()
            } else {
                "not_initialized".to_string()
            },
        })
    }

    async fn setup(&self, input: &SetupRequest) -> Result<LoginResult, SetupError> {
        // ── Валідації ──────────────────────────────────────────────────────
        let name = input.name.trim();
        let login = input.login.trim();
        let store_name = input.store_name.trim();
        if name.is_empty() {
            return Err(SetupError::BadRequest(
                "Ім'я власника обов'язкове".to_string(),
            ));
        }
        if login.is_empty() {
            return Err(SetupError::BadRequest("Логін обов'язковий".to_string()));
        }
        if input.password.len() < 6 {
            return Err(SetupError::BadRequest(
                "Пароль має містити щонайменше 6 символів".to_string(),
            ));
        }
        if store_name.is_empty() {
            return Err(SetupError::BadRequest(
                "Назва торговельної точки обов'язкова".to_string(),
            ));
        }

        // ── Конкурентний захист: система вже ініціалізована? ───────────────
        let has_users: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users LIMIT 1)")
            .fetch_one(&self.pool)
            .await
            .se()?;
        if has_users {
            return Err(SetupError::Conflict(
                "Систему вже ініціалізовано".to_string(),
            ));
        }
        let login_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE login = $1)")
                .bind(login)
                .fetch_one(&self.pool)
                .await
                .se()?;
        if login_exists {
            return Err(SetupError::Conflict(
                "Користувач з таким логіном вже існує".to_string(),
            ));
        }

        // ── Підготовка даних ───────────────────────────────────────────────
        // bcrypt — той самий механізм, що в repositories/auth.rs (cost 12).
        let password_hash = bcrypt::hash(&input.password, 12)
            .map_err(|e| SetupError::Infrastructure(e.to_string()))?;
        let owner_id = Uuid::new_v4();
        let store_id = Uuid::new_v4();
        let now = Utc::now().naive_utc();
        let permissions = owner_permissions();
        let permissions_json = serde_json::json!(permissions);

        // ── Транзакція мета-БД: users + stores + user_stores ───────────────
        let mut tx = self.pool.begin().await.se()?;
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, pin_code, role, is_active, permissions, onboarding_completed, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, NULL, $5::user_role, $6, $7, true, $8, $9)",
        )
        .bind(owner_id)
        .bind(name)
        .bind(login)
        .bind(&password_hash)
        .bind("owner")
        .bind(true)
        .bind(&permissions_json)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .se()?;

        sqlx::query(
            "INSERT INTO stores (id, name, address, phone, is_active, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, true, $5, $6)",
        )
        .bind(store_id)
        .bind(store_name)
        .bind(input.store_address.as_deref())
        .bind(input.store_phone.as_deref())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .se()?;

        sqlx::query(
            "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at) \
             VALUES ($1, $2, 'owner', $3, true, $4)",
        )
        .bind(owner_id)
        .bind(store_id)
        .bind(&permissions_json)
        .bind(now)
        .execute(&mut *tx)
        .await
        .se()?;
        tx.commit().await.se()?;

        // ── Частина 2: персональна БД власника через шаблон ────────────────
        // (CREATE DATABASE поза транзакцією — обмеження PostgreSQL.)
        let db_name = create_owner_database(&self.pool, owner_id).await?;
        sqlx::query("INSERT INTO owners_db (owner_id, db_name, created_at) VALUES ($1, $2, $3)")
            .bind(owner_id)
            .bind(&db_name)
            .bind(now)
            .execute(&self.pool)
            .await
            .se()?;

        Ok(LoginResult {
            access_token: String::new(), // API-шар заповнює JWT
            refresh_token: String::new(),
            token_type: "bearer".to_string(),
            user: owner_user_dto(owner_id, name, login, &permissions, now),
        })
    }
}
