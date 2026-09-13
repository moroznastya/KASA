#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Скільки SQL-запитів робить ОДИН HTTP-виклик каси (через pg_stat_statements)
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q -At"
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
SID=65d5db51-672f-4a38-9c1e-f36c5feb5374

if [ "$($HUB -c "select count(*) from pg_extension where extname='pg_stat_statements'")" != "1" ]; then
  echo "pg_stat_statements не встановлено — вмикаю"
  $HUB -c "CREATE EXTENSION IF NOT EXISTS pg_stat_statements" 2>&1 | head -2
fi
echo "extensions: $($HUB -c "select string_agg(extname,',') from pg_extension")"

total_calls() { $HUB -c "select coalesce(sum(calls),0) from pg_stat_statements where dbid=(select oid from pg_database where datname='pos_system')"; }
reset()       { $HUB -c "select pg_stat_statements_reset()" >/dev/null; }

probe() {  # probe <підпис> <шлях> [curl-args]
  local label="$1" path="$2"; shift 2
  reset
  local s e
  s=$(date +%s%N)
  curl -s -o /dev/null --max-time 60 -G "http://127.0.0.1:8000$path" \
    -H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID" "$@"
  e=$(date +%s%N)
  local calls; calls=$(total_calls)
  awk -v ms=$(( (e-s)/1000 )) -v c="$calls" -v l="$label" \
    'BEGIN{printf "  %-30s %6.1f мс | %2d SQL-запитів\n", l, ms/1000, c}'
}

echo
echo "=== запитів на один HTTP-виклик ==="
probe "health (без БД)"     /api/v1/health
probe "categories"          /api/v1/categories  --data-urlencode "size=5"
probe "suppliers"           /api/v1/suppliers   --data-urlencode "size=5"
probe "stores"              /api/v1/stores
probe "users"               /api/v1/users
probe "products size=20"    /api/v1/products    --data-urlencode "size=20"
probe "products query"      /api/v1/products    --data-urlencode "query=молоко" --data-urlencode "size=20"
probe "barcode lookup"      /api/v1/products/barcode/4820267430787

echo
echo "=== ТОП запитів за кількістю викликів (останній probe) ==="
$HUB -c "select calls||' x '||round(total_exec_time::numeric,2)||'мс  '||left(regexp_replace(query,'\s+',' ','g'),90)
         from pg_stat_statements where dbid=(select oid from pg_database where datname='pos_system')
         order by calls desc, total_exec_time desc limit 12"
