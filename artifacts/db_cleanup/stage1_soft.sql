-- ============================================================================
-- ЕТАП 1/3 — М'ЯКЕ видалення (тільки дельти для вузла, жодного фізичного DELETE)
-- NIKO · 2026-09-13 · хаб <hub-host>/pos_system
-- Бекап: artifacts/db_cleanup/pos_system_before_cleanup_<TS>.sql.gz
--
-- Чому м'яко: хаб-запити каталогу НЕ фільтрують is_deleted (перевірено grep),
-- тому каса хаб НЕ бачить змін. М'яке видалення тут — ВИКЛЮЧНО транспорт
-- tombstone-ів для локального вузла (offline.db фільтрує is_deleted).
-- Фізичне видалення — окремий крок 3, ПІСЛЯ того як вузол протягне дельту.
-- ============================================================================
BEGIN;

-- 1.1 Тестові товари → is_deleted=1. Тригер trg_products_bump підніме
--     server_version, тому GET /api/v1/sync/master віддасть op=delete.
UPDATE products SET is_deleted = true, updated_at = now()
 WHERE is_deleted = false
   AND (title ~* '(тест|e2e)' OR barcode ~* '(тест|e2e)');

-- 1.2 «Тест» задіяний реальним товаром «Піна Колада» (EAN 5414145036551) —
--     знімаємо посилання, щоб товар не лишився з видаленим постачальником.
UPDATE products SET supplier_id = NULL, updated_at = now()
 WHERE supplier_id IN ('9789902c-ecd7-4e2d-a9c9-b21809f57a8b',
                       'dd89911c-b6dd-4fe2-a60e-5e1036ec0ad6');

-- 1.3 Тестові постачальники → tombstone
UPDATE suppliers SET is_deleted = true, updated_at = now()
 WHERE id IN ('9789902c-ecd7-4e2d-a9c9-b21809f57a8b',
              'dd89911c-b6dd-4fe2-a60e-5e1036ec0ad6');

COMMIT;
