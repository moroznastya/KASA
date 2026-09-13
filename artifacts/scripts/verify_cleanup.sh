#!/usr/bin/env bash
# Приймальний тест очищення: що реально бачить каса через API хаба.
set -u
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
SID=65d5db51-672f-4a38-9c1e-f36c5feb5374   # Білий магазин

api() {  # api <path> [curl-args...]
  local path="$1"; shift
  curl -s --max-time 25 -G "http://127.0.0.1:8000$path" \
       -H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID" "$@"
}

echo "=== 1. КАТАЛОГ ТОЧКИ «Білий магазин» ==="
api /api/v1/products --data-urlencode "size=1" \
 | python3 -c "import sys,json;d=json.load(sys.stdin);print('   total =',d['total'])"

echo "=== 2. СМІТТЯ ЧЕРЕЗ ПОШУК ==="
for q in ТЕСТ E2E тест; do
  api /api/v1/products --data-urlencode "query=$q" --data-urlencode "size=1" \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print('   query=%-5s → total=%s' % ('$q', d['total']))"
done

echo "=== 3. ПОШУК 'ов' — раніше першим ішов «E2E 4-каси Товар» ==="
api /api/v1/products --data-urlencode "query=ов" --data-urlencode "size=5" \
 | python3 -c "
import sys,json;d=json.load(sys.stdin)
print('   total =',d['total'])
for p in d['items']: print('     %-42s %s' % (p['title'][:42], p['barcode']))"

echo "=== 4. ТОЧКИ У ПЕРЕМИКАЧІ ==="
api /api/v1/stores \
 | python3 -c "
import sys,json;d=json.load(sys.stdin)
items=d if isinstance(d,list) else d.get('items',d.get('stores',[]))
print('   усього:',len(items))
for s in items: print('     ',s.get('name'))"

echo "=== 5. ПОСТАЧАЛЬНИКИ ==="
api /api/v1/suppliers \
 | python3 -c "
import sys,json;d=json.load(sys.stdin)
items=d if isinstance(d,list) else d.get('items',[])
bad=[s for s in items if 'тест' in s.get('name','').lower() or 'e2e' in s.get('name','').lower()]
print('   усього: %d, тестових: %d' % (len(items), len(bad)))"

echo "=== 6. ЗДОРОВ'Я ==="
api /api/v1/health | head -c 200; echo
