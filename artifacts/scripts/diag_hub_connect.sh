#!/usr/bin/env bash

# секрети — з artifacts/.env.local (див. _lib.sh)
. "$(dirname "$0")/_lib.sh"
# Чому встановлення з'єднання до хаба коштує ~183 мс?
set -u
export LC_ALL=C PGPASSWORD=$HUB_PG_PASSWORD
HUB="psql -h $HUB_HOST -U postgres -d pos_system -X -q -At"

echo "=== A. Налаштування хаба, що впливають на connect ==="
$HUB -c "select name||' = '||setting from pg_settings
 where name in ('log_hostname','log_connections','password_encryption',
                'ssl','max_connections','shared_buffers','listen_addresses')"

echo
echo "=== B. PTR-запис для нашого IP (10.179.55.x) ==="
OURIP=$(hostname -I | awk '{print $1}')
echo "  наш IP: $OURIP"
$HUB -c "select coalesce(host(inet_client_addr()),'—') as client, coalesce(inet_server_addr()::text,'—') as server"
echo "  зворотний DNS (з хаба):"
timeout 8 $HUB -c "select coalesce((select r from (select 1) x),'—')" >/dev/null 2>&1
getent hosts $HUB_HOST >/dev/null 2>&1 && echo "  (локально $HUB_HOST резолвиться)" || echo "  хаб НЕ має PTR-запису локально → серверні зворотні lookup-и будуть гальмувати"

echo
echo "=== C. Фази одного з'єднання ==="
python3 - <<'PY'
import socket, time
t=time.perf_counter(); s=socket.create_connection(("$HUB_HOST",5432),5); t1=time.perf_counter()
print("  TCP handshake          : %6.1f мс" % ((t1-t)*1000))
# надсилаємо StartupMessage з неіснуючим юзером → сервер відповідає одразу після auth-запиту
try:
    s.sendall(b"\x00\x00\x00\x08\x04\xd2\x16\x2f")   # SSLRequest
    resp=s.recv(1); t2=time.perf_counter()
    print("  SSLRequest→'N' відповідь: %6.1f мс" % ((t2-t1)*1000))
except Exception as e:
    print("  помилка:",e)
s.close()
PY

echo
echo "=== D. Чи народжуються нові бекенди на кожен запит каси? ==="
before=$($HUB -c "select count(*) from pg_stat_activity where datname='pos_system'")
bt=$($HUB -c "select coalesce(max(backend_start)::text,'—') from pg_stat_activity where datname='pos_system'")
echo "  до запиту:  бекендів=$before  останній старт=$bt"
TOK=$(python3 -c "import sqlite3;print(sqlite3.connect('file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro',uri=True).execute(\"select value from settings where key='api_token'\").fetchone()[0])")
for _ in 1 2 3; do
  curl -s -o /dev/null -G "http://127.0.0.1:8000/api/v1/categories" \
    -H "Authorization: Bearer $TOK" -H "X-Store-Id: 65d5db51-672f-4a38-9c1e-f36c5feb5374" \
    --data-urlencode "size=5"
done
after=$($HUB -c "select count(*) from pg_stat_activity where datname='pos_system'")
at=$($HUB -c "select coalesce(max(backend_start)::text,'—') from pg_stat_activity where datname='pos_system'")
echo "  після 3 запитів: бекендів=$after  останній старт=$at"
echo "  (якщо 'останній старт' посунувся → застосунок конектиться наново, пул не тримає)"
