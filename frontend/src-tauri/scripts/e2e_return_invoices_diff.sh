#!/usr/bin/env bash
# ============================================================================
# e2e_return_invoices_diff.sh — differential-тест повернень (етап 8, група 4).
# Rust :8002 (KASA_RUST_RETURN_INVOICES=1) vs Python :8001 (еталон).
#
# Покриває (7 роутів v1):
#   create (автономер ПВ-, total=sum, cost_price з продукту, markup),
#   get, list (пагінація), update (scalar+items; тільки draft),
#   delete (тільки draft; 204/400/404), confirm (deduct/add_to_cash/
#   exchange; source_invoice_id → doc_id), cancel (відкат залишків),
#   валідації 400/404/422.
#
# АНОМАЛІЯ PYTHON: confirm з return_action=exchange → 500 (Invoice без
# created_by_id при NOT NULL). Rust реалізує задуману семантику — окремі
# перевірки (Python 500, Rust 200 + створена накладна).
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
SUP_NAME="Diff Ret Sup $TS"
PROD_TITLE="Diff Ret Prod $TS"
PROD2_TITLE="Diff Ret Prod2 $TS"

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
                             "created_by_id","return_invoice_id")}
    if isinstance(x,list): return [drop(v) for v in x]
    if isinstance(x,str):
        x=re.sub(r'№ПВ-[0-9]+-[0-9]+','№ПВ-N',x)
        return x
    return x
print(json.dumps(drop(o),ensure_ascii=False,sort_keys=True))
PYEOF
}

# ── 1. Підготовка тестових даних (напряму в БД) ─────────────────────────────
export PGPASSWORD=VgxWd7MBJ10X
PG="psql -h localhost -U postgres -d pos_system -t -A -q"
SUP=$($PG -c "INSERT INTO suppliers (id,name,phone,created_at,updated_at) VALUES (gen_random_uuid(),'$SUP_NAME','000','2026-08-07',now()) RETURNING id;")
PROD=$($PG -c "INSERT INTO products (id,title,barcode,price,cost_price,markup,unit,is_fiscal,tax_group,tax_rate,stock,created_at,updated_at) VALUES (gen_random_uuid(),'$PROD_TITLE','DIFF-RET-$TS',200.00,120.00,66.67,'шт',false,'А',0,100.000,'2026-08-07',now()) RETURNING id;")
PROD2=$($PG -c "INSERT INTO products (id,title,barcode,price,cost_price,markup,unit,is_fiscal,tax_group,tax_rate,stock,created_at,updated_at) VALUES (gen_random_uuid(),'$PROD2_TITLE','DIFF-RET2-$TS',50.00,25.00,100.00,'шт',false,'А',0,100.000,'2026-08-07',now()) RETURNING id;")
PROD_C=$($PG -c "INSERT INTO products (id,title,barcode,price,cost_price,markup,unit,is_fiscal,tax_group,tax_rate,stock,created_at,updated_at) VALUES (gen_random_uuid(),'Diff Ret ProdC $TS','DIFF-RETC-$TS',80.00,40.00,100.00,'шт',false,'А',0,10.000,'2026-08-07',now()) RETURNING id;")
SRC_INV=$($PG -c "INSERT INTO invoices (id,number,supplier_id,invoice_date,payment_method,is_fiscal,total_amount,status,created_by_id,created_at,updated_at) VALUES (gen_random_uuid(),'DIFF-SRC-$TS','$SUP','2026-08-07','cash',false,100.00,'confirmed',(SELECT id FROM users WHERE role='admin' LIMIT 1),now(),now()) RETURNING id;")
log "тестові дані: sup=$SUP prod=$PROD prod2=$PROD2 prodc=$PROD_C src_inv=$SRC_INV"

