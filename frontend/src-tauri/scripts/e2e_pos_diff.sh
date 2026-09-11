#!/usr/bin/env bash
# E2E differential POS v1: Rust-фасад (:8002) vs Python (:8001), СПІЛЬНА БД.
# Покриває: чеки v2 (sale/return/list/detail/items/stats/search/by-product/
# returnable), робочі сесії, списання, переміщення, зміни ПРРО (X/Z).
# create-порівняння — послідовно (Rust create → Python create → normalize),
# GET-порівняння — прямо (спільна БД → однакові дані).
# Потрібно: Python :8001, фасад :8002 (TORGASHKA_RUST_READDIRS=1), /tmp/torgashka_token.
set -u
RUST=http://127.0.0.1:8002/api/v1
PY=http://127.0.0.1:8001/api/v1
RUSTV2=http://127.0.0.1:8002/api/v2
PYV2=http://127.0.0.1:8001/api/v2
TOKEN=$(cat /tmp/torgashka_token)
AUTH="Authorization: Bearer $TOKEN"
CT="Content-Type: application/json"
TS=$(date +%s)
FAIL=0

norm() {
  python3 -c "
import sys, json
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at','receipt_id','write_off_id','transfer_id','inventory_id','number','login_time','logout_time','opened_at','closed_at','fiscal_sent_at','terminal_created_at','created_by_id')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}

cmp_resp() {
  local label="$1" rcode="$2" pcode="$3" rbody="$4" pbody="$5"
  if [ "$rcode" != "$pcode" ]; then
    echo "❌ $label: статус Rust=$rcode Python=$pcode"; FAIL=1; return
  fi
  local rn pn
  rn=$(echo "$rbody" | norm); pn=$(echo "$pbody" | norm)
  if [ "$rn" == "$pn" ]; then
    echo "✅ $label: $rcode, тіло ідентичне"
  else
    echo "❌ $label: $rcode, тіло РОЗБІЖНЕ"
    echo "  Rust:   $rn" | head -c 1000; echo
    echo "  Python: $pn" | head -c 1000; echo
    FAIL=1
  fi
}

req() {
  local base=$1 method=$2 path=$3 body=$4
  if [ "$body" == "-" ]; then
    curl -s -o /tmp/e2e_body -w "%{http_code}" -X "$method" "$base$path" -H "$AUTH"
  else
    curl -s -o /tmp/e2e_body -w "%{http_code}" -X "$method" "$base$path" -H "$AUTH" -H "$CT" -d "$body"
  fi
}
get_body() { cat /tmp/e2e_body; }
jid() { python3 -c "import sys,json;print(json.load(sys.stdin)['$1'])"; }

# ─── Підготовка: 2 свіжі товари (через Python — еталон) ─────────────────────
mk_prod() { # barcode title stock
  curl -s -X POST $PY/products -H "$AUTH" -H "$CT" \
    -d "{\"barcode\":\"$1\",\"title\":\"$2\",\"price\":100,\"cost_price\":50,\"stock\":$3,\"tax_rate\":20}" | jid id
}
PA=$(mk_prod "E3P-$TS-A" "E3-ПРОДУКТ-A-$TS" 100)
PBID=$(mk_prod "E3P-$TS-B" "E3-ПРОДУКТ-B-$TS" 0)
echo "товари: A=$PA (stock 100), B=$PBID (stock 0)"

# ═══ ЧЕКИ v2: sale ═══
SALE_BODY="{\"items\":[{\"product_id\":\"$PA\",\"quantity\":2,\"price\":100,\"tax_rate\":20}],\"payment_method\":\"cash\",\"cash_amount\":250}"
RC=$(req $RUSTV2 POST /receipts/sale "$SALE_BODY"); RB=$(get_body); RID=$(echo "$RB" | jid id)
PC=$(req $PYV2 POST /receipts/sale "$SALE_BODY"); PB=$(get_body); PIDR=$(echo "$PB" | jid id)
cmp_resp "POST /receipts/sale (cash, здача)" "$RC" "$PC" "$RB" "$PB"
echo "  → Rust-чек $RID, Python-чек $PIDR"

# stock зменшено на 2 (перевірка через GET product)
STOCK_R=$(curl -s $RUST/products/$PA -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
[ "$STOCK_R" == "96.000" ] && echo "✅ stock після sale: $STOCK_R (100-2-2)" || { echo "❌ stock: $STOCK_R"; FAIL=1; }

# mixed-валідація (400 ідентично)
MIXED="{\"items\":[{\"product_id\":\"$PA\",\"quantity\":1,\"price\":100}],\"payment_method\":\"mixed\",\"cash_amount\":50}"
RC=$(req $RUSTV2 POST /receipts/sale "$MIXED"); PC=$(req $PYV2 POST /receipts/sale "$MIXED")
cmp_resp "sale mixed cash+card≠total → 400" "$RC" "$PC" "$(get_body)" "$(get_body)"

# недостатньо stock (400)
LOW="{\"items\":[{\"product_id\":\"$PBID\",\"quantity\":1,\"price\":100}],\"payment_method\":\"cash\",\"cash_amount\":100}"
RC=$(req $RUSTV2 POST /receipts/sale "$LOW"); RB=$(get_body)
PC=$(req $PYV2 POST /receipts/sale "$LOW"); PB=$(get_body)
echo "  [debug] LOW Rust=$RC body=$RB | Python=$PC body=$(echo $PB | head -c 200)"
cmp_resp "sale недостатньо stock → 400" "$RC" "$PC" "$RB" "$PB"

# 422: порожній items
EMPTY='{"items":[],"payment_method":"cash"}'
RC=$(req $RUSTV2 POST /receipts/sale "$EMPTY"); PC=$(req $PYV2 POST /receipts/sale "$EMPTY")
[ "$RC" == "422" ] && [ "$PC" == "422" ] && echo "✅ 422 порожній items: Rust=$RC Python=$PC" || { echo "❌ 422 items: Rust=$RC Python=$PC"; FAIL=1; }

# 422: declined terminal (card)
DECL="{\"items\":[{\"product_id\":\"$PA\",\"quantity\":1,\"price\":100}],\"payment_method\":\"card\",\"card_amount\":100,\"terminal_status\":\"declined\"}"
RC=$(req $RUSTV2 POST /receipts/sale "$DECL"); RB=$(get_body)
PC=$(req $PYV2 POST /receipts/sale "$DECL"); PB=$(get_body)
cmp_resp "sale terminal declined → 422" "$RC" "$PC" "$RB" "$PB"

# ═══ ЧЕКИ v2: return ═══
RET_BODY="{\"items\":[{\"product_id\":\"$PA\",\"quantity\":1,\"price\":100,\"tax_rate\":20}],\"payment_method\":\"cash\",\"cash_amount\":100}"
RC=$(req $RUSTV2 POST /receipts/return "$RET_BODY"); RB=$(get_body)
PC=$(req $PYV2 POST /receipts/return "$RET_BODY"); PB=$(get_body)
cmp_resp "POST /receipts/return" "$RC" "$PC" "$RB" "$PB"
STOCK_R=$(curl -s $RUST/products/$PA -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
[ "$STOCK_R" == "98.000" ] && echo "✅ stock після return: $STOCK_R (96+1+1)" || { echo "❌ stock return: $STOCK_R"; FAIL=1; }

# return card без rrn → 422
RETCARD="{\"items\":[{\"product_id\":\"$PA\",\"quantity\":1,\"price\":100}],\"payment_method\":\"card\",\"card_amount\":100}"
RC=$(req $RUSTV2 POST /receipts/return "$RETCARD"); RB=$(get_body)
PC=$(req $PYV2 POST /receipts/return "$RETCARD"); PB=$(get_body)
cmp_resp "return card без rrn → 422" "$RC" "$PC" "$RB" "$PB"

# ═══ ЧЕКИ v2: GET (спільна БД → однакові) ═══
RC=$(req $RUSTV2 GET /receipts/$RID -); RB=$(get_body)
PC=$(req $PYV2 GET /receipts/$RID -); PB=$(get_body)
cmp_resp "GET /receipts/{id}" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUSTV2 GET /receipts/$RID/items -); RB=$(get_body)
PC=$(req $PYV2 GET /receipts/$RID/items -); PB=$(get_body)
cmp_resp "GET /receipts/{id}/items" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUSTV2 GET "/receipts?page=1&size=5" -); RB=$(get_body)
PC=$(req $PYV2 GET "/receipts?page=1&size=5" -); PB=$(get_body)
cmp_resp "GET /receipts (list)" "$RC" "$PC" "$RB" "$PB"
# stats/today — інформативно: спільна жива БД (активна копія nastya) робить
# глобальну статистику нестабільною між запитами. Детермінована валідація —
# у cargo-тесті today_stats_delta (ізольована дельта).
P1=$(req $PYV2 GET /receipts/stats/today -); PB1=$(get_body)
RC=$(req $RUSTV2 GET /receipts/stats/today -); RB=$(get_body)
if [ "$RC" == "200" ] && [ "$P1" == "200" ]; then
  echo "ℹ️ GET /receipts/stats/today: обидва 200 (детальна звірка — у cargo-тесті)"
else
  echo "❌ GET /receipts/stats/today: Rust=$RC Python=$P1"; FAIL=1
fi
SQ=$(python3 -c "import urllib.parse;print(urllib.parse.quote('E3-ПРОДУКТ-A-$TS'))")
RC=$(req $RUSTV2 GET "/receipts/search?q=$SQ" -); RB=$(get_body)
PC=$(req $PYV2 GET "/receipts/search?q=$SQ" -); PB=$(get_body)
cmp_resp "GET /receipts/search" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUSTV2 GET "/receipts/by-product/E3P-$TS-A/recent-sales?limit=5" -); RB=$(get_body)
PC=$(req $PYV2 GET "/receipts/by-product/E3P-$TS-A/recent-sales?limit=5" -); PB=$(get_body)
cmp_resp "GET /receipts/by-product/recent-sales" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUSTV2 GET "/receipts/products/$PA/returnable-quantity" -); RB=$(get_body)
PC=$(req $PYV2 GET "/receipts/products/$PA/returnable-quantity" -); PB=$(get_body)
cmp_resp "GET returnable-quantity" "$RC" "$PC" "$RB" "$PB"
UUID=$(python3 -c "import uuid;print(uuid.uuid4())")
RC=$(req $RUSTV2 GET /receipts/$UUID -); RB=$(get_body)
PC=$(req $PYV2 GET /receipts/$UUID -); PB=$(get_body)
cmp_resp "GET /receipts/{uuid} → 404" "$RC" "$PC" "$RB" "$PB"

