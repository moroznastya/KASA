-- 0013_local_device_heartbeat.sql
-- ФАЗА 3.3b (ADR-0007 §11.7.9.8): життєвість device-каси БЕЗ запису в репліку.
--
-- `api/store_context.rs` на КОЖЕН запит device-каси виконував
-- `UPDATE devices SET last_seen_at = now()` через `state.store_pool` — на
-- standby це локальна **read-only** репліка: помилка лише логується (500 не
-- виникає), але серцебиття не пишеться і кожен запит лишає рядок помилки в
-- лог. Клас §11.1 для `devices` — `ProxyToPrimary`, але ця точка не має
-- HTTP-поверхні (це middleware) і не потребує негайної дії на primary, тому
-- на standby її місце — локальний SQLite-журнал (шар локальної копії §11.7.4).
--
-- Авторитетне `devices.last_seen_at` пише primary (device-авторизація йде
-- саме туди). Локальний журнал — діагностика каси.

CREATE TABLE IF NOT EXISTS device_heartbeats (
    device_id    TEXT PRIMARY KEY,
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
);
