-- 0003_master_tables.sql
-- Нормалізовані копії довідників сервера (sync-schema-design.md, розділи 7.2/8.1).
--
-- Ціль: pull-клієнт (ЕТАП 3) пише майстер-дані в ЦІ таблиці; кожен рядок —
-- копія рядка сервера з версією дельти (server_version), з якою прийшов.
-- Існуючий JSON-кеш `products` (0001) НЕ чіпається: каталог pull живе в
-- `products_v2` (розділ 7.2: «products_v2 замість JSON-кешу»).
--
-- Правила:
--   * is_deleted INTEGER — soft-delete: op=delete з сервера позначає рядок
--     (фізичне видалення не використовується, дизайн 1.4).
--   * server_version — версія дельти, з якою рядок прийшов (для діагностики).
--   * data TEXT — повний JSON payload рядка (зворотна сумісність з клієнтами,
--     що читають data-кеш; розділ 8.1 products.data).

-- ── Категорії (FK для products_v2) ────────────────────────────────────────
CREATE TABLE IF NOT EXISTS categories (
    id             TEXT PRIMARY KEY,          -- серверний uuid
    name           TEXT NOT NULL,
    parent_id      TEXT,
    is_deleted     INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL DEFAULT 0,
    data           TEXT
);
CREATE INDEX IF NOT EXISTS idx_categories_parent ON categories(parent_id);

-- ── Постачальники ─────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS suppliers (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    phone          TEXT,
    is_deleted     INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL DEFAULT 0,
    data           TEXT
);

-- ── Працівники (employees = серверні users) ───────────────────────────────
CREATE TABLE IF NOT EXISTS employees (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    pin_hash       TEXT,                       -- хеш PIN (bcrypt) для PIN-логіну касира
    role           TEXT,
    is_deleted     INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL DEFAULT 0,
    data           TEXT
);

-- ── Норми залишків (min/max; НЕ кількість) ────────────────────────────────
CREATE TABLE IF NOT EXISTS stock_norms (
    product_id     TEXT PRIMARY KEY,
    min_qty        NUMERIC,
    max_qty        NUMERIC,
    is_deleted     INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL DEFAULT 0,
    data           TEXT
);

-- ── Каталог (нормалізована копія серверних products) ─────────────────────
-- Явні колонки продажу + data JSON (повний payload сервера).
CREATE TABLE IF NOT EXISTS products_v2 (
    id             TEXT PRIMARY KEY,
    barcode        TEXT,
    name           TEXT NOT NULL,
    unit           TEXT,
    category_id    TEXT,
    price          NUMERIC,
    is_deleted     INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL DEFAULT 0,
    data           TEXT
);
CREATE INDEX IF NOT EXISTS idx_products_v2_barcode ON products_v2(barcode);
CREATE INDEX IF NOT EXISTS idx_products_v2_category ON products_v2(category_id);