# ═══ ТРАНЗАКЦІЙНІСТЬ: помилка в середині → нічого не записано ═══
CNT_BEFORE=$(curl -s "$PYV2/receipts?page=1&size=100" -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['total'])")
TXN="{\"items\":[{\"product_id\":\"$PA\",\"quantity\":1,\"price\":100},{\"product_id\":\"$PB\",\"quantity\":1,\"price\":100}],\"payment_method\":\"cash\",\"cash_amount\":200}"
STOCK_BEFORE=$(curl -s $RUST/products/$PA -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
RC=$(req $RUSTV2 POST /receipts/sale "$TXN"); RB=$(get_body)
CNT_AFTER=$(curl -s "$PYV2/receipts?page=1&size=100" -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['total'])")
STOCK_AFTER=$(curl -s $RUST/products/$PA -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
if [ "$RC" == "400" ] && [ "$CNT_BEFORE" == "$CNT_AFTER" ] && [ "$STOCK_BEFORE" == "$STOCK_AFTER" ]; then
  echo "✅ ТРАНЗАКЦІЙНІСТЬ: 400 у середині → чек не створено, stock не змінено ($STOCK_BEFORE)"
else
  echo "❌ ТРАНЗАКЦІЙНІСТЬ: RC=$RC count $CNT_BEFORE→$CNT_AFTER stock $STOCK_BEFORE→$STOCK_AFTER"; FAIL=1
fi

# ═══ КОНКУРЕНТНІСТЬ: 2 паралельні sale, один товар ═══
mk_prod2() {
  curl -s -X POST $PY/products -H "$AUTH" -H "$CT" \
    -d "{\"barcode\":\"E3C-$TS\",\"title\":\"E3-КОНКУР-$TS\",\"price\":10,\"cost_price\":5,\"stock\":100,\"tax_rate\":20}" | jid id
}
CP=$(mk_prod2)
CB="{\"items\":[{\"product_id\":\"$CP\",\"quantity\":7,\"price\":10}],\"payment_method\":\"cash\",\"cash_amount\":70}"
curl -s -X POST $RUSTV2/receipts/sale -H "$AUTH" -H "$CT" -d "$CB" -o /dev/null -w "sale A: %{http_code}\n" &
curl -s -X POST $RUSTV2/receipts/sale -H "$AUTH" -H "$CT" -d "$CB" -o /dev/null -w "sale B: %{http_code}\n" &
wait
STOCK_C=$(curl -s $RUST/products/$CP -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
if [ "$STOCK_C" == "86.000" ]; then
  echo "✅ КОНКУРЕНТНІСТЬ: 2 паралельні sale → stock=$STOCK_C (100-7-7), нуль втрат"
else
  echo "❌ КОНКУРЕНТНІСТЬ: stock=$STOCK_C, очікувано 86.000"; FAIL=1
fi

# ═══ РОБОЧІ СЕСІЇ ═══
USERID=$(python3 -c "import base64,json,os; p=os.environ['TOKEN'].split('.')[1]; p+='='*(-len(p)%4); print(json.loads(base64.urlsafe_b64decode(p))['sub'])" 2>/dev/null || echo "dded5c75-d093-4ab9-9a1c-bd61a35d2816")
RC=$(req $RUST GET "/work-sessions/my?month=8&year=2026" -); RB=$(get_body)
PC=$(req $PY GET "/work-sessions/my?month=8&year=2026" -); PB=$(get_body)
cmp_resp "GET /work-sessions/my" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET "/work-sessions/report?month=8&year=2026" -); RB=$(get_body)
PC=$(req $PY GET "/work-sessions/report?month=8&year=2026" -); PB=$(get_body)
cmp_resp "GET /work-sessions/report" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET "/work-sessions/user/$USERID?month=8&year=2026" -); RB=$(get_body)
PC=$(req $PY GET "/work-sessions/user/$USERID?month=8&year=2026" -); PB=$(get_body)
cmp_resp "GET /work-sessions/user/{id}" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET "/work-sessions/user/$UUID?month=8&year=2026" -); RB=$(get_body)
PC=$(req $PY GET "/work-sessions/user/$UUID?month=8&year=2026" -); PB=$(get_body)
cmp_resp "GET /work-sessions/user/{uuid} → 404" "$RC" "$PC" "$RB" "$PB"

