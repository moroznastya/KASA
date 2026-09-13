#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Скільки коштує ОДИН SQL-запит до хаба по мережі vs локально
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q -At"

t() {  # t <підпис> <команда...>
  local label="$1"; shift
  local s e
  s=$(date +%s%N)
  "$@" >/dev/null 2>&1
  e=$(date +%s%N)
  awk -v n=$(( (e-s)/1000 )) -v l="$label" 'BEGIN{printf "  %-42s %6.1f мс\n", l, n/1000}'
}

echo "=== ХАБ $HUB_HOST (мережа) ==="
t "пустий запит (1 RTT)"            $HUB -c "select 1"
t "count(categories)"               $HUB -c "select count(*) from categories"
t "count(products)"                 $HUB -c "select count(*) from products"
t "каталог 20 рядків"               $HUB -c "select id,title,price from products order by title limit 20"
t "пошук LIKE '%молоко%'"           $HUB -c "select count(*) from products where title ilike '%молоко%'"
t "УСЬОГО 4 psql-процеси"           bash -c "for i in 1 2 3 4; do $HUB -c 'select 1' >/dev/null 2>&1; done"

echo
echo "=== 4 RTT = скільки (модель products-хендлера) ==="
t "4 x (select 1) в одному psql"    $HUB -c "select 1; select 1; select 1; select 1"
t "4 окремі TCP-з'єднання"          bash -c "for i in 1 2 3 4; do $HUB -c 'select 1' >/dev/null 2>&1; done"
