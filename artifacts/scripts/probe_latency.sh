#!/usr/bin/env bash
export LC_ALL=C
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
SID=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='store_id'\").fetchone()[0])")
B=http://127.0.0.1:8000/api/v1
A=(-H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID")

t() {
  local l="$1"; shift
  for _ in 1 2 3 4 5; do curl -s -o /dev/null --max-time 20 "$@" -w '%{time_total}\n'; done \
    | sort -n | sed -n 3p | xargs -I{} python3 -c "print('  %-30s %5d мс' % ('$l', round(float('{}')*1000)))"
}

echo "=== декомпозиція (медіана з 5, srv) ==="
t "health (публічний)"     "$B/health"
t "БЕЗ токена (401?)"      "$B/products?size=1"
t "битий токен (401?)"     -H "Authorization: Bearer deadbeef" -H "X-Store-Id: $SID" "$B/products?size=1"
t "картка товару"          "${A[@]}" "$B/products/450da204-1545-4784-b1db-7a0a2d01ec4d"
t "категорії"              "${A[@]}" "$B/categories"
t "список size=1"          "${A[@]}" "$B/products?size=1"
t "список size=20"         "${A[@]}" "$B/products?size=20"
t "список size=100"        "${A[@]}" "$B/products?size=100"
