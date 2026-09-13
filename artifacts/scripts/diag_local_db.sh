#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Чи можна читати з ЛОКАЛЬНОЇ БД вузла (127.0.0.1:5433) замість хаба?
set -u
export LC_ALL=C
LOC="psql -h 127.0.0.1 -p 5433 -U postgres -X -q -At"

echo "=== спроба з паролем застосунку ==="
PGPASSWORD=$HUB_PG_PASSWORD $LOC -c "select current_database()" 2>&1 | head -2
echo "=== список БД ==="
PGPASSWORD=$HUB_PG_PASSWORD psql -h 127.0.0.1 -p 5433 -U postgres -d postgres -X -q -Atc "select datname from pg_database order by 1" 2>&1 | head -8
echo "=== чи це репліка ==="
for db in torgashka torgashka_template pos_system; do
  out=$(PGPASSWORD=$HUB_PG_PASSWORD psql -h 127.0.0.1 -p 5433 -U postgres -d "$db" -X -q -Atc \
        "select case when pg_is_in_recovery() then 'РЕПЛІКА (read-only)' else 'primary (read-write)' end" 2>&1 | head -1)
  echo "  $db: $out"
done
echo "=== чи є дані ==="
PGPASSWORD=$HUB_PG_PASSWORD psql -h 127.0.0.1 -p 5433 -U postgres -d torgashka -X -q -c \
 "select (select count(*) from products) as товарів, (select count(*) from stores) as точок,
         (select count(*) from users) as юзерів, (select count(*) from categories) as категорій" 2>&1 | head -5
echo "=== латентність локальної БД ==="
for _ in 1 2 3; do
  s=$(date +%s%N)
  PGPASSWORD=$HUB_PG_PASSWORD $LOC -c "select count(*) from products" >/dev/null 2>&1
  e=$(date +%s%N)
  awk -v n=$(( (e-s)/1000 )) 'BEGIN{printf "  %6.1f мс\n", n/1000}'
done