-- ============================================================================
-- ОЧИЩЕННЯ ТЕСТОВИХ ДАНИХ integration-тестів з pos_system_fresh
-- Дата: 2026-08-21
-- ----------------------------------------------------------------------------
-- Видаляє:
--   1) Товари title LIKE 'ТЕСТ-POS%' (6 шт) + усі записи-діти
--      (stock, barcodes, write_off_items, transfer_items, inventory_items,
--       invoice_items, product_images, purchase_order_items, return_invoice_items)
--   2) Чеки RCPT-20260821-* (10 шт), що містять ЛИШЕ тестові позиції,
--      разом з їх receipt_items. Реальні чеки не чіпаємо (їх немає — перевірено:
--      0 чеків поза префіксом RCPT-20260821).
-- Порядок: діти → receipts → products. Уся робота в одній транзакції.
-- ============================================================================

BEGIN;

-- ----------------------------------------------------------------------------
-- 0. Позиції тестових чеків (діти чеків)
--    Кваліфікація чеку: RCPT-20260821-* БЕЗ жодної позиції, що вказує на
--    не-тестовий або відсутній товар (NOT EXISTS порожнього = тестовий).
-- ----------------------------------------------------------------------------
DELETE FROM receipt_items ri
WHERE ri.receipt_id IN (
    SELECT r.id FROM receipts r
    WHERE r.receipt_number LIKE 'RCPT-20260821%'
      AND NOT EXISTS (
          SELECT 1 FROM receipt_items x
          WHERE x.receipt_id = r.id
            AND (x.product_id IS NULL
                 OR x.product_id NOT IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%'))
      )
);

-- ----------------------------------------------------------------------------
-- 1. Діти тестових товарів (усі 10 FK-таблиць, що посилаються на products.id)
-- ----------------------------------------------------------------------------
DELETE FROM stock               WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM barcodes            WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM write_off_items     WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM transfer_items      WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM inventory_items     WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM invoice_items       WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM product_images      WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM purchase_order_items WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');
DELETE FROM return_invoice_items WHERE product_id IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%');

-- ----------------------------------------------------------------------------
-- 2. Тестові чеки (та сама кваліфікація, що й у кроці 0)
-- ----------------------------------------------------------------------------
DELETE FROM receipts
WHERE receipt_number LIKE 'RCPT-20260821%'
  AND NOT EXISTS (
      SELECT 1 FROM receipt_items ri
      WHERE ri.receipt_id = receipts.id
        AND (ri.product_id IS NULL
             OR ri.product_id NOT IN (SELECT id FROM products WHERE title LIKE 'ТЕСТ-POS%'))
  );

-- ----------------------------------------------------------------------------
-- 3. Тестові товари
-- ----------------------------------------------------------------------------
DELETE FROM products WHERE title LIKE 'ТЕСТ-POS%';

-- Контрольні лічильники
SELECT 'products_left' AS check_name, count(*) AS cnt FROM products WHERE title LIKE 'ТЕСТ-POS%'
UNION ALL SELECT 'receipts_left', count(*) FROM receipts WHERE receipt_number LIKE 'RCPT-20260821%'
UNION ALL SELECT 'receipt_items_left', count(*) FROM receipt_items
UNION ALL SELECT 'transfer_items_left', count(*) FROM transfer_items;

COMMIT;
