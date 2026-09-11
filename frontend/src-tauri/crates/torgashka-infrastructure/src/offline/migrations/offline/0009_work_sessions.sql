-- 0009_work_sessions.sql
-- ADR-0007 §3.4 (#38–#40): робочі сесії користувача на standby пишуться в
-- SQLite вузла (клас LOCAL_SQLITE). Реплікація — outbox'ом (тип `work_session`).
-- Схема дзеркалить PG public.work_sessions (login_time/logout_time/
-- duration_hours/store_id) + client_uuid — ідемпотентний ключ push
-- (аналог Alembic 0013_sync_push_idempotency для PG).
-- Патерн агрегатів 0006 (data JSON + client_uuid + synced).
CREATE TABLE IF NOT EXISTS work_sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid     TEXT NOT NULL UNIQUE,   -- UUIDv4 сесії; ключ push-ідемпотентності
    user_id         TEXT NOT NULL,
    store_id        TEXT,
    login_time      TEXT NOT NULL,
    logout_time     TEXT,
    duration_hours  REAL,
    data            TEXT NOT NULL,          -- JSON-envelope для push (дизайн 2.2)
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    synced          INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_work_sessions_open   ON work_sessions(user_id, logout_time);
CREATE INDEX IF NOT EXISTS idx_work_sessions_synced ON work_sessions(synced);
