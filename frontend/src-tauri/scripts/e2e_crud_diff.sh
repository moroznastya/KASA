#!/usr/bin/env bash
# E2E differential CRUD v3: Rust-фасад (:8002) vs Python (:8001), СПІЛЬНА БД.
# Create-порівняння — послідовно: Rust create → delete → Python create (той самий
# barcode/name) → порівняти нормалізовані тіла. Stock перевіряється через GET.
set -u
RUST=http://127.0.0.1:8002/api/v1
PY=http://127.0.0.1:8001/api/v1
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
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at','inventory_id','number')}
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
    echo "  Rust:   $rn" | head -c 800; echo
    echo "  Python: $pn" | head -c 800; echo
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

# ═══ CATEGORIES ═══
BODY="{\"name\":\"E2E-КАТ-$TS\",\"description\":\"диференціал\"}"
RC=$(req $RUST POST /categories "$BODY"); RB=$(get_body)
req $RUST DELETE /categories/$(echo "$RB" | jid id) - >/dev/null
PC=$(req $PY POST /categories "$BODY"); PB=$(get_body)
cmp_resp "POST /categories" "$RC" "$PC" "$RB" "$PB"
req $PY DELETE /categories/$(echo "$PB" | jid id) - >/dev/null

# ═══ SUPPLIERS ═══
BODY="{\"name\":\"E2E-ПОСТАЧ-$TS\",\"edrpou\":\"99999999\",\"phone\":\"+380671112233\",\"email\":\"e2e@t.ua\",\"address\":\"Київ\",\"notes\":\"diff\"}"
RC=$(req $RUST POST /suppliers "$BODY"); RB=$(get_body)
req $RUST DELETE /suppliers/$(echo "$RB" | jid id) - >/dev/null
PC=$(req $PY POST /suppliers "$BODY"); PB=$(get_body)
cmp_resp "POST /suppliers" "$RC" "$PC" "$RB" "$PB"
req $PY DELETE /suppliers/$(echo "$PB" | jid id) - >/dev/null

# ═══ PRODUCTS: Rust create→put→get→400→delete; Python create→put→get→400→delete ═══
BODY="{\"barcode\":\"$TS-0001\",\"sku\":\"E2E-SKU-$TS\",\"title\":\"E2E-ТОВАР-$TS\",\"description\":\"diff\",\"price\":142.7,\"cost_price\":87.23,\"stock\":16,\"recommended_qty\":5,\"uktzed\":\"4820\",\"scan_excise\":false,\"tax_rate\":20,\"tax_group\":\"А\",\"is_weight\":false,\"unit\":\"шт\"}"
BODY2="{\"title\":\"E2E-ТОВАР-$TS-ОНОВЛЕНО\",\"price\":150}"

RC=$(req $RUST POST /products "$BODY"); RB=$(get_body); PID_R=$(echo "$RB" | jid id)
req $RUST PUT /products/$PID_R "$BODY2" >/dev/null; RB2=$(get_body)
req $RUST GET /products/$PID_R - >/dev/null; RB3=$(get_body)
req $RUST DELETE /products/$PID_R - >/dev/null; RB4=$(get_body)
req $RUST PUT /products/$PID_R '{"stock":0}' >/dev/null
req $RUST DELETE /products/$PID_R - >/dev/null

PC=$(req $PY POST /products "$BODY"); PB=$(get_body); PID_P=$(echo "$PB" | jid id)
req $PY PUT /products/$PID_P "$BODY2" >/dev/null; PB2=$(get_body)
req $PY GET /products/$PID_P - >/dev/null; PB3=$(get_body)
req $PY DELETE /products/$PID_P - >/dev/null; PB4=$(get_body)
req $PY PUT /products/$PID_P '{"stock":0}' >/dev/null
req $PY DELETE /products/$PID_P - >/dev/null

cmp_resp "POST /products create (вхідна scale)" "201" "201" "$RB" "$PB"
cmp_resp "PUT /products (markup перерахунок)" "200" "200" "$RB2" "$PB2"
cmp_resp "GET /products/{id} (scale БД)" "200" "200" "$RB3" "$PB3"
cmp_resp "DELETE stock≠0 → 400" "400" "400" "$RB4" "$PB4"

# ═══ Конфлікт barcode (Rust створив, Python намагається) ═══
req $RUST POST /products "$BODY" >/dev/null; PID_R=$(get_body | jid id)
PC=$(req $PY POST /products "$BODY"); PB=$(get_body)
RC=$(req $RUST POST /products "$BODY"); RB=$(get_body)
cmp_resp "Дубль barcode → 409" "$RC" "$PC" "$RB" "$PB"
req $RUST PUT /products/$PID_R '{"stock":0}' >/dev/null
req $RUST DELETE /products/$PID_R - >/dev/null

