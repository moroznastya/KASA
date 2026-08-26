//! SQL-репозиторій auth (етап 6): користувачі, робочі сесії, налаштування.
//!
//! 1:1 з Python v1:
//!   - login/login-pin: bcrypt verify, закриття попередніх активних сесій,
//!     створення нової робочої сесії (як users.py _close_active_work_sessions)
//!   - refresh/verify: перевірка існування та активності користувача
//!   - users CRUD: авто-генерація логіну (транслітерація), bcrypt hash,
//!     409 на дублікат, 409 на видалення з чеками/себе
//!   - settings: GET за модулями, batch update (тільки існуючі ключі),
//!     upsert за ключем (авто module/value_type/label)
//!
//! JWT-генерація — НЕ тут: API-шар (torgashka-api/auth_routes.rs) має секрет і
//! створює токени тим самим форматом/секретом, що Python.

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::store_ctx::StorePool;
use torgashka_domain::{
    generate_login_from_name, AuthError, AuthService, LoginPinRequest, LoginRequest, LoginResult,
    PublicUserDto, SettingDto, SettingsModulesDto, UserCreateInput, UserDto, UserListDto,
    UserUpdateInput,
};
use uuid::Uuid;

/// SQL-репозиторій auth поверх спільного пулу PostgreSQL.
#[derive(Clone)]
pub struct SqlxAuth {
    pool: StorePool,
}

impl SqlxAuth {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }
}