# ═══ СПИСАННЯ ═══
WO_BODY="{\"reason\":\"expired\",\"write_off_date\":\"2026-08-07T10:00:00\",\"notes\":\"diff\",\"items\":[{\"product_id\":\"$PA\",\"quantity\":1}]}"
RC=$(req $RUST POST /write-offs "$WO_BODY"); RB=$(get_body); WID=$(echo "$RB" | jid id)
PC=$(req $PY POST /write-offs "$WO_BODY"); PB=$(get_body); WIDP=$(echo "$PB" | jid id)
cmp_resp "POST /write-offs (авто-confirm)" "$RC" "$PC" "$RB" "$PB"
STOCK_W=$(curl -s $RUST/products/$PA -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
echo "  stock після 2 списань: $STOCK_W (очікувано ~95.000)"
RC=$(req $RUST GET /write-offs/$WID -); RB=$(get_body)
PC=$(req $PY GET /write-offs/$WID -); PB=$(get_body)
cmp_resp "GET /write-offs/{id} (спільна БД)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET /write-offs/$WIDP -); RB=$(get_body)
PC=$(req $PY GET /write-offs/$WIDP -); PB=$(get_body)
cmp_resp "GET /write-offs/{id} Python-документ" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET "/write-offs?page=1&size=10" -); RB=$(get_body)
PC=$(req $PY GET "/write-offs?page=1&size=10" -); PB=$(get_body)
cmp_resp "GET /write-offs (list)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET /write-offs/$UUID -); RB=$(get_body)
PC=$(req $PY GET /write-offs/$UUID -); PB=$(get_body)
cmp_resp "GET /write-offs/{uuid} → 404" "$RC" "$PC" "$RB" "$PB"
WO_UP="{\"notes\":\"оновлено-diff\"}"
RC=$(req $RUST PUT /write-offs/$WID "$WO_UP"); RB=$(get_body)
PC=$(req $PY PUT /write-offs/$WIDP "$WO_UP"); PB=$(get_body)
cmp_resp "PUT /write-offs/{id}" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST POST /write-offs/$WID/confirm -); RB=$(get_body)
PC=$(req $PY POST /write-offs/$WIDP/confirm -); PB=$(get_body)
cmp_resp "POST /write-offs/{id}/confirm (повторно)" "$RC" "$PC" "$RB" "$PB"

