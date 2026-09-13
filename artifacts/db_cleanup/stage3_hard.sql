-- ============================================================================
-- ЕТАП 3/3 — ФІЗИЧНЕ видалення тестових даних
-- NIKO · 2026-09-13 · хаб <hub-host>/pos_system
-- ПЕРЕДУМОВА: ЕТАП 1 виконано І вузол протягнув дельту (етал 2 перевірено).
-- ============================================================================
BEGIN;

-- 3.1 Точки. Усі 263 чеки — у цих точках, тож сиріт-документів не лишиться.
--     CASCADE: receipts, receipt_items, stock(114), user_stores(33), sync_log(457).
DELETE FROM stores WHERE name ~* '(тест|e2e)';

-- 3.2 Товари. FK RESTRICT (receipt_items) уже не заважає — чеки знято в 3.1.
--     CASCADE: stock (552 рядки у Білому/Жовтому), barcodes, product_images.
DELETE FROM products WHERE title ~* '(тест|e2e)' OR barcode ~* '(тест|e2e)';

-- 3.3 Постачальники (посилання з «Піна Колада» знято в 1.2).
--     FK на suppliers: invoices, products, purchase_orders, return_invoices,
--     supplier_ledger — усі або порожні, або вже не вказують на ці id.
DELETE FROM suppliers
 WHERE id IN ('9789902c-ecd7-4e2d-a9c9-b21809f57a8b',
              'dd89911c-b6dd-4fe2-a60e-5e1036ec0ad6');

COMMIT;
