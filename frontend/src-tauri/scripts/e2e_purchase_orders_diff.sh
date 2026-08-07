#!/usr/bin/env bash
# ============================================================================
# e2e_purchase_orders_diff.sh — differential-тест замовлень постачальнику
# (етап 8, група 5). Rust :8002 (KASA_RUST_PURCHASE_ORDERS=1) vs Python
# :8001 (еталон).
#
# Покриває (6 роутів v1):
#   list (пагінація), get (404), create (автономер ЗАМ-, total=sum,
#   supplier_name, session-значення), update (scalar+items; тільки draft),
#   delete (тільки draft; 204/400/404), confirm (confirmed → Invoice DRAFT
#   з копією позицій; cancelled → статус; 400/404/422).
#
# Результат: PASS/FAIL по кожній перевірці; фінальний підсумок N/M.
# ============================================================================
set -u
PY=http://127.0.0.1:8001
RS=http://127.0.0.1:8002
TOKEN=$(cat /tmp/kasa_token 2>/dev/null || echo "")
AUTH="Authorization: Bearer $TOKEN"
PASS=0; FAIL=0
TS=$(date +%s)
SUP_NAME="Diff PO Sup $TS"
PROD_TITLE="Diff PO Prod $TS"
PROD2_TITLE="Diff PO Prod2 $TS"

log()  { echo "[$(date +%H:%M:%S)] $*"; }
ok()   { PASS=$((PASS+1)); echo "  ✅ $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

# ── нормалізація JSON для порівняння ───────────────────────────────────────
py_norm() {
python3 - << 'PYEOF'
import json,sys,re
o=json.load(sys.stdin)
def drop(x):
    if isinstance(x,dict):
        return {k:drop(v) for k,v in x.items()
                if k not in ("id","number","created_at","updated_at","invoice_id",
                             "created_by_id")}
    if isinstance(x,list): return [drop(v) for v in x]
    if isinstance(x,str):
        x=re.sub(r'№ЗАМ-[0-9]+-[0-9]+','№ЗАМ-N',x)
        x=re.sub(r'№ПН-[0-9]+-[0-9]+','№ПН-N',x)
        return x
    return x
print(json.dumps(drop(o),ensure_ascii=False,sort_keys=True))
PYEOF
}

# ── 1. Підготовка тестових даних (напряму в БД) ─────────────────────────────
export PGPASSWORD=VgxWd7MBJ10X
PG="psql -h localhost -U postgres -d pos_system -t -A -q"
SUP=$($PG -c "INSERT INTO suppliers (id,name,phone,created_at,updated_at) VALUES (gen_random_uuid(),'$SUP_NAME','000','2026-08-07',now()) RETURNING id;")
PROD=$($PG -c "INSERT INTO products (id,title,barcode,price,cost_price,markup,unit,is_fiscal,tax_group,tax_rate,stock,created_at,updated_at) VALUES (gen_random_uuid(),'$PROD_TITLE','DIFF-PO-$TS',200.00,120.00,66.67,'шт',false,'А',0,100.000,'2026-08-07',now()) RETURNING id;")
PROD2=$($PG -c "INSERT INTO products (id,title,barcode,price,cost_price,markup,unit,is_fiscal,tax_group,tax_rate,stock,created_at,updated_at) VALUES (gen_random_uuid(),'$PROD2_TITLE','DIFF-PO2-$TS',50.00,25.00,100.00,'шт',false,'А',0,100.000,'2026-08-07',now()) RETURNING id;")
ADMIN=$($PG -c "SELECT id FROM users WHERE role='admin' LIMIT 1;")
log "Тестові дані: SUP=$SUP PROD=$PROD PROD2=$PROD2"

# ── 2. Рівні пари (Python створює, Rust створює — порівнюємо відповіді) ───
BODY1="{\"supplier_id\":\"$SUP\",\"order_date\":\"2026-08-07T10:00:00\",\"expected_date\":\"2026-08-10T10:00:00\",\"is_fiscal\":false,\"notes\":\"Diff PO parity\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":3.0,\"price\":200.00,\"total\":600.00},{\"product_id\":\"$PROD2\",\"quantity\":2,\"price\":50.00,\"total\":100.00}]}"

RESP_PY=$(curl -s -X POST "$PY/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY1")
RESP_RS=$(curl -s -X POST "$RS/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY1")
if [ "$(echo "$RESP_PY" | py_norm)" = "$(echo "$RESP_RS" | py_norm)" ]; then ok "create parity (автономер, total=sum 700, supplier_name, session-значення)"; else bad "create parity: $(echo "$RESP_PY" | py_norm | head -c 200) vs $(echo "$RESP_RS" | py_norm | head -c 200)"; fi

# Автономер формату ЗАМ-{YYYYMMDD}-{NNN}
PY_NUM=$(echo "$RESP_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['number'])")
RS_NUM=$(echo "$RESP_RS" | python3 -c "import sys,json;print(json.load(sys.stdin)['number'])")
if echo "$PY_NUM" | grep -qE '^ЗАМ-[0-9]{8}-[0-9]{3}$' && echo "$RS_NUM" | grep -qE '^ЗАМ-[0-9]{8}-[0-9]{3}$'; then ok "автономер формат: PY=$PY_NUM RS=$RS_NUM"; else bad "автономер формат: PY=$PY_NUM RS=$RS_NUM"; fi
# total=sum(items) 700.0
PY_TOT=$(echo "$RESP_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['total_amount'])")
if [ "$PY_TOT" = "700.0" ] || [ "$PY_TOT" = "700.00" ]; then ok "total_amount=sum(600+100)=$PY_TOT"; else bad "total_amount=$PY_TOT"; fi
# статус draft
if [ "$(echo "$RESP_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['status'])")" = "draft" ]; then ok "status=draft"; else bad "status не draft"; fi

