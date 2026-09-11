-- 0008_sync_log.sql — локальний журнал подій синхронізації каси.
--
-- ЕТАП 7 (дизайн sync-schema-design.md, розділ 9 «Тести, приймання,
-- моніторинг»): моніторинг sync_log — алерт на failed/стагнацію.
--
-- Кожна подія push/pull (успіх і помилка) пишеться сюди в межах своєї
-- транзакції (див. sync_push.rs mark_done/mark_failed/defer_or_fail,
-- sync_pull.rs pull_all). Це ЛОКАЛЬНИЙ журнал каси (SQLite) — окремо від
-- серверного sync_log у PostgreSQL (0011_sync_server_schema: аудит/SLA
-- прийому). Тут — діагностика каси зсередини: чи доходить push/pull,
-- скільки спроб, коли була остання успішна синхронізація.
--
-- kind:
--   push_ok    — агрегат прийнято сервером (created/already_exists → done)
--   push_fail  — агрегат перейшов у failed (бізнес/валідація) або мережева
--                помилка циклу push (entity=NULL)
--   retry      — 5xx/429: спробу відкладено exponential backoff
--                (detail: спроба N, затримка Xс)
--   pull_ok    — сутність майстер-даних успішно оновлена (entity: назва)
--   pull_fail  — помилка pull сутності (detail: текст помилки)
-- entity: client_uuid агрегата (push) | назва сутності (pull) | NULL
-- attempts: лічильник спроб агрегата на момент події
CREATE TABLE IF NOT EXISTS sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL DEFAULT (datetime('now')),
    kind TEXT NOT NULL CHECK (kind IN ('push_ok', 'push_fail', 'pull_ok', 'pull_fail', 'retry')),
    entity TEXT,
    detail TEXT,
    attempts INTEGER
);

-- Індекс для health-запитів: останні події за типом, діапазони ts.
CREATE INDEX IF NOT EXISTS idx_sync_log_kind_ts ON sync_log(kind, ts);