# ═══ ПЕРЕМІЩЕННЯ ═══
TR_BODY="{\"from_location\":\"Склад-1\",\"to_location\":\"Склад-2\",\"transfer_date\":\"2026-08-07T10:00:00\",\"notes\":\"diff\",\"items\":[{\"product_id\":\"$PA\",\"quantity\":1}]}"
RC=$(req $RUST POST /transfers "$TR_BODY"); RB=$(get_body); TID=$(echo "$RB" | jid id)
PC=$(req $PY POST /transfers "$TR_BODY"); PB=$(get_body); TIDP=$(echo "$PB" | jid id)
cmp_resp "POST /transfers (draft)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET /transfers/$TID -); RB=$(get_body)
PC=$(req $PY GET /transfers/$TID -); PB=$(get_body)
cmp_resp "GET /transfers/{id} (спільна БД)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET /transfers/$UUID -); RB=$(get_body)
PC=$(req $PY GET /transfers/$UUID -); PB=$(get_body)
cmp_resp "GET /transfers/{uuid} → 404" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST PUT /transfers/$TID '{"notes":"x"}'); RB=$(get_body)
PC=$(req $PY PUT /transfers/$TIDP '{"notes":"x"}'); PB=$(get_body)
cmp_resp "PUT /transfers (draft)" "$RC" "$PC" "$RB" "$PB"
CONF='{"status":"confirmed"}'
RC=$(req $RUST POST /transfers/$TID/confirm "$CONF"); RB=$(get_body)
PC=$(req $PY POST /transfers/$TIDP/confirm "$CONF"); PB=$(get_body)
cmp_resp "POST /transfers confirm → confirmed" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST POST /transfers/$TID/confirm "$CONF"); RB=$(get_body)
PC=$(req $PY POST /transfers/$TIDP/confirm "$CONF"); PB=$(get_body)
cmp_resp "Повторний confirm → 400" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST PUT /transfers/$TID '{"notes":"y"}'); RB=$(get_body)
PC=$(req $PY PUT /transfers/$TIDP '{"notes":"y"}'); PB=$(get_body)
cmp_resp "PUT confirmed → 400 (тільки чернетки)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST DELETE /transfers/$TID -); RB=$(get_body)
PC=$(req $PY DELETE /transfers/$TIDP -); PB=$(get_body)
cmp_resp "DELETE confirmed → 400" "$RC" "$PC" "$RB" "$PB"
CANCEL='{"status":"cancelled"}'
RC=$(req $RUST POST /transfers/$TID/confirm "$CANCEL"); RB=$(get_body)
PC=$(req $PY POST /transfers/$TIDP/confirm "$CANCEL"); PB=$(get_body)
cmp_resp "confirm cancelled (відкат stock)" "$RC" "$PC" "$RB" "$PB"
BADSTAT='{"status":"draft"}'
RC=$(req $RUST POST /transfers/$TID/confirm "$BADSTAT"); RB=$(get_body)
PC=$(req $PY POST /transfers/$TIDP/confirm "$BADSTAT"); PB=$(get_body)
cmp_resp "confirm 'draft' → 400" "$RC" "$PC" "$RB" "$PB"

