-- 0011_local_cash.sql
-- КАСОВІ ОПЕРАЦІЇ (внесення/інкасація) на standby-вузлі (ADR-0007 §11.6,
-- клас LocalOutbox/Queue): агрегат іде в НАЯВНУ таблицю 0006 `cash_ledger`
-- (client_uuid + data + store_id + synced) — окремої таблиці агрегата не
-- заводимо (конвенція 0006: одна таблиця = один агрегат, `transactions::table_of`).
--
-- Ця міграція додає ЛИШЕ похідний стан — локальний баланс каси:
--   * `cash_type` — окремі ящики `cash` / `card` (як на сервері,
--     `cash_operations.cash_type`);
--   * `balance_cents` — ціле, КОПІЙКИ (scale 2, server numeric(12,2));
--     цілочисельна арифметика замість f64 (конвенція `stock.rs`, scale 3);
--   * UPDATE виконує `offline/cash.rs::apply_cash_delta` АТОМАРНО з записом
--     агрегата й outbox-записом (одна SQLite-транзакція, дизайн 4.4).
-- Авторитетний баланс — прерогатива primary (приймач `cash_operation`).

CREATE TABLE IF NOT EXISTS cash_balance (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id      TEXT NOT NULL,
    cash_type     TEXT NOT NULL,                    -- 'cash' | 'card'
    balance_cents INTEGER NOT NULL DEFAULT 0,       -- копійки (scale 2)
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (store_id, cash_type)
);
