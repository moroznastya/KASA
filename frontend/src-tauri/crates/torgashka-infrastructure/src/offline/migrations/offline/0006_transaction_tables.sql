-- 0006_transaction_tables.sql
-- ЕТАП 6 (offline-first): повні таблиці транзакцій каси
-- (sync-schema-design.md, розділ 8.1).
--
-- Узгодження з фактичним push-потоком (sync_push.rs, ЕТАП 4):
--   * ПРОДАЖ і ПОВЕРНЕННЯ йдуть через receipts (data = JSON чека,
--     receipt_type sale|return) + outbox (типи receipt/return_receipt) —
--     це РОБОЧИЙ шлях, який серверний Rust-фасад приймає (e2e green).
--     Повернення НЕ отримує окремої таблиці return_receipts: вона дублювала
--     б receipts(outbox return_receipt) другим шляхом запису тих самих даних
--     → роз'їжджання станів. (Розбіжність з текстом дизайну 8.1 зафіксовано
--     у звіті ЕТАП 6; слідуємо робочому коду — канал аномалій.)
--   * receipt_items — нормалізована ДЕТАЛІЗАЦІЯ чеків (sale/return):
--     пишеться enqueue_receipt у тій самій транзакції, що receipts+outbox
--     (атомарно, дизайн 4.4). Забезпечує локальний перегляд складу чека
--     без парсингу receipts.data.
--   * purchase_orders / inventories / transfers / write_offs — локальні
--     агрегати НОВИХ операцій каси (самодостатність: працюють з вимкненим
--     сервером). Схема = receipts-патерн: data JSON (payload як його формує
--     фронт для /v2-ендпоінтів сервера) + client_uuid (ідемпотентність
--     майбутнього push). Серверний Rust-фасад /sync/push СЬОГОДНІ приймає
--     лише sale/return → ці агрегати НЕ кладуться в outbox (push дав би
--     гарантований failed); синхронізація — ЕТАП 7 (розширення фасаду),
--     тоді ж вони отримають outbox-записи. Статус: synced = 0.
--   * debtors_ledger / cash_ledger — таблиці за дизайном 8.1; локальні
--     операції боргів/касових рухів — пізніші етапи (фронт сьогодні робить
--     їх онлайн). Створюються зараз, щоб схема каси була повною.

-- ── Деталізація чеків (FK на receipts.client_uuid, 0004) ──────────────────
CREATE TABLE IF NOT EXISTS receipt_items (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    receipt_client_uuid  TEXT NOT NULL REFERENCES receipts(client_uuid),
    product_id           TEXT,
    barcode              TEXT,
    name_snapshot        TEXT,
    quantity             INTEGER NOT NULL DEFAULT 0,  -- міліодиниці (scale 3)
    price                NUMERIC,
    price_snapshot       NUMERIC,
    sum                  NUMERIC,
    created_at           TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_receipt_items_receipt
    ON receipt_items(receipt_client_uuid);

-- ── Агрегати нових операцій (патерн receipts: data JSON + client_uuid) ────
CREATE TABLE IF NOT EXISTS purchase_orders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,               -- nullable: legacy-сумісність
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_purchase_orders_synced ON purchase_orders(synced);

CREATE TABLE IF NOT EXISTS inventories (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_inventories_synced ON inventories(synced);

CREATE TABLE IF NOT EXISTS transfers (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,                       -- точка, що створила переміщення
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_transfers_synced ON transfers(synced);

CREATE TABLE IF NOT EXISTS write_offs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_write_offs_synced ON write_offs(synced);

CREATE TABLE IF NOT EXISTS debtors_ledger (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS cash_ledger (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
