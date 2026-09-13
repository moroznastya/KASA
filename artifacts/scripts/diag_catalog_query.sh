#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Серверний час ТОЧНОГО запиту каталогу каса-репозиторію
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q"

Q="SELECT p.id, p.barcode, p.sku, p.title, p.description,
          COALESCE(NULLIF(st.price, 0), p.price)::text AS price,
          p.cost_price::text, p.markup::text,
          COALESCE(st.quantity, p.stock)::text AS stock,
          p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text,
          p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id,
          p.created_at, p.updated_at, count(*) OVER () AS total_count
   FROM products p
   LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = '65d5db51-672f-4a38-9c1e-f36c5feb5374'
   ORDER BY lower(p.title) COLLATE \"C\" LIMIT 20 OFFSET 0"

echo "=== план точного запиту каталогу ==="
$HUB -c "EXPLAIN (ANALYZE, SUMMARY ON) $Q" 2>&1 | grep -E "Execution Time|Planning Time|Sort|Seq Scan|WindowAgg|Index Scan|Hash" | head -12

echo
echo "=== чи покриває ix_products_title вираз lower(title) COLLATE \"C\" ==="
$HUB -c "EXPLAIN (ANALYZE, SUMMARY ON)
  SELECT id FROM products ORDER BY lower(title) COLLATE \"C\" LIMIT 20 OFFSET 0" 2>&1 \
  | grep -E "Execution Time|Sort|Index" | head -6

echo
echo "=== що на stock (JOIN) ==="
$HUB -c "select 'stock: '||count(*)||' рядків, індекси: '||
   (select string_agg(indexname,',') from pg_indexes where tablename='stock') from stock"