# ═══ ЗМІНИ ПРРО (X/Z) ═══
RC=$(req $RUSTV2 GET "/prro/shifts?page=1&size=5" -); RB=$(get_body)
PC=$(req $PYV2 GET "/prro/shifts?page=1&size=5" -); PB=$(get_body)
cmp_resp "GET /prro/shifts (спільна БД)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUSTV2 POST /prro/shift/open '{}'); RB=$(get_body)
PC=$(req $PYV2 POST /prro/shift/open '{}'); PB=$(get_body)
cmp_resp "POST /prro/shift/open (без ПРРО → 400)" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUSTV2 POST /prro/shift/close '{}'); RB=$(get_body)
PC=$(req $PYV2 POST /prro/shift/close '{}'); PB=$(get_body)
cmp_resp "POST /prro/shift/close (немає відкритої → 400)" "$RC" "$PC" "$RB" "$PB"

# ═══ ОЧИЩЕННЯ (тестові дані; реальні не чіпаємо) ═══
export PGPASSWORD=VgxWd7MBJ10X
for pid in $PA $PBID $CP; do
  curl -s -X PUT $RUST/products/$pid -H "$AUTH" -H "$CT" -d '{"stock":0}' >/dev/null
  curl -s -X DELETE $RUST/products/$pid -H "$AUTH" >/dev/null