/// Рядок користувача з БД (повний набір полів для UserDto).
struct UserRow {
    id: Uuid,
    name: String,
    login: String,
    role: String,
    is_active: bool,
    onboarding_completed: bool,
    permissions: Option<serde_json::Value>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl UserRow {
    fn to_dto(&self) -> UserDto {
        UserDto {
            id: self.id,
            name: self.name.clone(),
            login: self.login.clone(),
            role: self.role.clone(),
            is_active: self.is_active,
            onboarding_completed: self.onboarding_completed,
            permissions: self.permissions_vec(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    fn permissions_vec(&self) -> Option<Vec<String>> {
        self.permissions.as_ref().and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
        })
    }
}

async fn fetch_user(pool: &StorePool, user_id: Uuid) -> Result<Option<UserRow>, AuthError> {
    let row = sqlx::query_as::<_, (Uuid, String, String, String, bool, bool, Option<serde_json::Value>, NaiveDateTime, NaiveDateTime)>(
        "SELECT id, name, login, role::text, is_active, onboarding_completed, permissions, created_at, updated_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
    Ok(row.map(|r| UserRow {
        id: r.0,
        name: r.1,
        login: r.2,
        role: r.3,
        is_active: r.4,
        onboarding_completed: r.5,
        permissions: r.6,
        created_at: r.7,
        updated_at: r.8,
    }))
}

/// Закриває всі активні сесії користувача (Python _close_active_work_sessions).
async fn close_active_work_sessions(pool: &StorePool, user_id: Uuid) -> Result<(), AuthError> {
    let now = Utc::now().naive_utc();
    let rows: Vec<(NaiveDateTime,)> = sqlx::query_as(
        "SELECT login_time FROM work_sessions WHERE user_id = $1 AND logout_time IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
    for (login_time,) in rows {
        let duration = (now - login_time).num_milliseconds() as f64 / 3_600_000.0;
        let duration_rounded = (duration * 100.0).round() / 100.0;
        sqlx::query(
            "UPDATE work_sessions SET logout_time = $1, duration_hours = $2 WHERE user_id = $3 AND logout_time IS NULL",
        )
        .bind(now)
        .bind(duration_rounded)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
    }
    Ok(())
}

/// Створює нову робочу сесію (Python WorkSession(login_time=utcnow())).
async fn create_work_session(pool: &StorePool, user_id: Uuid) -> Result<(), AuthError> {
    sqlx::query(
        "INSERT INTO work_sessions (id, user_id, login_time, store_id, created_at)
         VALUES (uuid_generate_v4(), $1, $2,
                 COALESCE(NULLIF(current_setting('app.store_id', true), '')::uuid, NULL),
                 $2)",
    )
    .bind(user_id)
    .bind(Utc::now().naive_utc())
    .execute(pool)
    .await
    .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
    Ok(())
}

/// Логін: спільна логіка для password/pin.
async fn login_common(
    pool: &StorePool,
    login: &str,
    password: &str,
    pin: bool,
) -> Result<LoginResult, AuthError> {
    let row = sqlx::query_as::<_, (Uuid, String, String, String, bool, bool, Option<serde_json::Value>, Option<String>, NaiveDateTime, NaiveDateTime)>(
        "SELECT id, name, login, role::text, is_active, onboarding_completed, permissions, password_hash, created_at, updated_at FROM users WHERE login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    .map_err(|e| AuthError::Infrastructure(e.to_string()))?;

    let Some((
        id,
        name,
        login,
        role,
        is_active,
        onboarding_completed,
        permissions,
        password_hash,
        created_at,
        updated_at,
    )) = row
    else {
        return Err(AuthError::Unauthorized(if pin {
            "Невірний логін або PIN-код".to_string()
        } else {
            "Невірний логін або пароль".to_string()
        }));
    };

    let user = UserRow {
        id,
        name,
        login,
        role,
        is_active,
        onboarding_completed,
        permissions,
        created_at,
        updated_at,
    };

    if !user.is_active {
        return Err(AuthError::Forbidden("Користувач деактивований".to_string()));
    }

    if pin {
        // PIN: хеш може бути NULL → 401 "PIN-код не встановлений".
        let stored_pin =
            sqlx::query_scalar::<_, Option<String>>("SELECT pin_code FROM users WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        match stored_pin {
            None => {
                return Err(AuthError::Unauthorized(
                    "PIN-код не встановлений для цього користувача".to_string(),
                ))
            }
            Some(hash) => {
                let ok = bcrypt::verify(password, &hash).unwrap_or(false);
                if !ok {
                    return Err(AuthError::Unauthorized(
                        "Невірний логін або PIN-код".to_string(),
                    ));
                }
            }
        }
    } else {
        let ok = bcrypt::verify(password, password_hash.as_deref().unwrap_or("")).unwrap_or(false);
        if !ok {
            return Err(AuthError::Unauthorized(
                "Невірний логін або пароль".to_string(),
            ));
        }
    }

    // Робоча сесія: закриваємо попередні активні, створюємо нову.
    close_active_work_sessions(pool, id).await?;
    create_work_session(pool, id).await?;

    Ok(LoginResult {
        access_token: String::new(), // заповнює API-шар (має секрет)
        refresh_token: String::new(),
        token_type: "bearer".to_string(),
        user: user.to_dto(),
    })
}

#[async_trait::async_trait]
impl AuthService for SqlxAuth {
    async fn login(&self, input: &LoginRequest) -> Result<LoginResult, AuthError> {
        login_common(&self.pool, &input.login, &input.password, false).await
    }

    async fn login_pin(&self, input: &LoginPinRequest) -> Result<LoginResult, AuthError> {
        login_common(&self.pool, &input.login, &input.pin_code, true).await
    }

    async fn refresh(&self, user_id: Uuid) -> Result<LoginResult, AuthError> {
        // API-шар вже декодував refresh_token → user_id.
        let user = fetch_user(&self.pool, user_id)
            .await?
            .ok_or_else(|| AuthError::Unauthorized("Користувача не знайдено".to_string()))?;
        if !user.is_active {
            return Err(AuthError::Forbidden("Користувача деактивовано".to_string()));
        }
        Ok(LoginResult {
            access_token: String::new(),
            refresh_token: String::new(),
            token_type: "bearer".to_string(),
            user: user.to_dto(),
        })
    }

    async fn logout(&self, user_id: Uuid) -> Result<(), AuthError> {
        // Остання активна сесія (ORDER BY login_time DESC LIMIT 1).
        let now = Utc::now().naive_utc();
        let row = sqlx::query_as::<_, (NaiveDateTime,)>(
            "SELECT login_time FROM work_sessions WHERE user_id = $1 AND logout_time IS NULL ORDER BY login_time DESC LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        if let Some((login_time,)) = row {
            let duration = (now - login_time).num_milliseconds() as f64 / 3_600_000.0;
            let duration_rounded = (duration * 100.0).round() / 100.0;
            sqlx::query(
                "UPDATE work_sessions SET logout_time = $1, duration_hours = $2 WHERE user_id = $3 AND logout_time IS NULL",
            )
            .bind(now)
            .bind(duration_rounded)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        }
        Ok(())
    }

    async fn get_user_by_id(&self, user_id: Uuid) -> Result<UserDto, AuthError> {
        let user = fetch_user(&self.pool, user_id)
            .await?
            .ok_or_else(|| AuthError::Unauthorized("Користувача не знайдено".to_string()))?;
        if !user.is_active {
            return Err(AuthError::Forbidden("Користувач деактивований".to_string()));
        }
        Ok(user.to_dto())
    }

    async fn users_list_public(&self) -> Result<Vec<PublicUserDto>, AuthError> {
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, name, login FROM users WHERE is_active = true ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(id, name, login)| PublicUserDto { id, name, login })
            .collect())
    }

    async fn list_users(&self, page: i64, size: i64) -> Result<UserListDto, AuthError> {
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        let offset = (page - 1) * size;
        let rows = sqlx::query_as::<_, (Uuid, String, String, String, bool, bool, Option<serde_json::Value>, NaiveDateTime, NaiveDateTime)>(
            "SELECT id, name, login, role::text, is_active, onboarding_completed, permissions, created_at, updated_at FROM users ORDER BY name LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        let items: Vec<UserDto> = rows
            .into_iter()
            .map(|r| {
                UserRow {
                    id: r.0,
                    name: r.1,
                    login: r.2,
                    role: r.3,
                    is_active: r.4,
                    onboarding_completed: r.5,
                    permissions: r.6,
                    created_at: r.7,
                    updated_at: r.8,
                }
                .to_dto()
            })
            .collect();
        let pages = if total > 0 {
            (total + size - 1) / size
        } else {
            1
        };
        Ok(UserListDto {
            items,
            total,
            page,
            page_size: size,
            pages: pages.max(1),
        })
    }

    async fn create_user(&self, input: &UserCreateInput) -> Result<UserDto, AuthError> {
        // Логін: авто-генерація (транслітерація + суфікс) або перевірка 409.
        let login = match &input.login {
            Some(l) => {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE login = $1)")
                        .bind(l)
                        .fetch_one(&self.pool)
                        .await
                        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
                if exists {
                    return Err(AuthError::Conflict(format!(
                        "Користувач з логіном '{l}' вже існує"
                    )));
                }
                l.clone()
            }
            None => {
                let base = generate_login_from_name(&input.name);
                let mut candidate = base.clone();
                let mut counter = 1;
                loop {
                    let exists: bool =
                        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE login = $1)")
                            .bind(&candidate)
                            .fetch_one(&self.pool)
                            .await
                            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
                    if !exists {
                        break;
                    }
                    candidate = format!("{base}_{counter}");
                    counter += 1;
                }
                candidate
            }
        };

        let password_hash = bcrypt::hash(&input.password, 12)
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        let pin_hash = match &input.pin_code {
            Some(pin) => {
                Some(bcrypt::hash(pin, 12).map_err(|e| AuthError::Infrastructure(e.to_string()))?)
            }
            None => None,
        };
        let role_str = input.role.as_str();
        let permissions_json = input.permissions.as_ref().map(|p| serde_json::json!(p));

        let id = Uuid::new_v4();
        let now = Utc::now().naive_utc();
        sqlx::query(
            "INSERT INTO users (id, name, login, password_hash, pin_code, role, is_active, permissions, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6::user_role,$7,$8,$9,$10)",
        )
        .bind(id)
        .bind(&input.name)
        .bind(&login)
        .bind(&password_hash)
        .bind(&pin_hash)
        .bind(role_str)
        .bind(input.is_active)
        .bind(&permissions_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;

        Ok(UserDto {
            id,
            name: input.name.clone(),
            login,
            role: role_str.to_string(),
            is_active: input.is_active,
            onboarding_completed: true,
            permissions: input.permissions.clone(),
            created_at: now,
            updated_at: now,
        })
    }

    async fn update_user(
        &self,
        user_id: Uuid,
        input: &UserUpdateInput,
    ) -> Result<UserDto, AuthError> {
        let user = fetch_user(&self.pool, user_id).await?.ok_or_else(|| {
            AuthError::NotFound(format!("Користувача з ID '{user_id}' не знайдено"))
        })?;

        // Унікальність логіну (тільки якщо змінюється).
        if let Some(login) = &input.login {
            if login != &user.login {
                let exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE login = $1)")
                        .bind(login)
                        .fetch_one(&self.pool)
                        .await
                        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
                if exists {
                    return Err(AuthError::Conflict(format!(
                        "Користувач з логіном '{login}' вже існує"
                    )));
                }
            }
        }

        let name = input.name.clone().unwrap_or(user.name.clone());
        let login = input.login.clone().unwrap_or(user.login.clone());
        let role = input
            .role
            .map(|r| r.as_str().to_string())
            .unwrap_or(user.role.clone());
        let is_active = input.is_active.unwrap_or(user.is_active);
        let onboarding_completed = input
            .onboarding_completed
            .unwrap_or(user.onboarding_completed);

        // password: Some(непорожній) → hash; Some(порожній) → ігнорується (як Python).
        let mut password_hash = user_password_hash(&self.pool, user_id).await?;
        if let Some(p) = &input.password {
            if !p.is_empty() {
                password_hash = Some(
                    bcrypt::hash(p, 12).map_err(|e| AuthError::Infrastructure(e.to_string()))?,
                );
            }
        }

        // pin_code: Some(непорожній) → hash; Some(порожній) → "" (як Python setattr);
        // None → без змін.
        let pin_code = match &input.pin_code {
            Some(p) if !p.is_empty() => {
                Some(bcrypt::hash(p, 12).map_err(|e| AuthError::Infrastructure(e.to_string()))?)
            }
            Some(_) => Some(String::new()),
            None => user_pin_code(&self.pool, user_id).await?,
        };

        let permissions_json = match &input.permissions {
            Some(p) => Some(serde_json::json!(p)),
            None => user.permissions.clone(),
        };

        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE users SET name=$1, login=$2, password_hash=$3, pin_code=$4, role=$5::user_role, is_active=$6, onboarding_completed=$7, permissions=$8, updated_at=$9 WHERE id=$10",
        )
        .bind(name.clone())
        .bind(login.clone())
        .bind(password_hash)
        .bind(pin_code)
        .bind(role.clone())
        .bind(is_active)
        .bind(onboarding_completed)
        .bind(&permissions_json)
        .bind(now)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;

        Ok(UserDto {
            id: user_id,
            name,
            login,
            role,
            is_active,
            onboarding_completed,
            permissions: permissions_json.as_ref().and_then(|v| {
                v.as_array().map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
            }),
            created_at: user.created_at,
            updated_at: now,
        })
    }

    async fn update_permissions(
        &self,
        user_id: Uuid,
        permissions: &[String],
    ) -> Result<UserDto, AuthError> {
        let user = fetch_user(&self.pool, user_id).await?.ok_or_else(|| {
            AuthError::NotFound(format!("Користувача з ID '{user_id}' не знайдено"))
        })?;
        let json = serde_json::json!(permissions);
        let now = Utc::now().naive_utc();
        sqlx::query("UPDATE users SET permissions=$1, updated_at=$2 WHERE id=$3")
            .bind(&json)
            .bind(now)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        Ok(UserDto {
            id: user_id,
            name: user.name,
            login: user.login,
            role: user.role,
            is_active: user.is_active,
            onboarding_completed: user.onboarding_completed,
            permissions: Some(permissions.to_vec()),
            created_at: user.created_at,
            updated_at: now,
        })
    }

    async fn update_hourly_rate(
        &self,
        user_id: Uuid,
        hourly_rate: f64,
    ) -> Result<serde_json::Value, AuthError> {
        let user = fetch_user(&self.pool, user_id).await?.ok_or_else(|| {
            AuthError::NotFound(format!("Користувача з ID '{user_id}' не знайдено"))
        })?;
        let now = Utc::now().naive_utc();
        sqlx::query("UPDATE users SET hourly_rate=$1, updated_at=$2 WHERE id=$3")
            .bind(hourly_rate)
            .bind(now)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        Ok(serde_json::json!({
            "id": user_id.to_string(),
            "name": user.name,
            "hourly_rate": hourly_rate,
            "message": "Погодинну ставку оновлено",
        }))
    }

    async fn delete_user(&self, user_id: Uuid, current_user_id: Uuid) -> Result<(), AuthError> {
        let user = fetch_user(&self.pool, user_id).await?.ok_or_else(|| {
            AuthError::NotFound(format!("Користувача з ID '{user_id}' не знайдено"))
        })?;
        // Пов'язані чеки → 409.
        let has_receipts: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM receipts WHERE cashier_id = $1 LIMIT 1)",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        if has_receipts {
            return Err(AuthError::Conflict(format!(
                "Неможливо видалити користувача '{}' — він має пов'язані чеки продажу. Деактивуйте користувача замість видалення.",
                user.name
            )));
        }
        // Самого себе → 409.
        if user_id == current_user_id {
            return Err(AuthError::Conflict(
                "Неможливо видалити самого себе".to_string(),
            ));
        }
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        Ok(())
    }

    // ─── Settings ──────────────────────────────────────────────────────────

    async fn settings_all(&self) -> Result<SettingsModulesDto, AuthError> {
        let rows = fetch_settings(&self.pool, None).await?;
        Ok(build_modules(rows))
    }

    async fn settings_by_module(&self, module: &str) -> Result<Vec<SettingDto>, AuthError> {
        let rows = fetch_settings(&self.pool, Some(module)).await?;
        if rows.is_empty() {
            return Err(AuthError::NotFound(format!(
                "Модуль '{module}' не знайдено або він порожній"
            )));
        }
        Ok(rows)
    }

    async fn settings_batch_update(
        &self,
        settings: &[(String, Option<String>)],
    ) -> Result<SettingsModulesDto, AuthError> {
        let mut updated: Vec<String> = Vec::new();
        for (key, value) in settings {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM system_settings WHERE key = $1 AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid)")
                    .bind(key)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
            if exists {
                // updated_at = now() — Python: SQLAlchemy onupdate.
                sqlx::query(
                    "UPDATE system_settings SET value = $1, updated_at = now() WHERE key = $2 AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid",
                )
                .bind(value)
                .bind(key)
                .execute(&self.pool)
                .await
                .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
                updated.push(key.clone());
            }
        }
        let _ = updated;
        let rows = fetch_settings(&self.pool, None).await?;
        Ok(build_modules(rows))
    }

    async fn settings_update_key(
        &self,
        key: &str,
        value: Option<String>,
    ) -> Result<SettingDto, AuthError> {
        use torgashka_domain::{determine_module, determine_value_type, humanize_key};
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM system_settings WHERE key = $1 AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid)")
                .bind(key)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AuthError::Infrastructure(e.to_string()))?;

        if exists {
            sqlx::query("UPDATE system_settings SET value = $1, updated_at = now() WHERE key = $2 AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid")
                .bind(&value)
                .bind(key)
                .execute(&self.pool)
                .await
                .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        } else {
            let id = Uuid::new_v4();
            let module = determine_module(key);
            let value_type = determine_value_type(value.as_deref());
            let label = humanize_key(key);
            sqlx::query(
                "INSERT INTO system_settings (id, module, key, value, value_type, label, description, options, is_active, store_id, created_at, updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,NULL,NULL,true,
                         COALESCE(NULLIF(current_setting('app.store_id', true), '')::uuid, NULL), now(), now())",
            )
            .bind(id)
            .bind(&module)
            .bind(key)
            .bind(&value)
            .bind(&value_type)
            .bind(&label)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        }
        let row = sqlx::query_as::<_, (Uuid, String, String, Option<String>, String, String, Option<String>, Option<String>, bool, DateTime<Utc>, DateTime<Utc>)>(
            "SELECT id, module, key, value, value_type, label, description, options, is_active, created_at, updated_at FROM system_settings WHERE key = $1 AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid",
        )
        .bind(key)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))?;
        Ok(SettingDto {
            id: row.0,
            module: row.1,
            key: row.2,
            value: row.3,
            value_type: row.4,
            label: row.5,
            description: row.6,
            options: row.7,
            is_active: row.8,
            created_at: row.9,
            updated_at: row.10,
        })
    }
}

