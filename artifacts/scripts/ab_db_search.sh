#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# DB-рівневий A/B: стара поведінка (витягти ВЕСЬ матчинг, сортувати в Rust)
# проти нової (сортування + LIMIT у PostgreSQL). Обидва — на тій самій
# робочій БД, тож цифри не залежать від перезапусків застосунку.
export LC_ALL=C
PHPASS=$HUB_PG_PASSWORD
DBH=$HUB_HOST
DB=pos_system
SID=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='store_id'\").fetchone()[0])")

COLS="SELECT p.id, p.barcode, p.sku, p.title, p.description,
        COALESCE(NULLIF(st.price, 0), p.price)::text AS price,
        p.cost_price::text, p.markup::text,
        COALESCE(st.quantity, p.stock)::text AS stock,
        p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text,
        p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id,
        p.created_at, p.updated_at"
JOIN=" FROM products p LEFT JOIN stock st ON st.product_id = p.id
       AND st.store_id = NULLIF(current_setting('app.store_id', true), '')::uuid"

SEARCH_WHERE=" WHERE (p.title ILIKE '%ов%' OR p.barcode ILIKE '%ов%' OR p.sku ILIKE '%ов%')"
ORDER_REL=" ORDER BY (COALESCE(st.quantity, p.stock, 0) > 0) DESC,
        CASE
          WHEN starts_with(lower(p.title), 'ов') THEN 0
          WHEN position(' ов' in lower(p.title)) > 0 THEN 1
          WHEN position('ов' in lower(p.title)) > 0 THEN 2
          WHEN position('ов' in lower(COALESCE(NULLIF(p.barcode, ''), p.sku, ''))) > 0 THEN 3
          ELSE 4 END,
        lower(p.title) COLLATE \"C\""
PLAIN_ORDER=" ORDER BY lower(p.title) COLLATE \"C\""

run() {
  local label="$1" sql="$2"
  local out
  out=$(PGPASSWORD=$PHPASS psql -h $DBH -U postgres -d $DB -X -q -o /dev/null -c "SET app.store_id='$SID';" -c "\timing on" -c "$sql" 2>&1 | grep -i "time:" | tail -1)
  local rows
  rows=$(PGPASSWORD=$PHPASS psql -h $DBH -U postgres -d $DB -X -q -Atc "SET app.store_id='$SID'; $sql" 2>/dev/null | wc -l)
  printf '  %-40s %-12s рядків=%s\n' "$label" "$out" "$rows"
}

echo "=== A/B на рівні БД (робоча: $DBH/$DB) ==="
run "СТАРЕ: каталог, весь матчинг"        "$COLS$JOIN$PLAIN_ORDER"
run "НОВЕ:  каталог, LIMIT 20"            "$COLS, count(*) OVER () AS total_count$JOIN$PLAIN_ORDER LIMIT 20 OFFSET 0"
run "СТАРЕ: пошук 'ов', весь матчинг"     "$COLS$JOIN$SEARCH_WHERE$ORDER_REL"
run "НОВЕ:  пошук 'ов', LIMIT 20"         "$COLS, count(*) OVER () AS total_count$JOIN$SEARCH_WHERE$ORDER_REL LIMIT 20 OFFSET 0"
run "довідково: count(*) без умов"        "SELECT count(*) FROM products"
