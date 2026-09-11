-- ============================================================================
-- МІГРАЦІЯ: Перенесення налаштувань з pos_system у pos_system_fresh
-- Дата: 2026-08-21
-- ----------------------------------------------------------------------------
-- Копіює ВСІ рядки:
--   pos_system.system_settings   → pos_system_fresh.system_settings   (30 рядків)
--   pos_system.print_templates   → pos_system_fresh.print_templates   (9 рядків)
--
-- ЗІ ЗБЕРЕЖЕННЯМ id (primary key) — print_templates.id потрібен як ціль
-- для system_settings.label_template_id / price_tag_template_id.
--
-- Техніка: PostgreSQL не підтримує cross-database запити напряму, тому
-- використовується розширення dblink. Параметри підключення до джерела
-- передаються psql-змінною src_conn:
--   psql -v src_conn="host=localhost port=5432 dbname=pos_system user=postgres password=***"
--
-- ВАЖЛИВА АНОМАЛІЯ (задокументовано):
--   Усі 30 рядків system_settings мають store_id = d9be9608-c011-49be-b776-3317ca5e9af6
--   ("Основна точка"), але цього store НЕМАЄ у pos_system_fresh
--   (там існує лише 65d5db51-672f-4a38-9c1e-f36c5feb5374 "Білий магазин").
--   FK fk_system_settings_store (NOT DEFERRABLE) заблокував би вставку.
--   Змінювати stores/user_stores заборонено контрактом.
--   Тому store_id копіюється як NULL (стовпець nullable). Застосунок читає
--   налаштування БЕЗ фільтру по store_id (SettingsRepository.get_all),
--   а підключення до БД — postgres (superuser), тож RLS не застосовується.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS dblink;

BEGIN;

-- ----------------------------------------------------------------------------
-- 1. print_templates: копіюємо ПЕРШИМИ — щоб system_settings.label_template_id /
--    price_tag_template_id посилалися на наявні id.
-- ----------------------------------------------------------------------------
INSERT INTO pos_system_fresh.public.print_templates
    (id, name, type, content, variables, is_default, is_active, created_at, updated_at)
SELECT
    t.id, t.name, t.type, t.content, t.variables, t.is_default, t.is_active,
    t.created_at, t.updated_at
FROM dblink(
        :'src_conn',
        'SELECT id, name, type, content, variables, is_default, is_active,
                created_at, updated_at
           FROM public.print_templates'
     ) AS t(
        id uuid, name varchar(255), type varchar(20), content text,
        variables jsonb, is_default boolean, is_active boolean,
        created_at timestamptz, updated_at timestamptz
     )
ON CONFLICT DO NOTHING;

-- ----------------------------------------------------------------------------
-- 2. system_settings: копіюємо всі рядки, store_id → NULL (див. коментар вище).
-- ----------------------------------------------------------------------------
INSERT INTO pos_system_fresh.public.system_settings
    (id, module, key, value, value_type, label, description, options,
     is_active, created_at, updated_at, store_id)
SELECT
    s.id, s.module, s.key, s.value, s.value_type, s.label, s.description,
    s.options, s.is_active, s.created_at, s.updated_at,
    NULL::uuid AS store_id  -- старого store немає у fresh; stores чіпати заборонено
FROM dblink(
        :'src_conn',
        'SELECT id, module, key, value, value_type, label, description, options,
                is_active, created_at, updated_at
           FROM public.system_settings'
     ) AS s(
        id uuid, module varchar(50), key varchar(100), value text,
        value_type varchar(20), label varchar(255), description text,
        options text, is_active boolean, created_at timestamptz,
        updated_at timestamptz
     )
ON CONFLICT DO NOTHING;

-- ----------------------------------------------------------------------------
-- 3. Контрольні запити (результат видно у логу виконання)
-- ----------------------------------------------------------------------------
SELECT 'system_settings_fresh' AS check_name, count(*) AS cnt FROM pos_system_fresh.public.system_settings
UNION ALL
SELECT 'print_templates_fresh', count(*) FROM pos_system_fresh.public.print_templates;

COMMIT;
