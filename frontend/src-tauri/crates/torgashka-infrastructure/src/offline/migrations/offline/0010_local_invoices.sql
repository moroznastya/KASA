-- 0010_local_invoices.sql
-- ПРИБУТКОВА НАКЛАДНА (invoice) на standby-вузлі (ADR-0007 §3.4, клас
-- LOCAL_SQLITE): локальний агрегат + деталізація позицій + outbox-опу
-- «invoice». Патерн — ТОЧНО як 0006:
--   * агрегат: client_uuid UNIQUE + data JSON (payload як його формує фронт
--     для /v2-ендпоінтів) + store_id + synced;
--   * деталізація: окрема таблиця позицій (як receipt_items, 0006) —
--     локальний перегляд накладної БЕЗ парсингу data;
--   * stock-ефект прибуткової (+qty) робить enqueue_invoice АТОМАРНО з
--     записом агрегата й outbox-записом (одна транзакція, дизайн 4.4).
-- Колонки supplier_id/number винесені з data окремо для локальних
-- запитів/діагностики («накладна постачальника X»).

CREATE TABLE IF NOT EXISTS invoices (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    client_uuid TEXT UNIQUE,               -- nullable: legacy-сумісність (0006)
    store_id    TEXT,
    supplier_id TEXT,
    number      TEXT,
    data        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    synced      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_invoices_synced ON invoices(synced);

CREATE TABLE IF NOT EXISTS invoice_items (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_client_uuid TEXT NOT NULL REFERENCES invoices(client_uuid),
    product_id          TEXT,
    quantity            INTEGER NOT NULL DEFAULT 0,  -- міліодиниці (scale 3)
    price               NUMERIC,
    sum                 NUMERIC,
    created_at          TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_invoice_items_invoice
    ON invoice_items(invoice_client_uuid);
