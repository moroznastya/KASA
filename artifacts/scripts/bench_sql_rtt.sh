#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Точна латентність ОДНОГО запиту на ВЖЕ відкритому з'єднанні (RTT у SQL)
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q -At"

echo "=== 10 запитів в ОДНОМУ з'єднанні (кожен -c = окремий round-trip) ==="
ARGS=(); for _ in $(seq 1 10); do ARGS+=(-c "select 1"); done
/usr/bin/time -f "  разом: %e с" $HUB "${ARGS[@]}" >/dev/null 2>/tmp/t10
cat /tmp/t10 | tail -1
python3 -c "
import re
t=open('/tmp/t10').read()
m=re.search(r'разом: ([\d.]+)',t)
tot=float(m.group(1))*1000
print('  10 round-trip-ів: %.0f мс → 1 round-trip ≈ %.1f мс' % (tot, tot/10))"

echo
echo "=== той самий запит, але SELECT із реальної таблиці ==="
ARGS=(); for _ in $(seq 1 10); do ARGS+=(-c "select count(*) from categories"); done
/usr/bin/time -f "  разом: %e с" $HUB "${ARGS[@]}" >/dev/null 2>/tmp/t10b
python3 -c "
import re
t=open('/tmp/t10b').read()
tot=float(re.search(r'разом: ([\d.]+)',t).group(1))*1000
print('  10 запитів: %.0f мс → 1 запит ≈ %.1f мс' % (tot, tot/10))"

echo
echo "=== чи має значення розмір відповіді (каталог 20 рядків) ==="
ARGS=(); for _ in $(seq 1 10); do ARGS+=(-c "select id,title,price,barcode from products order by title limit 20"); done
/usr/bin/time -f "  разом: %e с" $HUB "${ARGS[@]}" >/dev/null 2>/tmp/t10c
python3 -c "
import re
t=open('/tmp/t10c').read()
tot=float(re.search(r'разом: ([\d.]+)',t).group(1))*1000
print('  10 запитів каталогу: %.0f мс → 1 запит ≈ %.1f мс' % (tot, tot/10))"
