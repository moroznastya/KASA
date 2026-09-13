#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Видалення користувача 'admin' ШТАТНИМ шляхом застосунку:
#   DELETE /api/v1/users/:id  (роль admin, не сам себе, без чеків)
set -u
export LC_ALL=C
cd /home/anastasia/Andriy/aegis_v3/Niko/Projects/Torgashka || exit 1

TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
ADMIN=dded5c75-d093-4ab9-9a1c-bd61a35d2816     # login='admin'
SID=65d5db51-672f-4a38-9c1e-f36c5feb5374

echo "=== ДО видалення ==="
export PGPASSWORD=$HUB_PG_PASSWORD
PSQL="psql -h $HUB_HOST -U postgres -d pos_system -X -q -At"
$PSQL -c "select '  users: '||count(*)||' | work_sessions: '||(select count(*) from work_sessions)
          ||' | сесій admin: '||(select count(*) from work_sessions where user_id='$ADMIN') from users"

echo
echo "=== DELETE /api/v1/users/$ADMIN ==="
curl -s -o /tmp/del.out -w "  HTTP %{http_code} за %{time_total}s\n" \
  -X DELETE "http://127.0.0.1:8000/api/v1/users/$ADMIN" \
  -H "Authorization: Bearer $TOK" -H "X-Store-Id: $SID"
[ -s /tmp/del.out ] && echo "  тіло: $(head -c 300 /tmp/del.out)"

echo
echo "=== ПІСЛЯ видалення ==="
$PSQL -c "select '  users: '||count(*)||' | work_sessions: '||(select count(*) from work_sessions)
          ||' | сесій admin: '||(select count(*) from work_sessions where user_id='$ADMIN') from users"
echo "  --- хто лишився ---"
$PSQL -c "select '    '||login||' ('||role||')' from users order by created_at"
echo "  --- сесії по користувачах (реальні збережено) ---"
$PSQL -c "select '    '||coalesce(u.login,'(видалений)')||': '||count(*)
          from work_sessions w left join users u on u.id=w.user_id group by 1 order by 1"

echo
echo "=== чи прибрався admin у кеші вузла ==="
python3 -c "
import sqlite3;c=sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True)
def q(s):
    try: return c.execute(s).fetchone()
    except Exception as e: return ('—',e)
print('  settings(api_token) ще живий:', bool(q(\"select value from settings where key='api_token'\")))
print('  (вузол тягне users раз на 30 с — перевірка нижче)')"
