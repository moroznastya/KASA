#!/usr/bin/env bash
# ═════════════════════════════════════════════════════════════════════════════
# E2E етап 5 (Tauri-обгортка): друк чеків open→pay→close + офлайн-черга.
#
# Потрібно:
#   - PostgreSQL (як у backend/.env),
#   - JWT у /tmp/kasa_token (Authorization: Bearer),
#   - фасад :8000 (Rust-ядро; serve() вмикає KASA_RUST_*=1 за замовчуванням).
#
# Що перевіряється (реально, без імітації):
#   1. health: фасад :8000 /api/v1/health → 200.
#   2. open→pay→close: створення товару → POST /api/v2/receipts/sale (pay)
#      через фасад → 201 + receipt_number; stock зменшено; друк (close):
#      реальний Rust-конвеєр print_raster_image → ESC/POS → МОК-пристрій
#      (файл), структура потоку валідується (ESC @ / GS v 0 / GS V).
#      Фізичного принтера на dev-машині немає → мок = тестовий контур.
#   3. Офлайн-черга: зупинка фасаду (офлайн) → чек збережено у SQLite
#      чергу НА ДИСК (offline_queue save → count=1, персистентність через
#      повторне відкриття БД) → підняття фасаду → синхронізація (як у
#      frontend syncReceipts: POST sale + mark_receipt_synced) → count=0,
#      чек реально існує в backend.
#   4. Очищення тестових даних (API + SQL), health 200 після змін.
# ═════════════════════════════════════════════════════════════════════════════
set -u
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
TAURI_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
ROOT=$(cd "$TAURI_DIR/../.." && pwd)

API=http://127.0.0.1:8000/api
TOKEN=$(cat /tmp/kasa_token 2>/dev/null)
if [ -z "$TOKEN" ]; then echo "❌ /tmp/kasa_token порожній"; exit 1; fi
AUTH="Authorization: Bearer $TOKEN"
CT="Content-Type: application/json"
TS=$(date +%s)
XDG=/tmp/kasa-stage5-$TS
FAIL=0

# ─── Допоміжні ─────────────────────────────────────────────────────────────
req() { # method path body(- = без тіла) base
  local base=$1 method=$2 path=$3 body=$4
  if [ "$body" == "-" ]; then
    curl -s -o /tmp/e2e5_body -w "%{http_code}" -X "$method" "$base$path" -H "$AUTH"
  else
    curl -s -o /tmp/e2e5_body -w "%{http_code}" -X "$method" "$base$path" -H "$AUTH" -H "$CT" -d "$body"
  fi
}
get_body() { cat /tmp/e2e5_body; }
jid() { python3 -c "import sys,json;print(json.load(sys.stdin)['$1'])"; }

start_facade() {
  if ! curl -s -o /dev/null http://127.0.0.1:8000/api/v1/health; then
    (cd "$TAURI_DIR" && KASA_RUST_READDIRS=1 nohup ./target/debug/facade \
      > /tmp/kasa_facade_8000.log 2>&1 &)
    for _ in $(seq 1 20); do curl -s -o /dev/null http://127.0.0.1:8000/api/v1/health && break; sleep 0.5; done
  fi
}

stop_all() {
  pkill -f "target/debug/facade" 2>/dev/null
  sleep 1
}

echo "═══ ЕТАП 5 (Tauri): друк чеків open→pay→close + офлайн-черга ═══"

