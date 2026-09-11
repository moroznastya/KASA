-- 0012_local_return_debtor_ledger.sql
-- ФАЗА 3.3b (ADR-0007 §11.7.9.7): ще три сутності класу `LocalOutbox` на
-- standby-вузлі отримують локальний шлях запису.
--
--   * `return_invoice` — ПОВЕРНЕННЯ ПОСТАЧАЛЬНИКУ (не чек покупця
--     `return_receipt` → `receipts`!). Агрегат `return_invoices` +
--     stock-ефект **−qty** (товар іде назад постачальнику) — атомарно в
--     `transactions::enqueue_transaction`.
--   * `debtor_payment` — оплата боргу покупця. Агрегат — НАЯВНА таблиця 0006
--     `debtors_ledger` (client_uuid + data + store_id + synced), окремої не
--     заводимо. Похідний стан — `debtor_balances` (скільки оплат каси ще НЕ
--     дійшло до primary).
--   * `supplier_ledger` — РУЧНИЙ запис у книгу постачальника (самостійний
--     документ, не похідна накладної). Агрегат `supplier_ledger`; похідний
--     стан — `supplier_balances` (сума несинхронізованих записів).
--
-- Конвенція агрегатів — точно як 0006/0010: client_uuid UNIQUE + data JSON
-- (payload як його приймає приймач на primary) + store_id + synced.
-- Похідні таблиці (`*_balances`) — ЛОКАЛЬНА ОЦІНКА каси (той самий статус,
-- що локальний `stock`/`cash_balance`); авторитетні баланси рахує primary
-- (`stock`, `debtors.total_debt`, `SUM(supplier_ledger.amount)`).

CREATE TABLE IF NOT EXISTS return_invoices (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_return_invoices_synced ON return_invoices(synced);

CREATE TABLE IF NOT EXISTS supplier_ledger (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,
    store_id    TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_supplier_ledger_synced ON supplier_ledger(synced);

-- Борг покупця за очима каси: сума оплат, ще не підтверджених primary.
-- Гроші — КОПІЙКИ (ціле, scale 2, як cash_balance 0011), без f64.
CREATE TABLE IF NOT EXISTS debtor_balances (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id      TEXT NOT NULL,
    debtor_id     TEXT NOT NULL,
    pending_cents INTEGER NOT NULL DEFAULT 0,
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (store_id, debtor_id)
);

-- Книга постачальника за очима каси: сума несинхронізованих записів (копійки).
CREATE TABLE IF NOT EXISTS supplier_balances (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id             TEXT NOT NULL,
    supplier_id          TEXT NOT NULL,
    pending_amount_cents INTEGER NOT NULL DEFAULT 0,
    updated_at           TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (store_id, supplier_id)
);
