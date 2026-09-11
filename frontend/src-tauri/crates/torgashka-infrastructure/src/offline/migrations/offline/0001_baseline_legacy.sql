-- 0001_baseline_legacy.sql
-- Базова схема каси: products, receipts, settings (як у offline/db.rs) + індекси.
-- Для нових інсталяцій — створює таблиці; для старих БД — no-op (CREATE IF NOT EXISTS).
--
-- УВАГА: `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` НЕ використовується:
-- локальна SQLite-збірка (перевірено 3.45.1/3.46.0) відхиляє цей синтаксис
-- ("near EXISTS: syntax error"). store_id для legacy-БД (створених до Етапу 5)
-- додає двигун міграцій Rust-кроком (PRAGMA table_info → ALTER) у межах
-- транзакції цієї міграції — див. migrations.rs::legacy_add_store_id.

CREATE TABLE IF NOT EXISTS products (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    store_id TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    data TEXT NOT NULL,
    store_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_receipts_synced ON receipts(synced);
CREATE INDEX IF NOT EXISTS idx_products_name ON products(id);
CREATE INDEX IF NOT EXISTS idx_receipts_store ON receipts(store_id);
CREATE INDEX IF NOT EXISTS idx_products_store ON products(store_id);