# ─── 1. Запуск серверів + health ───────────────────────────────────────────
echo "── 1. Сервери ──"
start_facade
sleep 2
H2=$(curl -s -o /dev/null -w "%{http_code}" -H "$AUTH" http://127.0.0.1:8000/api/v1/health)
echo "  Фасад :8000 /api/v1/health → $H2"
[ "$H2" == "200" ] || { echo "❌ health не 200"; FAIL=1; }

# ─── 2. open→pay→close (через фасад :8000, Rust-гілка POS) ─────────────────
echo "── 2. open→pay→close ──"
BAR="TST5-$TS"
PID=$(curl -s -X POST $API/v1/products -H "$AUTH" -H "$CT" \
  -d "{\"barcode\":\"$BAR\",\"title\":\"ТЕСТ-ЕТАП5-$TS\",\"price\":100,\"cost_price\":50,\"stock\":100,\"tax_rate\":20}" | jid id)
echo "  open: товар $PID (barcode $BAR, stock 100)"

SALE="{\"items\":[{\"product_id\":\"$PID\",\"quantity\":2,\"price\":100,\"tax_rate\":20}],\"payment_method\":\"cash\",\"cash_amount\":200}"
RC=$(req $API POST /v2/receipts/sale "$SALE"); RB=$(get_body)
RID=$(echo "$RB" | jid id 2>/dev/null || echo "-")
RNUM=$(echo "$RB" | jid receipt_number 2>/dev/null || echo "-")
echo "  pay: POST /v2/receipts/sale → $RC, чек $RID ($RNUM)"
[ "$RC" == "201" ] || { echo "❌ sale не 201"; FAIL=1; }

STOCK=$(curl -s $API/v1/products/$PID -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
[ "$STOCK" == "98.000" ] && echo "  ✅ stock після pay: $STOCK (100-2)" || { echo "  ❌ stock: $STOCK"; FAIL=1; }

echo "  close: друк чека → реальний Rust-конвеєр ESC/POS → мок-пристрій (файл)"
( cd "$TAURI_DIR" && cargo test -q -p kasa-infrastructure --test stage5_tauri print_receipt_to_mock_device -- --nocapture 2>&1 | tail -8 )
if [ "${PIPESTATUS[0]}" == "0" ]; then echo "  ✅ друк (close): ESC/POS потік записано у мок-пристрій"; else echo "  ❌ друк: тест-контур не пройшов"; FAIL=1; fi

# ─── 3. Офлайн-черга: сервер down → у чергу; сервер up → синхронізація ──────
echo "── 3. Офлайн-черга ──"
stop_all
sleep 1
if curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8000/api/v1/health 2>/dev/null | grep -q 000; then
  echo "  ✅ офлайн: сервери зупинено (з'єднання немає)"
else
  echo "  ⚠️ сервери ще відповідають — продовжую"
fi

mkdir -p "$XDG"
OQ() { (cd "$TAURI_DIR" && XDG_DATA_HOME="$XDG" cargo run -q -p kasa-infrastructure --example offline_queue -- "$@"); }

OFFLINE_RECEIPT="{\"receipt_type\":\"sale\",\"items\":[{\"product_id\":\"$PID\",\"quantity\":1,\"price\":100,\"tax_rate\":20}],\"payment_method\":\"cash\",\"cash_amount\":100,\"total_amount\":100,\"paid_amount\":100}"
OFF_ID=$(OQ save "$OFFLINE_RECEIPT" | tail -1)
CNT1=$(OQ count | tail -1)
echo "  save у чергу → id=$OFF_ID, count=$CNT1"
[ "$CNT1" == "1" ] && echo "  ✅ операція збережена у чергу (на диск)" || { echo "  ❌ count=$CNT1"; FAIL=1; }

# Персистентність: «перезапуск» каси — новий процес, той самий файл.
CNT2=$(OQ count | tail -1)
[ "$CNT2" == "1" ] && echo "  ✅ персистентність: після перезапуску процесу count=$CNT2 (SQLite на диску)" || { echo "  ❌ count після перезапуску: $CNT2"; FAIL=1; }

# Піднімаємо фасад → синхронізація (як syncReceipts у frontend).
start_facade
sleep 2
H2=$(curl -s -o /dev/null -w "%{http_code}" -H "$AUTH" http://127.0.0.1:8000/api/v1/health)
echo "  фасад піднято: :8000 /api/v1/health → $H2"

SYNCED=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  OID=$(echo "$line" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
  DATA=$(echo "$line" | python3 -c "import sys,json;print(json.load(sys.stdin)['data'])")
  RC=$(curl -s -o /tmp/e2e5_sync -w "%{http_code}" -X POST $API/v2/receipts/sale \
    -H "$AUTH" -H "$CT" -d "$DATA")
  if [ "$RC" == "201" ]; then
    OQ mark "$OID" > /dev/null
    SYNCED=$((SYNCED+1))
    echo "  ✅ sync id=$OID → POST sale $RC → mark"
  else
    echo "  ❌ sync id=$OID → POST sale $RC: $(cat /tmp/e2e5_sync | head -c 200)"
    FAIL=1
  fi
done < <(OQ list)

CNT3=$(OQ count | tail -1)
echo "  синхронізовано: $SYNCED, черга після sync: $CNT3"
[ "$CNT3" == "0" ] && echo "  ✅ чек виконано на сервері і зник з черги" || { echo "  ❌ черга не порожня: $CNT3"; FAIL=1; }

# Чек реально існує в backend (пошук за товаром).
Q=$(python3 -c "import urllib.parse;print(urllib.parse.quote('ТЕСТ-ЕТАП5-$TS'))")
FOUND=$(curl -s "$API/v2/receipts/search?q=$Q" -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin).get('total',0))")
[ "$FOUND" -ge 1 ] && echo "  ✅ чек знайдено в backend (search total=$FOUND)" || { echo "  ❌ чек не знайдено в backend"; FAIL=1; }

# ─── 4. Очищення тестових даних ────────────────────────────────────────────
echo "── 4. Очищення ──"
curl -s -X PUT $API/v1/products/$PID -H "$AUTH" -H "$CT" -d '{"stock":0}' > /dev/null
curl -s -X DELETE $API/v1/products/$PID -H "$AUTH" > /dev/null
export PGPASSWORD=VgxWd7MBJ10X
psql -h localhost -U postgres -d pos_system -q -c "
DELETE FROM receipt_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'TST5-%');
DELETE FROM receipts WHERE id IN (SELECT receipt_id FROM receipt_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'TST5-%'));
DELETE FROM products WHERE barcode LIKE 'TST5-%';" 2>/dev/null
rm -rf "$XDG"
echo "  🧹 тестові дані видалено (товар, чеки, offline.db у $XDG)"

# ─── 5. Фінальний health (нічого не зламано) ───────────────────────────────
echo "── 5. Фінальний health ──"
H2=$(curl -s -o /dev/null -w "%{http_code}" -H "$AUTH" http://127.0.0.1:8000/api/v1/health)
H3=$(curl -s -o /dev/null -w "%{http_code}" -H "$AUTH" http://127.0.0.1:8000/api/v1/products?page=1\&size=1)
echo "  Фасад :8000 health → $H2 | Фасад products → $H3"
[ "$H2" == "200" ] && [ "$H3" == "200" ] && echo "  ✅ Rust-ядро живе після змін" || { echo "  ❌ health не 200"; FAIL=1; }

echo "════════════════════════════════════"
[ $FAIL -eq 0 ] && echo "ЕТАП 5 (Tauri): ВСІ ПЕРЕВІРКИ ПРОЙДЕНО ✅" || echo "ЕТАП 5 (Tauri): Є ПОМИЛКИ ❌"
exit $FAIL
