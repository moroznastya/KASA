-- 0002_sync_meta.sql
-- Таблиці синхронізації offline-first (sync-schema-design.md):
--   sync_meta — розділи 1.2 / 8.1 (остання застосована версія довідника);
--   outbox    — розділ 4.1 (черга push каса → сервер, точна схема з дизайну).

CREATE TABLE IF NOT EXISTS sync_meta (
    entity  TEXT PRIMARY KEY,   -- categories|products|stock_norms|suppliers|employees|settings
    version INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    type            TEXT    NOT NULL,          -- receipt, return_receipt, ...
    client_uuid     TEXT    NOT NULL UNIQUE,   -- UUIDv4 каси (ідемпотентність push)
    payload         TEXT    NOT NULL,          -- JSON агрегата (розділ 2.2)
    status          TEXT    NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','in_flight','failed','done')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT    NOT NULL DEFAULT (datetime('now')),
    last_error      TEXT,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    pushed_at       TEXT
);

CREATE INDEX IF NOT EXISTS idx_outbox_status ON outbox(status, next_attempt_at);
CREATE INDEX IF NOT EXISTS idx_outbox_created ON outbox(created_at);
