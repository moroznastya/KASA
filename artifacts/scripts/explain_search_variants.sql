-- Пошук: варіанти отримання total (хаб, generic plan).
SET plan_cache_mode = force_generic_plan;
\echo === SW: пошук, WindowAgg (поточний) ===
EXPLAIN (ANALYZE, TIMING OFF) SELECT p.id, p.title, count(*) OVER () AS total_count FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid WHERE (p.title ILIKE '%молоко%' OR p.barcode ILIKE '%молоко%' OR p.sku ILIKE '%молоко%' OR EXISTS (SELECT 1 FROM barcodes b WHERE b.product_id = p.id AND b.barcode ILIKE '%молоко%')) ORDER BY (COALESCE(st.quantity, p.stock, 0) > 0) DESC, CASE WHEN starts_with(lower(p.title), 'молоко') THEN 0 WHEN position(' молоко' in lower(p.title)) > 0 THEN 1 WHEN position('молоко' in lower(p.title)) > 0 THEN 2 WHEN position('молоко' in lower(COALESCE(NULLIF(p.barcode, ''), p.sku, ''))) > 0 THEN 3 ELSE 4 END, lower(p.title) COLLATE "C" LIMIT 20 OFFSET 0;
\echo
\echo === SP: пошук, лише сторінка без total ===
EXPLAIN (ANALYZE, TIMING OFF) SELECT p.id, p.title FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid WHERE (p.title ILIKE '%молоко%' OR p.barcode ILIKE '%молоко%' OR p.sku ILIKE '%молоко%' OR EXISTS (SELECT 1 FROM barcodes b WHERE b.product_id = p.id AND b.barcode ILIKE '%молоко%')) ORDER BY (COALESCE(st.quantity, p.stock, 0) > 0) DESC, lower(p.title) COLLATE "C" LIMIT 20 OFFSET 0;
\echo
\echo === SU: пошук через UNION трgm-гілок + total як count по CTE ===
EXPLAIN (ANALYZE, TIMING OFF) WITH m AS (SELECT id, 1 AS src FROM products WHERE title ILIKE '%молоко%' UNION SELECT id, 2 FROM products WHERE barcode ILIKE '%молоко%' UNION SELECT id, 3 FROM products WHERE sku ILIKE '%молоко%' UNION SELECT product_id, 4 FROM barcodes WHERE barcode ILIKE '%молоко%'), m2 AS (SELECT DISTINCT id FROM m) SELECT p.id, p.title, (SELECT count(*) FROM m2) AS total_count FROM m2 JOIN products p ON p.id = m2.id LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid ORDER BY (COALESCE(st.quantity, p.stock, 0) > 0) DESC, CASE WHEN starts_with(lower(p.title), 'молоко') THEN 0 WHEN position(' молоко' in lower(p.title)) > 0 THEN 1 WHEN position('молоко' in lower(p.title)) > 0 THEN 2 WHEN position('молоко' in lower(COALESCE(NULLIF(p.barcode, ''), p.sku, ''))) > 0 THEN 3 ELSE 4 END, lower(p.title) COLLATE "C" LIMIT 20 OFFSET 0;
\echo
\echo === індекси barcodes/sku ===
SELECT indexname, indexdef FROM pg_indexes WHERE tablename IN ('barcodes') OR indexname LIKE '%sku%';