// ─── Допоміжні функції ──────────────────────────────────────────────────────

async fn user_password_hash(pool: &StorePool, user_id: Uuid) -> Result<Option<String>, AuthError> {
    sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))
}

async fn user_pin_code(pool: &StorePool, user_id: Uuid) -> Result<Option<String>, AuthError> {
    sqlx::query_scalar("SELECT pin_code FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AuthError::Infrastructure(e.to_string()))
}

type SettingRow = (
    Uuid,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    bool,
    DateTime<Utc>,
    DateTime<Utc>,
);

async fn fetch_settings(
    pool: &StorePool,
    module: Option<&str>,
) -> Result<Vec<SettingDto>, AuthError> {
    let rows: Vec<SettingRow> = match module {
        Some(m) => {
            sqlx::query_as(
                "SELECT id, module, key, value, value_type, label, description, options, is_active, created_at, updated_at FROM system_settings WHERE module = $1 AND is_active = true AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid ORDER BY key",
            )
            .bind(m)
            .fetch_all(pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?
        }
        None => {
            sqlx::query_as(
                "SELECT id, module, key, value, value_type, label, description, options, is_active, created_at, updated_at FROM system_settings WHERE is_active = true AND store_id = NULLIF(current_setting('app.store_id', true), '')::uuid ORDER BY module, key",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| AuthError::Infrastructure(e.to_string()))?
        }
    };
    Ok(rows
        .into_iter()
        .map(|r| SettingDto {
            id: r.0,
            module: r.1,
            key: r.2,
            value: r.3,
            value_type: r.4,
            label: r.5,
            description: r.6,
            options: r.7,
            is_active: r.8,
            created_at: r.9,
            updated_at: r.10,
        })
        .collect())
}

/// Будує {modules: {module: [SettingDto...]}} (Python SettingsModuleResponse).
fn build_modules(rows: Vec<SettingDto>) -> SettingsModulesDto {
    let mut modules: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for s in rows {
        let entry = modules
            .entry(s.module.clone())
            .or_insert_with(|| serde_json::json!([]));
        if let Some(arr) = entry.as_array_mut() {
            arr.push(serde_json::to_value(&s).unwrap_or(serde_json::Value::Null));
        }
    }
    SettingsModulesDto { modules }
}
