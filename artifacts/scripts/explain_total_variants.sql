-- Вибір способу отримати total для каталогу товарів (міряється на хабі).
SET plan_cache_mode = force_generic_plan;
\echo === V0: поточний (count(*) OVER () в сторінковому запиті) ===
EXPLAIN (ANALYZE, TIMING OFF) SELECT p.id, p.barcode, p.sku, p.title, p.description, COALESCE(NULLIF(st.price,0),p.price)::text AS price, p.cost_price::text, p.markup::text, COALESCE(st.quantity,p.stock)::text AS stock, p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text, p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id, p.created_at, p.updated_at, count(*) OVER () AS total_count FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid ORDER BY lower(p.title) COLLATE "C" LIMIT 20 OFFSET 0;
\echo
\echo === V1: скалярний підзапит count у SELECT (1 statement) ===
EXPLAIN (ANALYZE, TIMING OFF) SELECT p.id, p.barcode, p.sku, p.title, p.description, COALESCE(NULLIF(st.price,0),p.price)::text AS price, p.cost_price::text, p.markup::text, COALESCE(st.quantity,p.stock)::text AS stock, p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text, p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id, p.created_at, p.updated_at, (SELECT count(*) FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid) AS total_count FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid ORDER BY lower(p.title) COLLATE "C" LIMIT 20 OFFSET 0;
\echo
\echo === V1b: окремий count-запит (окремий круг) ===
EXPLAIN (ANALYZE, TIMING OFF) SELECT count(*) FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid;
\echo
\echo === V3: лише сторінка (без total) ===
EXPLAIN (ANALYZE, TIMING OFF) SELECT p.id, p.title FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'::uuid ORDER BY lower(p.title) COLLATE "C" LIMIT 20 OFFSET 0;