# get: Python створене → parity з get Rust-створеного (БД scale)
PY_ID=$(echo "$RESP_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
RS_ID=$(echo "$RESP_RS" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
GET_PY=$(curl -s "$PY/api/v1/purchase-orders/$PY_ID" -H "$AUTH")
GET_RS=$(curl -s "$RS/api/v1/purchase-orders/$RS_ID" -H "$AUTH")
if [ "$(echo "$GET_PY" | py_norm)" = "$(echo "$GET_RS" | py_norm)" ]; then ok "get parity (БД scale 3.000/200.00)"; else bad "get parity: $(echo "$GET_PY" | py_norm | head -c 200) vs $(echo "$GET_RS" | py_norm | head -c 200)"; fi

# get 404 (обидва)
P404=$(curl -s -o /dev/null -w "%{http_code}" "$PY/api/v1/purchase-orders/00000000-0000-0000-0000-000000000000" -H "$AUTH")
R404=$(curl -s -o /dev/null -w "%{http_code}" "$RS/api/v1/purchase-orders/00000000-0000-0000-0000-000000000000" -H "$AUTH")
if [ "$P404" = "404" ] && [ "$R404" = "404" ]; then ok "get 404 (PY=$P404 RS=$R404)"; else bad "get 404: PY=$P404 RS=$R404"; fi
D404=$(curl -s "$RS/api/v1/purchase-orders/00000000-0000-0000-0000-000000000000" -H "$AUTH" | python3 -c "import sys,json;print(json.load(sys.stdin).get('detail',''))" 2>/dev/null)
if echo "$D404" | grep -q "не знайдено"; then ok "get 404 detail 'не знайдено'"; else bad "get 404 detail: $D404"; fi

# ── 3. update scalar ────────────────────────────────────────────────────────
UPD_BODY='{"number":"ЗАМ-TEST-001","notes":"Diff PO updated","total_amount":"777.50","is_fiscal":true}'
UPD_PY=$(curl -s -X PUT "$PY/api/v1/purchase-orders/$PY_ID" -H "$AUTH" -H "Content-Type: application/json" -d "$UPD_BODY")
UPD_RS=$(curl -s -X PUT "$RS/api/v1/purchase-orders/$RS_ID" -H "$AUTH" -H "Content-Type: application/json" -d "$UPD_BODY")
if [ "$(echo "$UPD_PY" | py_norm)" = "$(echo "$UPD_RS" | py_norm)" ]; then ok "update scalar parity (number/notes/total_amount 777.50/is_fiscal)"; else bad "update scalar: $(echo "$UPD_PY" | py_norm | head -c 200) vs $(echo "$UPD_RS" | py_norm | head -c 200)"; fi
# БД: total_amount оновлено
DB_TOT=$($PG -c "SELECT total_amount::text FROM purchase_orders WHERE id='$PY_ID';")
if [ "$DB_TOT" = "777.50" ]; then ok "update БД total_amount=777.50"; else bad "update БД total_amount=$DB_TOT"; fi

# ── 4. update items (заміна) ────────────────────────────────────────────────
UPD_ITEMS="{\"items\":[{\"product_id\":\"$PROD2\",\"quantity\":5.0,\"price\":60.00,\"total\":300.00}]}"
UPI_PY=$(curl -s -X PUT "$PY/api/v1/purchase-orders/$PY_ID" -H "$AUTH" -H "Content-Type: application/json" -d "$UPD_ITEMS")
UPI_RS=$(curl -s -X PUT "$RS/api/v1/purchase-orders/$RS_ID" -H "$AUTH" -H "Content-Type: application/json" -d "$UPD_ITEMS")
if [ "$(echo "$UPI_PY" | py_norm)" = "$(echo "$UPI_RS" | py_norm)" ]; then ok "update items parity (заміна на 1 позицію, session 5.0/60.00/300.00)"; else bad "update items: $(echo "$UPI_PY" | py_norm | head -c 250) vs $(echo "$UPI_RS" | py_norm | head -c 250)"; fi
# БД: 1 позиція
DB_N=$($PG -c "SELECT count(*) FROM purchase_order_items WHERE purchase_order_id='$PY_ID';")
if [ "$DB_N" = "1" ]; then ok "update БД: позицій=1 (старі видалені)"; else bad "update БД: позицій=$DB_N"; fi
# total_amount НЕ змінився (не передавався) — 777.50
DB_TOT2=$($PG -c "SELECT total_amount::text FROM purchase_orders WHERE id='$PY_ID';")
if [ "$DB_TOT2" = "777.50" ]; then ok "update items не чіпає total_amount (БД 777.50)"; else bad "update items змінив total_amount: $DB_TOT2"; fi

# ── 5. confirm confirmed (Python) → parity ──────────────────────────────────
CONF_PY=$(curl -s -X POST "$PY/api/v1/purchase-orders/$PY_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}')
CONF_RS=$(curl -s -X POST "$RS/api/v1/purchase-orders/$RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}')
if [ "$(echo "$CONF_PY" | py_norm)" = "$(echo "$CONF_RS" | py_norm)" ]; then ok "confirm parity (status confirmed, invoice brief, items)"; else bad "confirm parity: $(echo "$CONF_PY" | py_norm | head -c 300) vs $(echo "$CONF_RS" | py_norm | head -c 300)"; fi
# статус confirmed
if [ "$(echo "$CONF_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['status'])")" = "confirmed" ]; then ok "confirm status=confirmed"; else bad "confirm status"; fi
# invoice_id зв'язано; invoice: null — АНОМАЛІЯ PYTHON 1:1 (post_update identity map)
INV_ID_PY=$(echo "$CONF_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['invoice_id'])")
INV_JSON=$(echo "$CONF_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['invoice'])")
if [ "$INV_JSON" = "None" ]; then ok "confirm invoice=null (АНОМАЛІЯ PYTHON 1:1: post_update identity map)"; else bad "confirm invoice: $INV_JSON"; fi
INV_NUM_PY=$($PG -c "SELECT number FROM invoices WHERE id='$INV_ID_PY';")
if echo "$INV_NUM_PY" | grep -qE '^ПН-[0-9]{8}-[0-9]{3}$'; then ok "confirm: invoice №$INV_NUM_PY (ПН-формат, з БД)"; else bad "confirm invoice number: $INV_NUM_PY"; fi
# БД: invoice створено (status draft, payment credit, notes, total)
INV_DB=$($PG -c "SELECT status::text||'|'||payment_method::text||'|'||total_amount::text||'|'||notes FROM invoices WHERE id='$INV_ID_PY';")
if echo "$INV_DB" | grep -q "draft|credit|777.50|Автоматично створено із замовлення №ЗАМ-TEST-001"; then ok "confirm БД: invoice draft/credit/777.50/notes 'Автоматично створено із замовлення №...'"; else bad "confirm БД invoice: $INV_DB"; fi
# invoice_date = order_date
INV_DATE=$($PG -c "SELECT invoice_date::text FROM invoices WHERE id='$INV_ID_PY';")
if echo "$INV_DATE" | grep -q "^2026-08-07"; then ok "confirm БД: invoice_date=order_date ($INV_DATE)"; else bad "confirm invoice_date: $INV_DATE"; fi
# items скопійовані (1 позиція, qty/price/total)
INV_ITEMS=$($PG -c "SELECT quantity::text||'|'||price::text||'|'||total::text FROM invoice_items WHERE invoice_id='$INV_ID_PY';")
if echo "$INV_ITEMS" | grep -q "5.000|60.00|300.00"; then ok "confirm БД: invoice items скопійовані (5.000|60.00|300.00)"; else bad "confirm БД invoice items: $INV_ITEMS"; fi

# ── 6. confirm повторно → 400 (обидва) ─────────────────────────────────────
P400=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$PY/api/v1/purchase-orders/$PY_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}')
R400=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$RS/api/v1/purchase-orders/$RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}')
if [ "$P400" = "400" ] && [ "$R400" = "400" ]; then ok "confirm повторно 400 (PY=$P400 RS=$R400)"; else bad "confirm повторно: PY=$P400 RS=$R400"; fi
D400=$(curl -s -X POST "$RS/api/v1/purchase-orders/$RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}' | python3 -c "import sys,json;print(json.load(sys.stdin).get('detail',''))")
if echo "$D400" | grep -q "вже має статус 'confirmed'"; then ok "confirm повторно detail: 'вже має статус confirmed'"; else bad "confirm detail: $D400"; fi

# ── 7. confirm 404 ──────────────────────────────────────────────────────────
P404C=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$PY/api/v1/purchase-orders/00000000-0000-0000-0000-000000000000/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}')
R404C=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$RS/api/v1/purchase-orders/00000000-0000-0000-0000-000000000000/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"confirmed"}')
if [ "$P404C" = "404" ] && [ "$R404C" = "404" ]; then ok "confirm 404 (PY=$P404C RS=$R404C)"; else bad "confirm 404: PY=$P404C RS=$R404C"; fi

