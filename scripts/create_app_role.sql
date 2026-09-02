-- ============================================================================
-- create_app_role.sql — роль додатка torgashka_app (ідемпотентний)
-- ============================================================================
-- Призначення: роль, під якою працює Rust-фасад (torgashka-api) для ВСІХ
-- бізнес-запитів. RLS (міграції 0004/0005) блокує доступ поза контекстом
-- store_id/user_id — ця роль НЕ є власником таблиць, тому RLS для неї
-- застосовується завжди (у поєднанні з FORCE RLS — і для власника).
--
-- Безпека (вимога плану, розділ 3.1):
--   - БЕЗ SUPERUSER / BYPASSRLS / CREATEDB / CREATEROLE;
--   - пароль НЕ хардкодиться: передається через psql-змінну
--     APP_DB_PASSWORD (-v APP_DB_PASSWORD=...) або з оточення
--     (APP_DB_PASSWORD=... psql -f scripts/create_app_role.sql),
--     інакше ставиться заборонений плейсхолдер (змінити до продакшну);
--   - при повторному запуску роль не перестворюється, лише оновлюється
--     пароль та атрибути (ідемпотентність).
--
-- Використання:
--   APP_DB_PASSWORD='<пароль>' psql -h localhost -U postgres -d postgres \
--       -v APP_DB_PASSWORD='<пароль>' -f scripts/create_app_role.sql
--   (скрипт сам перемикається на pos_system і torgashka_template через \connect)
--
-- Права на torgashka_owner_* надаються раннером міграцій (ЕТАП 9.3,
-- scripts/migrate_all_owners.sh) — тут достатньо шаблону, бо owner-БД
-- створюються КОПІЮВАННЯМ torgashka_template (CREATE DATABASE ... TEMPLATE).
-- ============================================================================

\set ON_ERROR_STOP on

-- ── 0. Пароль: зі змінної psql / оточення / плейсхолдер ──────────────
\if :{?APP_DB_PASSWORD}
\else
  \set APP_DB_PASSWORD 'CHANGE_ME_INSTALL_PASSWORD'
\endif

\echo '>> create_app_role.sql: створення/оновлення ролі torgashka_app'

-- ── 1. Роль: створення (ідемпотентно) + гарантія атрибутів ────────────
-- CREATE ROLE виконується лише якщо ролі ще немає (через \gexec).
SELECT format(
    'CREATE ROLE torgashka_app LOGIN PASSWORD %L NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE',
    :'APP_DB_PASSWORD'
)
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'torgashka_app')\gexec

-- ALTER — ідемпотентно оновлює пароль і «скидає» небезпечні атрибути,
-- якщо роль була створена раніше з іншими параметрами.
ALTER ROLE torgashka_app WITH LOGIN PASSWORD :'APP_DB_PASSWORD'
    NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;

-- ── 2. CONNECT на цільові БД кластера ──────────────────────────────────
GRANT CONNECT ON DATABASE pos_system TO torgashka_app;
GRANT CONNECT ON DATABASE torgashka_template TO torgashka_app;

-- ── 3. Права в pos_system (мета-БД) ────────────────────────────────────
\connect pos_system
GRANT USAGE ON SCHEMA public TO torgashka_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO torgashka_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO torgashka_app;
-- майбутні таблиці (міграції) — автоматично доступні ролі
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO torgashka_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO torgashka_app;

-- ── 4. Права в torgashka_template (шаблон owner-БД) ────────────────────
\connect torgashka_template
GRANT USAGE ON SCHEMA public TO torgashka_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO torgashka_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO torgashka_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO torgashka_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO torgashka_app;

\echo '>> Готово. Перевірка: psql -U torgashka_app -d pos_system -c "SELECT current_user"'