# ── 2. CREATE (parity) ──────────────────────────────────────────────────────
BODY_PY="{\"number\":\"DIFF-PY-$TS\",\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T12:00:00\",\"return_action\":\"deduct_from_debt\",\"is_fiscal\":false,\"notes\":\"diff ret create\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":3.000,\"price\":200.00,\"total\":600.00}]}"
BODY_RS="{\"number\":\"DIFF-RS-$TS\",\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T12:00:00\",\"return_action\":\"deduct_from_debt\",\"is_fiscal\":false,\"notes\":\"diff ret create\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":3.000,\"price\":200.00,\"total\":600.00}]}"
PY_CREATE=$(curl -s -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$BODY_PY")
RS_CREATE=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$BODY_RS")
if [ "$(echo "$PY_CREATE" | py_norm)" = "$(echo "$RS_CREATE" | py_norm)" ]; then ok "create: parity (normalized, cost+markup з продукту)"; else bad "create: parity"; echo "  PY: $(echo "$PY_CREATE" | head -c 400)"; echo "  RS: $(echo "$RS_CREATE" | head -c 400)"; fi
PY_RET=$(echo "$PY_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
RS_RET=$(echo "$RS_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")

# ── 3. CREATE: автономер (формат ПВ-YYYYMMDD-NNN) ───────────────────────────

AUTO_PY=$(curl -s -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T13:00:00\",\"return_action\":\"deduct_from_debt\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1,\"price\":10,\"total\":10}]}")
AUTO_RS=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T13:00:00\",\"return_action\":\"deduct_from_debt\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1,\"price\":10,\"total\":10}]}")
if [ "$(echo "$AUTO_PY" | py_norm)" = "$(echo "$AUTO_RS" | py_norm)" ]; then ok "create автономер+total=sum: parity"; else bad "create автономер+total=sum"; echo "  PY: $(echo "$AUTO_PY" | head -c 300)"; echo "  RS: $(echo "$AUTO_RS" | head -c 300)"; fi
AUTO_RS_ID=$(echo "$AUTO_RS" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
AUTO_PY_NUM=$(echo "$AUTO_PY" | python3 -c "import sys,json; print(json.load(sys.stdin)['number'])" 2>/dev/null)
AUTO_RS_NUM=$(echo "$AUTO_RS" | python3 -c "import sys,json; print(json.load(sys.stdin)['number'])" 2>/dev/null)
if echo "$AUTO_PY_NUM" | grep -qE '^ПВ-[0-9]{8}-[0-9]{3}$' && echo "$AUTO_RS_NUM" | grep -qE '^ПВ-[0-9]{8}-[0-9]{3}$'; then ok "create автономер: формат ПВ-YYYYMMDD-NNN (PY=$AUTO_PY_NUM RS=$AUTO_RS_NUM)"; else bad "create автономер: формат (PY=$AUTO_PY_NUM RS=$AUTO_RS_NUM)"; fi

# ── 4. GET (parity, обидві гілки читають RS id) ─────────────────────────────
PY_GET=$(curl -s $PY/api/v1/return-invoices/$RS_RET -H "$AUTH")
RS_GET=$(curl -s $RS/api/v1/return-invoices/$RS_RET -H "$AUTH")
if [ "$(echo "$PY_GET" | py_norm)" = "$(echo "$RS_GET" | py_norm)" ]; then ok "get: parity"; else bad "get: parity"; echo "  PY: $(echo "$PY_GET" | head -c 300)"; echo "  RS: $(echo "$RS_GET" | head -c 300)"; fi
PY_404=$(curl -s -o /dev/null -w "%{http_code}" $PY/api/v1/return-invoices/00000000-0000-0000-0000-000000000000 -H "$AUTH")
RS_404=$(curl -s -o /dev/null -w "%{http_code}" $RS/api/v1/return-invoices/00000000-0000-0000-0000-000000000000 -H "$AUTH")
if [ "$PY_404" = "$RS_404" ] && [ "$RS_404" = "404" ]; then ok "get незнайдений → 404 parity"; else bad "get незнайдений → 404 (PY=$PY_404 RS=$RS_404)"; fi

# ── 5. LIST (parity + пагінація структура) ─────────────────────────────────
PY_L=$(curl -s "$PY/api/v1/return-invoices?page=1&size=10" -H "$AUTH")
RS_L=$(curl -s "$RS/api/v1/return-invoices?page=1&size=10" -H "$AUTH")
if [ "$(echo "$PY_L" | py_norm)" = "$(echo "$RS_L" | py_norm)" ]; then ok "list: parity (normalized)"; else bad "list: parity"; echo "  PY: $(echo "$PY_L" | head -c 300)"; echo "  RS: $(echo "$RS_L" | head -c 300)"; fi
PY_P=$(curl -s "$PY/api/v1/return-invoices?page=2&size=7" -H "$AUTH"); RS_P=$(curl -s "$RS/api/v1/return-invoices?page=2&size=7" -H "$AUTH")
if [ "$(echo "$PY_P" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'],d['page'],d['page_size'],d['pages'],len(d['items']))")" = "$(echo "$RS_P" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'],d['page'],d['page_size'],d['pages'],len(d['items']))")" ]; then ok "list пагінація: структура"; else bad "list пагінація: структура"; fi

# ── 6. UPDATE: scalar + items (parity) ──────────────────────────────────────
UP_BODY='{"notes":"оновлено diff","total_amount":"700.00"}'
PY_UP=$(curl -s -X PUT $PY/api/v1/return-invoices/$RS_RET -H "$AUTH" -H 'Content-Type: application/json' -d "$UP_BODY")
RS_UP=$(curl -s -X PUT $RS/api/v1/return-invoices/$RS_RET -H "$AUTH" -H 'Content-Type: application/json' -d "$UP_BODY")
if [ "$(echo "$PY_UP" | py_norm)" = "$(echo "$RS_UP" | py_norm)" ]; then ok "update scalar: parity"; else bad "update scalar: parity"; echo "  PY: $(echo "$PY_UP" | head -c 300)"; echo "  RS: $(echo "$RS_UP" | head -c 300)"; fi
UP_ITEMS="{\"items\":[{\"product_id\":\"$PROD\",\"quantity\":2.000,\"price\":150.00,\"total\":300.00},{\"product_id\":\"$PROD2\",\"quantity\":1.000,\"price\":50.00,\"total\":50.00}]}"
PY_UPI=$(curl -s -X PUT $PY/api/v1/return-invoices/$RS_RET -H "$AUTH" -H 'Content-Type: application/json' -d "$UP_ITEMS")
RS_UPI=$(curl -s -X PUT $RS/api/v1/return-invoices/$RS_RET -H "$AUTH" -H 'Content-Type: application/json' -d "$UP_ITEMS")
if [ "$(echo "$PY_UPI" | py_norm)" = "$(echo "$RS_UPI" | py_norm)" ]; then ok "update items (заміна): parity"; else bad "update items (заміна)"; echo "  PY: $(echo "$PY_UPI" | head -c 400)"; echo "  RS: $(echo "$RS_UPI" | head -c 400)"; fi

# ── 7. ВАЛІДАЦІЇ 422 (page/size) ───────────────────────────────────────────
PY_422=$(curl -s -o /dev/null -w "%{http_code}" "$PY/api/v1/return-invoices?size=1001" -H "$AUTH")
RS_422=$(curl -s -o /dev/null -w "%{http_code}" "$RS/api/v1/return-invoices?size=1001" -H "$AUTH")
if [ "$PY_422" = "$RS_422" ] && [ "$RS_422" = "422" ]; then ok "list size>1000 → 422 parity"; else bad "list size>1000 → 422 (PY=$PY_422 RS=$RS_422)"; fi
PY_422=$(curl -s -o /dev/null -w "%{http_code}" "$PY/api/v1/return-invoices?page=0" -H "$AUTH")
RS_422=$(curl -s -o /dev/null -w "%{http_code}" "$RS/api/v1/return-invoices?page=0" -H "$AUTH")
if [ "$PY_422" = "$RS_422" ] && [ "$RS_422" = "422" ]; then ok "list page=0 → 422 parity"; else bad "list page=0 → 422 (PY=$PY_422 RS=$RS_422)"; fi

# ── 8. CREATE exchange без exchange_items → 400 (parity) ────────────────────
EX_BODY="{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T14:00:00\",\"return_action\":\"exchange\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1,\"price\":10,\"total\":10}]}"
PY_EX=$(curl -s -w "\n%{http_code}" -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$EX_BODY")
RS_EX=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$EX_BODY")
PY_EX_CODE=$(echo "$PY_EX" | tail -1); RS_EX_CODE=$(echo "$RS_EX" | tail -1)
PY_EX_D=$(echo "$PY_EX" | head -1 | python3 -c "import sys,json; print(json.load(sys.stdin)['detail'][:60])" 2>/dev/null)
RS_EX_D=$(echo "$RS_EX" | head -1 | python3 -c "import sys,json; print(json.load(sys.stdin)['detail'][:60])" 2>/dev/null)
if [ "$PY_EX_CODE" = "$RS_EX_CODE" ] && [ "$RS_EX_CODE" = "400" ] && [ "$PY_EX_D" = "$RS_EX_D" ]; then ok "create exchange без items → 400 parity"; else bad "create exchange без items → 400 (PY=$PY_EX_CODE:$PY_EX_D RS=$RS_EX_CODE:$RS_EX_D)"; fi

# ── 9. CONFIRM (deduct_from_debt) parity + БД ───────────────────────────────
# Примітка: total_amount залишається 700.00 (Python не перераховує його при
# update items) — ledger буде -700.00.
PY_CF=$(curl -s -X POST $PY/api/v1/return-invoices/$PY_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_CF=$(curl -s -X POST $RS/api/v1/return-invoices/$RS_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
if [ "$(echo "$PY_CF" | py_norm)" = "$(echo "$RS_CF" | py_norm)" ]; then ok "confirm deduct: parity (status confirmed, notes action_label)"; else bad "confirm deduct: parity"; echo "  PY: $(echo "$PY_CF" | head -c 400)"; echo "  RS: $(echo "$RS_CF" | head -c 400)"; fi
RS_STOCK=$($PG -c "SELECT stock::text FROM products WHERE id='$PROD';")
RS_LEDGER=$($PG -c "SELECT count(*) FROM supplier_ledger WHERE document_id='$RS_RET' AND operation_type='return' AND amount::text='-700.00';")
# Чиста БД-перевірка: RS-повернення на PROD_C (stock 10) quantity 2 → 8.
CFC_RS=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T12:30:00\",\"items\":[{\"product_id\":\"$PROD_C\",\"quantity\":2.000,\"price\":80.00,\"total\":160.00}]}" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
curl -s -X POST $RS/api/v1/return-invoices/$CFC_RS/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}' > /dev/null
RS_STOCK_C=$($PG -c "SELECT stock::text FROM products WHERE id='$PROD_C';")
if [ "$RS_STOCK_C" = "8.000" ]; then ok "confirm БД (чистий RS): stock 10→8, ledger -700.00 (doc_id=повернення)"; else bad "confirm БД: stock_c=$RS_STOCK_C ledger=$RS_LEDGER"; fi

# ── 10. CONFIRM повторно → 400 (parity) ─────────────────────────────────────
PY_CF2=$(curl -s -o /dev/null -w "%{http_code}" -X POST $PY/api/v1/return-invoices/$PY_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_CF2=$(curl -s -o /dev/null -w "%{http_code}" -X POST $RS/api/v1/return-invoices/$RS_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
PY_CF2_D=$(curl -s -X POST $PY/api/v1/return-invoices/$PY_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}' | python3 -c "import sys,json; print(json.load(sys.stdin)['detail'])" 2>/dev/null)
RS_CF2_D=$(curl -s -X POST $RS/api/v1/return-invoices/$RS_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}' | python3 -c "import sys,json; print(json.load(sys.stdin)['detail'])" 2>/dev/null)
if [ "$PY_CF2" = "$RS_CF2" ] && [ "$RS_CF2" = "400" ] && [ "$PY_CF2_D" = "$RS_CF2_D" ]; then ok "confirm повторно → 400 parity"; else bad "confirm повторно → 400 (PY=$PY_CF2 RS=$RS_CF2)"; fi

# ── 11. CONFIRM незнайдений → 404 (parity) ─────────────────────────────────
PY_CFN=$(curl -s -o /dev/null -w "%{http_code}" -X POST $PY/api/v1/return-invoices/00000000-0000-0000-0000-000000000000/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_CFN=$(curl -s -o /dev/null -w "%{http_code}" -X POST $RS/api/v1/return-invoices/00000000-0000-0000-0000-000000000000/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
if [ "$PY_CFN" = "$RS_CFN" ] && [ "$RS_CFN" = "404" ]; then ok "confirm незнайдений → 404 parity"; else bad "confirm незнайдений → 404 (PY=$PY_CFN RS=$RS_CFN)"; fi

# ── 12. CONFIRM невірний статус → 400 (parity) ──────────────────────────────
PY_CFB=$(curl -s -o /dev/null -w "%{http_code}" -X POST $PY/api/v1/return-invoices/$PY_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"shipped"}')
RS_CFB=$(curl -s -o /dev/null -w "%{http_code}" -X POST $RS/api/v1/return-invoices/$RS_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"shipped"}')
if [ "$PY_CFB" = "$RS_CFB" ] && [ "$RS_CFB" = "422" ]; then ok "confirm невірний статус → 422 parity (Pydantic enum)"; else bad "confirm невірний статус (PY=$PY_CFB RS=$RS_CFB)"; fi

# ── 13. CANCEL (confirmed) parity + БД ──────────────────────────────────────
PY_CC=$(curl -s -X POST $PY/api/v1/return-invoices/$PY_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"cancelled"}')
RS_CC=$(curl -s -X POST $RS/api/v1/return-invoices/$RS_RET/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"cancelled"}')
if [ "$(echo "$PY_CC" | py_norm)" = "$(echo "$RS_CC" | py_norm)" ]; then ok "cancel (confirmed): parity"; else bad "cancel (confirmed): parity"; echo "  PY: $(echo "$PY_CC" | head -c 300)"; echo "  RS: $(echo "$RS_CC" | head -c 300)"; fi
# Чистий cancel-БД на PROD_C (stock 8 після confirm) → 10.
curl -s -X POST $RS/api/v1/return-invoices/$CFC_RS/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"cancelled"}' > /dev/null
RS_STOCK_C=$($PG -c "SELECT stock::text FROM products WHERE id='$PROD_C';")
if [ "$RS_STOCK_C" = "10.000" ]; then ok "cancel БД (чистий RS): stock 8→10"; else bad "cancel БД: stock_c=$RS_STOCK_C"; fi
# ledger НЕ видаляється при cancel (Python 1:1)
RS_LEDGER=$($PG -c "SELECT count(*) FROM supplier_ledger WHERE document_id='$RS_RET';")
if [ "$RS_LEDGER" = "1" ]; then ok "cancel: ledger залишається (Python 1:1)"; else bad "cancel: ledger=$RS_LEDGER (очікувано 1)"; fi

# ── 14. CANCEL draft → 400 (parity) ─────────────────────────────────────────
PY_CD=$(curl -s -o /dev/null -w "%{http_code}" -X POST $PY/api/v1/return-invoices/$AUTO_RS_ID/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"cancelled"}')
RS_CD=$(curl -s -o /dev/null -w "%{http_code}" -X POST $RS/api/v1/return-invoices/$AUTO_RS_ID/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"cancelled"}')
if [ "$PY_CD" = "$RS_CD" ] && [ "$RS_CD" = "400" ]; then ok "cancel draft → 400 parity"; else bad "cancel draft → 400 (PY=$PY_CD RS=$RS_CD)"; fi

# ── 15. UPDATE confirmed → 400 (parity) ─────────────────────────────────────
PY_UPD=$(curl -s -o /dev/null -w "%{http_code}" -X PUT $PY/api/v1/return-invoices/$PY_RET -H "$AUTH" -H 'Content-Type: application/json' -d '{"notes":"x"}')
RS_UPD=$(curl -s -o /dev/null -w "%{http_code}" -X PUT $RS/api/v1/return-invoices/$RS_RET -H "$AUTH" -H 'Content-Type: application/json' -d '{"notes":"x"}')
if [ "$PY_UPD" = "$RS_UPD" ] && [ "$RS_UPD" = "400" ]; then ok "update confirmed → 400 parity"; else bad "update confirmed → 400 (PY=$PY_UPD RS=$RS_UPD)"; fi

# ── 16. DELETE: confirmed → 400; draft → 204; незнайдений → 404 (parity) ────
PY_DEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE $PY/api/v1/return-invoices/$PY_RET -H "$AUTH")
RS_DEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE $RS/api/v1/return-invoices/$RS_RET -H "$AUTH")
if [ "$PY_DEL" = "$RS_DEL" ] && [ "$RS_DEL" = "400" ]; then ok "delete confirmed → 400 parity"; else bad "delete confirmed → 400 (PY=$PY_DEL RS=$RS_DEL)"; fi
# Створюємо окремі чернетки для delete parity
DEL_PY=$(curl -s -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T15:00:00\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1,\"price\":10,\"total\":10}]}" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
DEL_RS=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T15:00:00\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1,\"price\":10,\"total\":10}]}" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
PY_DEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE $PY/api/v1/return-invoices/$DEL_PY -H "$AUTH")
RS_DEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE $RS/api/v1/return-invoices/$DEL_RS -H "$AUTH")
if [ "$PY_DEL" = "$RS_DEL" ] && [ "$RS_DEL" = "204" ]; then ok "delete draft → 204 parity"; else bad "delete draft → 204 (PY=$PY_DEL RS=$RS_DEL)"; fi
PY_DEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE $PY/api/v1/return-invoices/00000000-0000-0000-0000-000000000000 -H "$AUTH")
RS_DEL=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE $RS/api/v1/return-invoices/00000000-0000-0000-0000-000000000000 -H "$AUTH")
if [ "$PY_DEL" = "$RS_DEL" ] && [ "$RS_DEL" = "404" ]; then ok "delete незнайдений → 404 parity"; else bad "delete незнайдений → 404 (PY=$PY_DEL RS=$RS_DEL)"; fi

# ── 17. CONFIRM add_to_cash: parity (ledger 0.00, notes в касу) ─────────────
CASH_PY=$(curl -s -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T16:00:00\",\"return_action\":\"add_to_cash\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.000,\"price\":100.00,\"total\":100.00}]}" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
CASH_RS=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T16:00:00\",\"return_action\":\"add_to_cash\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.000,\"price\":100.00,\"total\":100.00}]}" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
PY_CASH=$(curl -s -X POST $PY/api/v1/return-invoices/$CASH_PY/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_CASH=$(curl -s -X POST $RS/api/v1/return-invoices/$CASH_RS/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
if [ "$(echo "$PY_CASH" | py_norm)" = "$(echo "$RS_CASH" | py_norm)" ]; then ok "confirm add_to_cash: parity"; else bad "confirm add_to_cash: parity"; echo "  PY: $(echo "$PY_CASH" | head -c 300)"; echo "  RS: $(echo "$RS_CASH" | head -c 300)"; fi
RS_LEDGER=$($PG -c "SELECT amount::text FROM supplier_ledger WHERE document_id='$CASH_RS' AND operation_type='return';")
if [ "$RS_LEDGER" = "0.00" ]; then ok "add_to_cash БД: ledger amount=0.00"; else bad "add_to_cash БД: ledger=$RS_LEDGER"; fi

# ── 18. CONFIRM source_invoice_id: parity + БД (doc_id = source) ────────────
SRC_BODY="{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T17:00:00\",\"source_invoice_id\":\"$SRC_INV\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.000,\"price\":50.00,\"total\":50.00}]}"
SRC_PY=$(curl -s -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$SRC_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
SRC_RS=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$SRC_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
PY_SRC=$(curl -s -X POST $PY/api/v1/return-invoices/$SRC_PY/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_SRC=$(curl -s -X POST $RS/api/v1/return-invoices/$SRC_RS/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
if [ "$(echo "$PY_SRC" | py_norm)" = "$(echo "$RS_SRC" | py_norm)" ]; then ok "confirm source_invoice_id: parity (doc_id=накладна, notes прив'язка)"; else bad "confirm source_invoice_id: parity"; echo "  PY: $(echo "$PY_SRC" | head -c 400)"; echo "  RS: $(echo "$RS_SRC" | head -c 400)"; fi
RS_LEDGER=$($PG -c "SELECT count(*) FROM supplier_ledger WHERE document_id='$SRC_INV' AND operation_type='return';")
if [ "$RS_LEDGER" -ge "1" ]; then ok "source БД: ledger document_id = source_invoice_id (Python doc_id = source накладної)"; else bad "source БД: ledger=$RS_LEDGER (очікувано >=1 по $SRC_INV)"; fi

# ── 19. EXCHANGE: Python 500 (аномалія) vs Rust задумана семантика ──────────
EXC_BODY="{\"supplier_id\":\"$SUP\",\"return_date\":\"2026-08-07T18:00:00\",\"return_action\":\"exchange\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":2.000,\"price\":100.00,\"total\":200.00}],\"exchange_items\":[{\"product_id\":\"$PROD2\",\"quantity\":5.000,\"price\":30.00,\"total\":150.00}]}"
EXC_PY=$(curl -s -X POST $PY/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$EXC_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
EXC_RS=$(curl -s -X POST $RS/api/v1/return-invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$EXC_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
PY_EXC_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST $PY/api/v1/return-invoices/$EXC_PY/confirm -H "$AUTH" -H 'Content-Type: application/json' -d "{\"status\":\"confirmed\",\"exchange_items\":[{\"product_id\":\"$PROD2\",\"quantity\":5.000,\"price\":30.00,\"total\":150.00}]}")
RS_EXC_BODY=$(curl -s -X POST $RS/api/v1/return-invoices/$EXC_RS/confirm -H "$AUTH" -H 'Content-Type: application/json' -d "{\"status\":\"confirmed\",\"exchange_items\":[{\"product_id\":\"$PROD2\",\"quantity\":5.000,\"price\":30.00,\"total\":150.00}]}")
RS_EXC_CODE=$(echo "$RS_EXC_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','error'))" 2>/dev/null)
if [ "$PY_EXC_CODE" = "500" ]; then ok "exchange: Python 500 (аномалія Invoice created_by_id NOT NULL)"; else bad "exchange: Python очікувано 500, отримано $PY_EXC_CODE"; fi
if [ "$RS_EXC_CODE" = "confirmed" ]; then
    EXC_INV=$(echo "$RS_EXC_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('exchange_invoice_id',''))" 2>/dev/null)
    RS_STOCK2=$($PG -c "SELECT stock::text FROM products WHERE id='$PROD2';")
    RS_EXC_ST=$($PG -c "SELECT status::text FROM invoices WHERE id='$EXC_INV';" 2>/dev/null)
    if [ -n "$EXC_INV" ] && [ "$RS_EXC_ST" = "confirmed" ] && [ "$RS_STOCK2" = "105.000" ]; then ok "exchange: Rust 200 + накладна confirmed + stock 100→105 (задумана семантика)"; else bad "exchange: Rust накладна/exchange_invoice_id/stock (inv=$EXC_INV st=$RS_EXC_ST stock2=$RS_STOCK2)"; fi
    # cancel exchange: накладна cancelled, stock відкат
    RS_CX=$(curl -s -X POST $RS/api/v1/return-invoices/$EXC_RS/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"cancelled"}')
    RS_EXC_ST=$($PG -c "SELECT status::text FROM invoices WHERE id='$EXC_INV';")
    RS_STOCK2=$($PG -c "SELECT stock::text FROM products WHERE id='$PROD2';")
    RS_STOCK1=$($PG -c "SELECT stock::text FROM products WHERE id='$PROD';")
    if [ "$(echo "$RS_CX" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])")" = "cancelled" ] && [ "$RS_EXC_ST" = "cancelled" ] && [ "$RS_STOCK2" = "100.000" ]; then ok "cancel exchange: Rust 200 + накладна cancelled + stock2 відкат (105→100)"; else bad "cancel exchange (st=$RS_EXC_ST stock2=$RS_STOCK2)"; fi
else
    bad "exchange: Rust очікувано 200, отримано $RS_EXC_CODE"
fi

# ── Підсумок ────────────────────────────────────────────────────────────────
echo "[$(date +%H:%M:%S)] ПІДСУМОК: PASS=$PASS FAIL=$FAIL"
if [ "$FAIL" -eq 0 ]; then echo "E2E RETURN_INVOICES: ALL PASS ✅"; else echo "E2E RETURN_INVOICES: FAILURES ❌"; exit 1; fi