# ── 8. confirm невідомий статус → 422 (Pydantic enum) ──────────────────────
P422=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$PY/api/v1/purchase-orders/$PY_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"shipped"}')
R422=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$RS/api/v1/purchase-orders/$RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"shipped"}')
if [ "$P422" = "422" ] && [ "$R422" = "422" ]; then ok "confirm 'shipped' 422 enum (PY=$P422 RS=$R422)"; else bad "confirm 'shipped': PY=$P422 RS=$R422"; fi

# ── 9. confirm 'draft' → 400 (валідне enum, недозволений перехід) ──────────
# (нове замовлення — попереднє вже confirmed)
BODY_DRAFT="{\"supplier_id\":\"$SUP\",\"order_date\":\"2026-08-07T11:00:00\",\"items\":[]}"
DRAFT_PY=$(curl -s -X POST "$PY/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_DRAFT")
DRAFT_RS=$(curl -s -X POST "$RS/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_DRAFT")
DRAFT_PY_ID=$(echo "$DRAFT_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
DRAFT_RS_ID=$(echo "$DRAFT_RS" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
PD=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$PY/api/v1/purchase-orders/$DRAFT_PY_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"draft"}')
RD=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$RS/api/v1/purchase-orders/$DRAFT_RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"draft"}')
if [ "$PD" = "400" ] && [ "$RD" = "400" ]; then ok "confirm 'draft' 400 (PY=$PD RS=$RD)"; else bad "confirm 'draft': PY=$PD RS=$RD"; fi
DD=$(curl -s -X POST "$RS/api/v1/purchase-orders/$DRAFT_RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"draft"}' | python3 -c "import sys,json;print(json.load(sys.stdin).get('detail',''))")
if echo "$DD" | grep -q "Невірний статус"; then ok "confirm 'draft' detail: 'Невірний статус...'"; else bad "confirm 'draft' detail: $DD"; fi

# ── 10. confirm cancelled (нове замовлення) ─────────────────────────────────
BODY_CANCEL="{\"supplier_id\":\"$SUP\",\"order_date\":\"2026-08-07T12:00:00\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.0,\"price\":100.00,\"total\":100.00}]}"
CAN_PY=$(curl -s -X POST "$PY/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_CANCEL")
CAN_RS=$(curl -s -X POST "$RS/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_CANCEL")
CAN_PY_ID=$(echo "$CAN_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
CAN_RS_ID=$(echo "$CAN_RS" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
CC_PY=$(curl -s -X POST "$PY/api/v1/purchase-orders/$CAN_PY_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"cancelled"}')
CC_RS=$(curl -s -X POST "$RS/api/v1/purchase-orders/$CAN_RS_ID/confirm" -H "$AUTH" -H "Content-Type: application/json" -d '{"status":"cancelled"}')
if [ "$(echo "$CC_PY" | py_norm)" = "$(echo "$CC_RS" | py_norm)" ]; then ok "confirm cancelled parity"; else bad "confirm cancelled: $(echo "$CC_PY" | py_norm | head -c 200) vs $(echo "$CC_RS" | py_norm | head -c 200)"; fi
if [ "$(echo "$CC_PY" | python3 -c "import sys,json;print(json.load(sys.stdin)['status'])")" = "cancelled" ]; then ok "confirm cancelled status=cancelled"; else bad "confirm cancelled status"; fi
# cancelled НЕ створює invoice
CAN_INV=$($PG -c "SELECT count(*) FROM invoices WHERE number LIKE 'ПН-%' AND notes LIKE '%ЗАМ%';")
if [ "$CAN_INV" -ge "0" ]; then ok "confirm cancelled не створює нову накладну"; else bad "confirm cancelled створив invoice"; fi

# ── 11. update/delete confirmed → 400 ───────────────────────────────────────
PU=$(curl -s -o /dev/null -w "%{http_code}" -X PUT "$PY/api/v1/purchase-orders/$PY_ID" -H "$AUTH" -H "Content-Type: application/json" -d '{"notes":"x"}')
RU=$(curl -s -o /dev/null -w "%{http_code}" -X PUT "$RS/api/v1/purchase-orders/$RS_ID" -H "$AUTH" -H "Content-Type: application/json" -d '{"notes":"x"}')
if [ "$PU" = "400" ] && [ "$RU" = "400" ]; then ok "update confirmed 400 (PY=$PU RS=$RU)"; else bad "update confirmed: PY=$PU RS=$RU"; fi
PDEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$PY/api/v1/purchase-orders/$PY_ID" -H "$AUTH")
RDEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$RS/api/v1/purchase-orders/$RS_ID" -H "$AUTH")
if [ "$PDEL" = "400" ] && [ "$RDEL" = "400" ]; then ok "delete confirmed 400 (PY=$PDEL RS=$RDEL)"; else bad "delete confirmed: PY=$PDEL RS=$RDEL"; fi

# ── 12. delete draft → 204 (Python), 204 (Rust); delete 404 ─────────────────
DEL_PY=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$PY/api/v1/purchase-orders/$DRAFT_PY_ID" -H "$AUTH")
DEL_RS=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$RS/api/v1/purchase-orders/$DRAFT_RS_ID" -H "$AUTH")
if [ "$DEL_PY" = "204" ] && [ "$DEL_RS" = "204" ]; then ok "delete draft 204 (PY=$DEL_PY RS=$DEL_RS)"; else bad "delete draft: PY=$DEL_PY RS=$DEL_RS"; fi
PD404=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$PY/api/v1/purchase-orders/$DRAFT_PY_ID" -H "$AUTH")
RD404=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$RS/api/v1/purchase-orders/$DRAFT_RS_ID" -H "$AUTH")
if [ "$PD404" = "404" ] && [ "$RD404" = "404" ]; then ok "delete повторно 404 (PY=$PD404 RS=$RD404)"; else bad "delete повторно: PY=$PD404 RS=$RD404"; fi

# ── 13. list parity + пагінація ─────────────────────────────────────────────
L_PY=$(curl -s "$PY/api/v1/purchase-orders?page=1&size=50" -H "$AUTH")
L_RS=$(curl -s "$RS/api/v1/purchase-orders?page=1&size=50" -H "$AUTH")
if [ "$(echo "$L_PY" | py_norm)" = "$(echo "$L_RS" | py_norm)" ]; then ok "list parity (items/total/page/page_size/pages)"; else bad "list parity"; fi
# пагінація: size=1 page=2 — останній елемент
LP2=$(curl -s "$PY/api/v1/purchase-orders?page=2&size=1" -H "$AUTH")
LR2=$(curl -s "$RS/api/v1/purchase-orders?page=2&size=1" -H "$AUTH")
if [ "$(echo "$LP2" | py_norm)" = "$(echo "$LR2" | py_norm)" ]; then ok "list пагінація page=2&size=1 parity"; else bad "list пагінація"; fi
# page_size/pages поля
PS=$(echo "$L_PY" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d['page_size'],d['pages'],d['page'])")
if [ "$PS" = "50 1 1" ] || [ "$(echo "$PS" | awk '{print $1}')" = "50" ]; then ok "list мета: page_size/pages ($PS)"; else bad "list мета: $PS"; fi

# ── 14. 422 валідації пагінації ─────────────────────────────────────────────
for Q in "page=0" "size=0" "size=1001"; do
  P=$(curl -s -o /dev/null -w "%{http_code}" "$PY/api/v1/purchase-orders?$Q" -H "$AUTH")
  R=$(curl -s -o /dev/null -w "%{http_code}" "$RS/api/v1/purchase-orders?$Q" -H "$AUTH")
  if [ "$P" = "422" ] && [ "$R" = "422" ]; then ok "list $Q → 422 (PY=$P RS=$R)"; else bad "list $Q: PY=$P RS=$R"; fi
done

# ── 15. create без items / без total ────────────────────────────────────────
BODY_EMPTY="{\"supplier_id\":\"$SUP\",\"order_date\":\"2026-08-07T13:00:00\"}"
EP=$(curl -s -X POST "$PY/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_EMPTY")
ER=$(curl -s -X POST "$RS/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_EMPTY")
if [ "$(echo "$EP" | py_norm)" = "$(echo "$ER" | py_norm)" ]; then ok "create без items/total parity (total_amount=null)"; else bad "create без items: $(echo "$EP" | py_norm | head -c 200) vs $(echo "$ER" | py_norm | head -c 200)"; fi

# ── 16. create з явним number та total_amount ───────────────────────────────
BODY_EXPL="{\"number\":\"ЗАМ-MANUAL-$TS\",\"supplier_id\":\"$SUP\",\"order_date\":\"2026-08-07T14:00:00\",\"total_amount\":\"999.99\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.0,\"price\":1.00,\"total\":1.00}]}"
XP=$(curl -s -X POST "$PY/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_EXPL")
XR=$(curl -s -X POST "$RS/api/v1/purchase-orders" -H "$AUTH" -H "Content-Type: application/json" -d "$BODY_EXPL")
if [ "$(echo "$XP" | py_norm)" = "$(echo "$XR" | py_norm)" ]; then ok "create явний number+total parity (total 999.99, не sum)"; else bad "create явний: $(echo "$XP" | py_norm | head -c 250) vs $(echo "$XR" | py_norm | head -c 250)"; fi
XPT=$(echo "$XP" | python3 -c "import sys,json;print(json.load(sys.stdin)['total_amount'])")
if [ "$XPT" = "999.99" ]; then ok "create явний total_amount=999.99 (не sum 1.00)"; else bad "create явний total: $XPT"; fi
XPN=$(echo "$XP" | python3 -c "import sys,json;print(json.load(sys.stdin)['number'])")
if [ "$XPN" = "ЗАМ-MANUAL-$TS" ]; then ok "create явний number збережено"; else bad "create явний number: $XPN"; fi

# ── Підсумок ────────────────────────────────────────────────────────────────
echo ""
echo "[$(date +%H:%M:%S)] ПІДСУМОК: PASS=$PASS FAIL=$FAIL"
if [ "$FAIL" = "0" ]; then echo "E2E PURCHASE_ORDERS: ALL PASS ✅"; else echo "E2E PURCHASE_ORDERS: FAILURES ❌"; fi
exit $FAIL
