//! Спільний хелпер integration-тестів `torgashka-infrastructure`.
//!
//! [`ensure_fixture`] ідемпотентно відновлює seed-фікстуру ТЕСТОВОЇ БД
//! (`users`, `stores`, `user_stores`, `system_settings`, `print_templates`) —
//! ті самі UUID і значення, що в хвості `scripts/schema.sql`
//! («Тестовий seed для Rust integration-тестів»).
//!
//! Навіщо: e2e адмін-етапів у `torgashka-api` роблять `TRUNCATE` спільних
//! таблиць (гігієна «чистого старту»), а всі e2e працюють на ОДНІЙ тестовій
//! БД. Тест, який залежить від фікстури (точка-донор «Білий магазин» +
//! власник), не має покладатись на порядок виконання бінарників — він
//! забезпечує фікстуру сам (QA §5.2 — гігієна тестів).
//!
//! Жодних асертів це не змінює: тести, як і раніше, перевіряють свою
//! бізнес-логіку; фікстура — лише передумова, яку вони реконструюють.

use sqlx::PgPool;

/// UUID точки-донора («Білий магазин»), власника та 3 точок сітки.
const FIXTURE_SQL: &[&str] = &[
    // Власник (ФОП Мельничук) — контекст запиту для StoreCtx.
    "INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
     VALUES (
       'e30d480c-ef3b-4d0e-8808-0c745196d3d8', 'ФОП Мельничук', 'igor2104@i.ua',
       '$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e',
       'owner'::public.user_role, true, now(), now(), true
     )
     ON CONFLICT (id) DO NOTHING",
    // Точки сітки (фіксовані UUID — контекст запиту тестів).
    "INSERT INTO stores (id, name) VALUES
       ('65d5db51-672f-4a38-9c1e-f36c5feb5374', 'Білий магазин'),
       ('5e840d11-6b9b-4f6f-a6e4-000d1bb0a307', 'Жовтий магазин'),
       ('d9be9608-c011-49be-b776-3317ca5e9af6', 'Тест Магазин C')
     ON CONFLICT (id) DO NOTHING",
    // Власник ↔ точки (без ON CONFLICT — захист від дублів через NOT EXISTS).
    "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
     SELECT 'e30d480c-ef3b-4d0e-8808-0c745196d3d8', s.id, 'owner', '{}'::jsonb, true, now()
     FROM stores s
     WHERE s.id IN (
       '65d5db51-672f-4a38-9c1e-f36c5feb5374',
       '5e840d11-6b9b-4f6f-a6e4-000d1bb0a307',
       'd9be9608-c011-49be-b776-3317ca5e9af6'
     )
     AND NOT EXISTS (
       SELECT 1 FROM user_stores us
       WHERE us.user_id = 'e30d480c-ef3b-4d0e-8808-0c745196d3d8' AND us.store_id = s.id
     )",
    // Налаштування точки-донора (>0 — передумова store_settings_isolation).
    "INSERT INTO system_settings (id, module, key, value, value_type, label, description, options, is_active, created_at, updated_at, store_id) VALUES
       ('a0000000-0000-4000-8000-000000000001', 'general', 'company_name', 'ФОП Мельничук', 'string', 'Назва магазину', 'Виводиться в чеках', NULL, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374'),
       ('a0000000-0000-4000-8000-000000000002', 'pos', 'allow_negative_stock', 'false', 'boolean', 'Торгівля в мінус', 'Дозволити продаж в мінус', NULL, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374'),
       ('a0000000-0000-4000-8000-000000000003', 'printing', 'default_template', 'receipt_80mm', 'string', 'Шаблон за замовчуванням', 'Для чеків', NULL, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374')
     ON CONFLICT (id) DO NOTHING",
    // Шаблон друку точки-донора (>0 — передумова store_settings_isolation).
    "INSERT INTO print_templates (id, name, type, content, variables, is_default, is_active, created_at, updated_at, store_id) VALUES
       ('b0000000-0000-4000-8000-000000000001', 'receipt_80mm (test)', 'receipt', '<html><body>{{items}}</body></html>', '[{\"key\": \"shop_name\", \"label\": \"Магазин\", \"default\": \"Мій\"}]', false, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374')
     ON CONFLICT (id) DO NOTHING",
];

/// Ідемпотентно відновлює seed-фікстуру тестової БД.
///
/// Викликати НА ПОЧАТКУ тесту, після [`torgashka_infrastructure::db::connect_test_pool`],
/// якщо тест залежить від точки-донора/власника (а не створює їх сам).
pub async fn ensure_fixture(pool: &PgPool) {
    for stmt in FIXTURE_SQL {
        sqlx::query(stmt)
            .execute(pool)
            .await
            .expect("seed-фікстура тестової БД застосовується");
    }
}
