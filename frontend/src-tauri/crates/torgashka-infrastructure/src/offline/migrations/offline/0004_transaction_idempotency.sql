-- 0004_transaction_idempotency.sql
-- ЕТАП 4 (offline-first): client_uuid на локальних чеках каси — ключ
-- ідемпотентності push каса → сервер (sync-schema-design.md, розділ 8.1:
-- receipts.client_uuid TEXT NOT NULL UNIQUE; розділ 3 — UUID каси).
--
-- Колонка NULLABLE на рівні схеми: існуючі legacy-чеки (створені до ЕТАП 4,
-- synced = 0, без client_uuid) лишаються в старій черзі й синхронізуються
-- старим (legacy) шляхом. НОВІ чеки каси завжди отримують UUIDv4 на рівні
-- застосунку (offline/sync_push.rs::enqueue_receipt) — NOT NULL гарантує
-- застосунок, не схема (щоб не блокувати legacy-рядки).
-- SQLite UNIQUE-індекс допускає множину NULL → legacy-рядки не конфліктують.

ALTER TABLE receipts ADD COLUMN client_uuid TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_receipts_client_uuid
    ON receipts(client_uuid);
