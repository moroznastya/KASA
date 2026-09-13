#!/usr/bin/env bash
export LC_ALL=C
# Бенчмарк пошуку товарів у касі (фасад :8000 → Rust products_v2).
# Вимірює повний HTTP-час (curl time_total), медіана з N прогонів після warm-up.
set -u
N=${N:-5}
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
SID=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='store_id'\").fetchone()[0])")
B=http://127.0.0.1:8000/api/v1
TMP=/tmp/bench_times.txt

bench() {
  local label="$1"; shift
  curl -s --max-time 30 -G "$B/products" -H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID" "$@" -o /dev/null  # warm-up
  : > "$TMP"
  for _ in $(seq "$N"); do
    curl -s --max-time 30 -G "$B/products" -H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID" "$@" \
      -w '%{time_total}\n' -o /tmp/bench_body.json >> "$TMP"
  done
  local med
  med=$(sort -n "$TMP" | awk '{a[NR]=$1} END{print (NR%2? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2)}')
  local meta
  meta=$(python3 -c "
import json;d=json.load(open('/tmp/bench_body.json'))
print('total=%s повернуто=%d перший=%s' % (d.get('total','?'), len(d.get('items',[])), (d['items'][0]['title'][:28] if d.get('items') else '—')))" 2>/dev/null)
  printf '%-26s %6s мс   %s\n' "$label" "$(python3 -c "print(round($med*1000))")" "$meta"
}

echo "=== ПОШУК У КАСІ (медіана з $N, після warm-up) ==="
bench "каталог (без тексту)"        --data-urlencode "size=20"
bench "query=ов"                    --data-urlencode "query=ов"     --data-urlencode "size=20"
bench "query=молоко"                --data-urlencode "query=молоко" --data-urlencode "size=20"
