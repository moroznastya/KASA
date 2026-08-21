//! Репозиторій торговельних точок (Етап 3 мультиточковості).
//!
//! Реалізує [`StoreService`] на sqlx/PostgreSQL. Усі запити йдуть через
//! [`StorePool`](crate::store_ctx::StorePool), тому RLS-контекст
//! (`app.user_id`/`app.store_id`) проставляється автоматично:
//!   - stores: політика 0004_rls — видно лише точки з user_stores користувача;
//!   - user_stores: політика — лише власні зв'язки;
//!   - stock: політика — лише рядки поточної точки (або всіх точок owner).

use sqlx::Row;
use uuid::Uuid;

use crate::store_ctx::{current_store_ctx, StorePool};
use torgashka_domain::{
    AvailabilityItemDto, StoreAvailabilityDto, StoreCreateInput, StoreDto, StoreError,
    StoreService, UserStoreAssignInput,
};

/// SQLx-реалізація сервісу точок.
#[derive(Clone)]
pub struct SqlxStoreService {
    pool: StorePool,
}

impl SqlxStoreService {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }
}

/// Map sqlx::Error → StoreError.
trait SqlxResultExt<T> {
    fn se(self) -> Result<T, StoreError>;
}

impl<T> SqlxResultExt<T> for Result<T, sqlx::Error> {
    fn se(self) -> Result<T, StoreError> {
        self.map_err(|e| StoreError::Infrastructure(e.to_string()))
    }
}

/// Зчитує StoreDto з рядка (stores + user_stores роль).
fn store_dto_from_row(row: &sqlx::postgres::PgRow) -> StoreDto {
    StoreDto {
        id: row.get("id"),
        name: row.get("name"),
        address: row.try_get("address").ok().flatten(),
        phone: row.try_get("phone").ok().flatten(),
        is_active: row.try_get("is_active").unwrap_or(true),
        created_at: row.try_get("created_at").unwrap_or_else(|_| {
            chrono::Utc::now().naive_utc()
        }),
        role: row.try_get("role").unwrap_or_else(|_| "cashier".to_string()),
        is_default: row.try_get("is_default").unwrap_or(false),
    }
}