done
# receipts/write_offs/transfers тестові — прибрати напряму (API не видаляє чеки)
psql -h localhost -U postgres -d pos_system -q -c "DELETE FROM receipt_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%'); DELETE FROM receipt_items WHERE receipt_id IN (SELECT id FROM receipts WHERE receipt_number LIKE 'RCPT-%' AND id IN (SELECT receipt_id FROM receipt_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%'))); DELETE FROM receipts WHERE id IN (SELECT receipt_id FROM receipt_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%')); DELETE FROM write_off_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%'); DELETE FROM write_offs WHERE id IN (SELECT write_off_id FROM write_off_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%')); DELETE FROM transfer_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%'); DELETE FROM transfers WHERE id IN (SELECT transfer_id FROM transfer_items WHERE product_id IN (SELECT id FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%')); DELETE FROM products WHERE barcode LIKE 'E3%' OR title LIKE 'ТЕСТ-POS%'; DELETE FROM receipts WHERE receipt_number LIKE 'RCPT-%' AND NOT EXISTS (SELECT 1 FROM receipt_items ri WHERE ri.receipt_id=receipts.id);" 2>/dev/null
echo "🧹 тестові дані видалено"

echo "===================="
[ $FAIL -eq 0 ] && echo "E2E DIFFERENTIAL POS v1: ВСІ ПРОЙДЕНО" || echo "E2E DIFFERENTIAL POS v1: Є РОЗБІЖНОСТІ"
exit $FAIL
