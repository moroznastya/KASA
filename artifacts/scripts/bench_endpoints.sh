#!/usr/bin/env bash
# Розклад латентності по ендпоінтах каси (локальний API :8000)
set -u
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
SID=65d5db51-672f-4a38-9c1e-f36c5feb5374

t() {  # t <підпис> <шлях> [curl-args...]
  local label="$1" path="$2"; shift 2
  local r
  r=$(curl -s -o /dev/null -w "%{time_starttransfer}" --max-time 60 \
      -G "http://127.0.0.1:8000$path" \
      -H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID" "$@")
  printf "  %-34s ttfb=%.3fs\n" "$label" "$r"
}

echo "=== розклад по ендпоінтах (1 прогрів + 1 вимір) ==="
# прогрів
curl -s -o /dev/null "http://127.0.0.1:8000/api/v1/health"
t "health (без БД)"         /api/v1/health
t "categories (1 запит)"    /api/v1/categories  --data-urlencode "size=5"
t "suppliers (2 запити)"    /api/v1/suppliers   --data-urlencode "size=5"
t "users"                   /api/v1/users
t "products size=1"         /api/v1/products    --data-urlencode "size=1"
t "products size=20"        /api/v1/products    --data-urlencode "size=20"
t "products query=молоко"   /api/v1/products    --data-urlencode "query=молоко" --data-urlencode "size=20"
t "products page=50"        /api/v1/products    --data-urlencode "page=50" --data-urlencode "size=20"
t "barcode lookup"          /api/v1/products/barcode/4820267430787