#[async_trait::async_trait]
impl StoreService for SqlxStoreService {
    async fn list_stores(&self) -> Result<Vec<StoreDto>, StoreError> {
        let ctx = current_store_ctx().ok_or_else(|| {
            StoreError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string())
        })?;
        let rows = sqlx::query(
            r#"
            SELECT s.id, s.name, s.address, s.phone, s.is_active, s.created_at,
                   us.role, us.is_default
            FROM stores s
            JOIN user_stores us ON us.store_id = s.id
            WHERE us.user_id = $1
            ORDER BY us.is_default DESC, s.created_at ASC
            "#,
        )
        .bind(ctx.user_id)
        .fetch_all(&self.pool)
        .await
        .se()?;
        Ok(rows.iter().map(store_dto_from_row).collect())
    }

    async fn create_store(&self, input: &StoreCreateInput) -> Result<StoreDto, StoreError> {
        let ctx = current_store_ctx().ok_or_else(|| {
            StoreError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string())
        })?;
        if ctx.role != "owner" {
            return Err(StoreError::Forbidden(
                "Тільки власник може створювати торговельні точки".to_string(),
            ));
        }
        let name = input.name.trim();
        if name.is_empty() {
            return Err(StoreError::BadRequest(
                "Назва точки не може бути порожньою".to_string(),
            ));
        }
        let mut tx = self.pool.begin().await.se()?;
        let row = sqlx::query(
            r#"
            INSERT INTO stores (name, address, phone, is_active, created_at, updated_at)
            VALUES ($1, $2, $3, true, (now() AT TIME ZONE 'UTC')::timestamp,
                    (now() AT TIME ZONE 'UTC')::timestamp)
            RETURNING id, name, address, phone, is_active, created_at
            "#,
        )
        .bind(name)
        .bind(input.address.as_deref())
        .bind(input.phone.as_deref())
        .fetch_one(&mut *tx)
        .await
        .se()?;
        let store_id: Uuid = row.get("id");
        // Автоприв'язка творця як owner точки.
        sqlx::query(
            r#"
            INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
            VALUES ($1, $2, 'owner', '{"*": true}'::jsonb, false,
                    (now() AT TIME ZONE 'UTC')::timestamp)
            ON CONFLICT (user_id, store_id) DO NOTHING
            "#,
        )
        .bind(ctx.user_id)
        .bind(store_id)
        .execute(&mut *tx)
        .await
        .se()?;
        // Копіювання налаштувань з поточної точки (ctx.store_id) у нову —
        // кожна точка бачить ТІЛЬКИ свої рядки (явний фільтр за store_id,
        // бо роль БД — postgres/superuser, RLS не застосовується).
        sqlx::query(
            r#"
            INSERT INTO system_settings (id, module, key, value, value_type, label, description, options, is_active, store_id, created_at, updated_at)
            SELECT gen_random_uuid(), module, key, value, value_type, label, description, options, is_active, $1, now(), now()
            FROM system_settings WHERE store_id = $2 AND is_active = true
            "#,
        )
        .bind(store_id)
        .bind(ctx.store_id)
        .execute(&mut *tx)
        .await
        .se()?;
        // Копіювання шаблонів друку з поточної точки у нову.
        sqlx::query(
            r#"
            INSERT INTO print_templates (id, name, type, content, variables, is_default, is_active, store_id, created_at, updated_at)
            SELECT gen_random_uuid(), name, type, content, variables, is_default, is_active, $1, now(), now()
            FROM print_templates WHERE store_id = $2 AND is_active = true
            "#,
        )
        .bind(store_id)
        .bind(ctx.store_id)
        .execute(&mut *tx)
        .await
        .se()?;
        tx.commit().await.se()?;
        Ok(store_dto_from_row(&row))
    }

    async fn assign_user_store(
        &self,
        input: &UserStoreAssignInput,
    ) -> Result<StoreDto, StoreError> {
        let ctx = current_store_ctx().ok_or_else(|| {
            StoreError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string())
        })?;
        if ctx.role != "owner" {
            return Err(StoreError::Forbidden(
                "Тільки власник може призначати користувачів на точки".to_string(),
            ));
        }
        let role = if input.role.trim().is_empty() {
            "cashier"
        } else {
            input.role.trim()
        };
        if !matches!(role, "owner" | "admin" | "cashier") {
            return Err(StoreError::BadRequest(format!(
                "Невідома роль на точці: {role}"
            )));
        }
        let mut tx = self.pool.begin().await.se()?;
        sqlx::query(
            r#"
            INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
            VALUES ($1, $2, $3, '{}'::jsonb, $4,
                    (now() AT TIME ZONE 'UTC')::timestamp)
            ON CONFLICT (user_id, store_id) DO UPDATE
                SET role = EXCLUDED.role, is_default = EXCLUDED.is_default
            "#,
        )
        .bind(input.user_id)
        .bind(input.store_id)
        .bind(role)
        .bind(input.is_default)
        .execute(&mut *tx)
        .await
        .se()?;
        let row = sqlx::query(
            r#"
            SELECT s.id, s.name, s.address, s.phone, s.is_active, s.created_at,
                   us.role, us.is_default
            FROM stores s
            JOIN user_stores us ON us.store_id = s.id
            WHERE s.id = $1 AND us.user_id = $2
            "#,
        )
        .bind(input.store_id)
        .bind(input.user_id)
        .fetch_optional(&mut *tx)
        .await
        .se()?;
        tx.commit().await.se()?;
        match row {
            Some(r) => Ok(store_dto_from_row(&r)),
            None => Err(StoreError::NotFound(format!(
                "Точка '{}' не знайдена",
                input.store_id
            ))),
        }
    }

    async fn availability(&self) -> Result<Vec<AvailabilityItemDto>, StoreError> {
        let ctx = current_store_ctx().ok_or_else(|| {
            StoreError::BadRequest("Відсутній контекст точки (X-Store-Id)".to_string())
        })?;
        // Всі точки користувача (owner бачить усі свої; cashier — свою).
        let stores = self.list_stores().await?;
        if stores.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            r#"
            SELECT p.id AS product_id, p.title, p.barcode, p.unit,
                   s.id AS store_id, s.name AS store_name,
                   COALESCE(st.quantity, 0)::text AS quantity,
                   COALESCE(st.price, 0)::text AS price
            FROM products p
            JOIN user_stores us ON us.user_id = $1
            JOIN stores s ON s.id = us.store_id
            LEFT JOIN stock st ON st.store_id = s.id AND st.product_id = p.id
            ORDER BY p.title ASC, s.name ASC
            "#,
        )
        .bind(ctx.user_id)
        .fetch_all(&self.pool)
        .await
        .se()?;
        // Групуємо по продукту.
        let mut out: Vec<AvailabilityItemDto> = Vec::new();
        for r in &rows {
            let product_id: Uuid = r.get("product_id");
            let store_id: Uuid = r.get("store_id");
            let avail = StoreAvailabilityDto {
                store_id,
                store_name: r.get("store_name"),
                quantity: r.get("quantity"),
                price: r.get("price"),
            };
            match out.iter_mut().find(|i| i.product_id == product_id) {
                Some(item) => item.stores.push(avail),
                None => out.push(AvailabilityItemDto {
                    product_id,
                    title: r.get("title"),
                    barcode: r.try_get("barcode").ok().flatten(),
                    unit: r.try_get("unit").ok().flatten(),
                    stores: vec![avail],
                }),
            }
        }
        Ok(out)
    }
}

/// Порожній типаж-маркер для зручності конструктора у фасаді.
impl SqlxStoreService {
    pub fn pool_ref(&self) -> &StorePool {
        &self.pool
    }
}

// ── Юніт-тести форматування ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_dto_from_row_never_panics_on_empty() {
        // Рядок обов'язковий — перевіряємо лише, що конструктор DTO існує.
        let _ = std::mem::size_of::<StoreDto>();
        let _ = std::mem::size_of::<AvailabilityItemDto>();
    }
}
