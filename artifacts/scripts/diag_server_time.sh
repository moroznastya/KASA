#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# 1) Серверний час SQL (без мережі)  2) параметри sqlx-з'єднання
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q"

echo "=== А. Серверний час запитів каталогу (EXPLAIN ANALYZE) ==="
echo "--- каталог без фільтра (пагінація вікном) ---"
$HUB -c "EXPLAIN (ANALYZE, TIMING ON, SUMMARY ON, FORMAT TEXT)
SELECT id,title,price FROM products ORDER BY title LIMIT 20 OFFSET 0" 2>&1 | grep -E "Execution Time|Planning Time"
echo "--- пошук ILIKE '%ов%' ---"
$HUB -c "EXPLAIN (ANALYZE, TIMING ON, SUMMARY ON)
SELECT id,title,price FROM products WHERE title ILIKE '%ов%' ORDER BY title LIMIT 20" 2>&1 | grep -E "Execution Time|Planning Time|Seq Scan"
echo "--- пошук ILIKE '%молоко%' ---"
$HUB -c "EXPLAIN (ANALYZE, TIMING ON, SUMMARY ON)
SELECT id,title,price FROM products WHERE title ILIKE '%молоко%' LIMIT 20" 2>&1 | grep -E "Execution Time|Planning Time"
echo "--- count(*) з ILIKE (окремий запит репозиторію) ---"
$HUB -c "EXPLAIN (ANALYZE, TIMING ON, SUMMARY ON)
SELECT count(*) FROM products WHERE title ILIKE '%молоко%'" 2>&1 | grep -E "Execution Time|Planning Time"
echo "--- індекси на products ---"
$HUB -c "select indexname from pg_indexes where tablename='products'"
echo "--- чи є GIN/trgm індекс для ILIKE ---"
$HUB -c "select indexdef from pg_indexes where tablename='products' and indexdef ilike '%trgm%'"
