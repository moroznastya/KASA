#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Чи тримає каса з'єднання до хаба, чи відкриває нове на кожен запит?
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q -At"
OUR="10.179.55.165"
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")

snap() { $HUB -c "select count(*)||' бекендів, стани: '||coalesce(string_agg(distinct state,','),'—')
                  from pg_stat_activity where datname='pos_system' and client_addr='$OUR'"; }

echo "=== з'єднання з НАШОГО IP ($OUR) ==="
echo "  спокій:        $(snap)"
$HUB -c "select pid||' | '||coalesce(state,'?')||' | start='||backend_start::time(0)||' | idle='||coalesce(now()-state_change,interval '0')::text
         from pg_stat_activity where datname='pos_system' and client_addr='$OUR' order by backend_start"

echo
echo "=== 5 ПОСЛІДОВНИХ запитів до каси (health→categories→products) ==="
for i in 1 2 3; do
  curl -s -o /dev/null -G "http://127.0.0.1:8000/api/v1/categories" \
    -H "Authorization: Bearer $TOK" -H "X-Store-Id: 65d5db51-672f-4a38-9c1e-f36c5feb5374" --data-urlencode "size=5"
  echo "  після запиту $i: $(snap)"
done

echo
echo "=== чи зростає лічильник з'єднань на хабі глобально ==="
$HUB -c "select 'numbackends total: '||numbackends from pg_stat_database where datname='pos_system'"
$HUB -c "select 'xact_commit: '||xact_commit||'  xact_rollback: '||xact_rollback from pg_stat_database where datname='pos_system'"