# ═══ 404 / 422 ═══
UUID=$(python3 -c "import uuid;print(uuid.uuid4())")
RC=$(req $RUST GET /products/$UUID -); RB=$(get_body)
PC=$(req $PY GET /products/$UUID -); PB=$(get_body)
cmp_resp "GET /products/{uuid} → 404" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST GET /products/not-a-uuid -); PC=$(req $PY GET /products/not-a-uuid -)
[ "$RC" == "422" ] && [ "$PC" == "422" ] && echo "✅ 422 невалідний UUID path: Rust=$RC Python=$PC" || { echo "❌ 422 uuid: Rust=$RC Python=$PC"; FAIL=1; }
LONG=$(python3 -c "print('A'*256)")
RC=$(req $RUST POST /products "{\"title\":\"$LONG\"}"); PC=$(req $PY POST /products "{\"title\":\"$LONG\"}")
[ "$RC" == "422" ] && [ "$PC" == "422" ] && echo "✅ 422 title>255: Rust=$RC Python=$PC" || { echo "❌ 422 title: Rust=$RC Python=$PC"; FAIL=1; }

# ═══ INVENTORY ═══
BODY="{\"barcode\":\"$TS-INV\",\"title\":\"E2E-ІНВЕНТАР-$TS\",\"price\":100,\"cost_price\":50,\"stock\":10}"
req $RUST POST /products "$BODY" >/dev/null; PID=$(get_body | jid id)
INV_BODY="{\"location\":\"E2E-СКЛАД-$TS\",\"inventory_date\":\"2026-08-07T10:00:00\",\"notes\":\"diff\",\"items\":[{\"product_id\":\"$PID\",\"actual_quantity\":12.5,\"accounting_quantity\":10,\"difference\":2.5,\"cost_price\":50,\"price\":100}]}"
RC=$(req $RUST POST /inventory "$INV_BODY"); RB=$(get_body)
PC=$(req $PY POST /inventory "$INV_BODY"); PB=$(get_body)
cmp_resp "POST /inventory (вхідні scale, summary)" "$RC" "$PC" "$RB" "$PB"
INV_R=$(echo "$RB" | jid id); INV_P=$(echo "$PB" | jid id)

RC=$(req $RUST POST /inventory/$INV_R/confirm '{"status":"confirmed"}'); RB=$(get_body)
PC=$(req $PY POST /inventory/$INV_P/confirm '{"status":"confirmed"}'); PB=$(get_body)
cmp_resp "POST /inventory confirm" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST POST /inventory/$INV_R/confirm '{"status":"confirmed"}'); RB=$(get_body)
PC=$(req $PY POST /inventory/$INV_P/confirm '{"status":"confirmed"}'); PB=$(get_body)
cmp_resp "Повторний confirm → 400" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST POST /inventory/$INV_R/confirm '{"status":"draft"}'); RB=$(get_body)
PC=$(req $PY POST /inventory/$INV_P/confirm '{"status":"draft"}'); PB=$(get_body)
cmp_resp "confirm 'draft' → 400" "$RC" "$PC" "$RB" "$PB"
RC=$(req $RUST DELETE /inventory/$INV_R -); RB=$(get_body)
PC=$(req $PY DELETE /inventory/$INV_P -); PB=$(get_body)
cmp_resp "DELETE /inventory confirmed → 400" "$RC" "$PC" "$RB" "$PB"
STOCK=$(curl -s $RUST/products/$PID -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
echo "stock після confirm: $STOCK (очікувано 12.500)"

# ═══ КОНКУРЕНТНІСТЬ: 2 паралельні confirm, один товар ═══
BODY="{\"barcode\":\"$TS-CONC\",\"title\":\"E2E-КОНКУР-$TS\",\"price\":10,\"cost_price\":5,\"stock\":100}"
req $RUST POST /products "$BODY" >/dev/null; CPID=$(get_body | jid id)
mk_inv() {
  curl -s -X POST $RUST/inventory -H "$AUTH" -H "$CT" -d "{\"location\":\"КОНКУР\",\"inventory_date\":\"2026-08-07T10:00:00\",\"items\":[{\"product_id\":\"$CPID\",\"actual_quantity\":0,\"accounting_quantity\":0,\"difference\":$1,\"cost_price\":5,\"price\":10}]}" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])"
}
IA=$(mk_inv 7); IB=$(mk_inv -3)
curl -s -X POST $RUST/inventory/$IA/confirm -H "$AUTH" -H "$CT" -d '{"status":"confirmed"}' -o /dev/null -w "confirm A: %{http_code}\n" &
curl -s -X POST $RUST/inventory/$IB/confirm -H "$AUTH" -H "$CT" -d '{"status":"confirmed"}' -o /dev/null -w "confirm B: %{http_code}\n" &
wait
STOCK2=$(curl -s $RUST/products/$CPID -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin)['stock'])")
if [ "$STOCK2" == "104.000" ]; then
  echo "✅ КОНКУРЕНТНІСТЬ: 2 паралельні confirm → stock=$STOCK2 (100+7-3), нуль втрат"
else
  echo "❌ КОНКУРЕНТНІСТЬ: stock=$STOCK2, очікувано 104.000"; FAIL=1
fi

echo "===================="
[ $FAIL -eq 0 ] && echo "E2E DIFFERENTIAL CRUD v3: ВСІ ПРОЙДЕНО" || echo "E2E DIFFERENTIAL CRUD v3: Є РОЗБІЖНОСТІ"
exit $FAIL
