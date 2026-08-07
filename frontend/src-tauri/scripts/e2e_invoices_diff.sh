#!/usr/bin/env bash
# ============================================================================
# e2e_documents_diff.sh — differential-тест інвойсів (етап 8, група 3).
# Rust :8002 (KASA_RUST_INVOICES=1) vs Python :8001 (еталон).
#
# Покриває (v1+v2):
#   v1: create/get/list(фільтри+пагінація)/update(scalar+items)/delete/
#       payment-info/price-changes/confirm/cancel/print-items/валідації
#   v2: list(search/status/date)/get/update/delete/payment/price-changes +
#       create/confirm/cancel (Python 500 — Rust реалізує задуману семантику)
#   print-items: структура + HTML (SVG-блоки нормалізуються — різні генератори)
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
SUP_NAME="Diff Inv Sup $TS"
PROD_TITLE="Diff Inv Prod $TS"

log()  { echo "[$(date +%H:%M:%S)] $*"; }
ok()   { PASS=$((PASS+1)); echo "  ✅ $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

# ── нормалізація JSON для порівняння (динамічні поля → маркери) ────────────
py_norm() {
python3 - << 'PYEOF'
import json,sys,re
o=json.load(sys.stdin)
def drop(x, path=""):
    if isinstance(x,dict):
        return {k:drop(v,path+"/"+k) for k,v in x.items()
                if k not in ("id","number","created_at","updated_at","invoice_id","created_by_id","confirmed_at")}
    if isinstance(x,list): return [drop(v,path) for v in x]
    return x
def strip_svg(x):
    if isinstance(x,str):
        x=re.sub(r'<svg[^>]*>.*?</svg>','[SVG]',x,flags=re.S)
        x=re.sub(r'<span style="font-family: monospace[^>]*>\[QR:[^<]*</span>','[QR]',x)
        return x
    if isinstance(x,dict): return {k:strip_svg(v) for k,v in x.items()}
    if isinstance(x,list): return [strip_svg(v) for v in x]
    return x
print(json.dumps(strip_svg(drop(o)),ensure_ascii=False,sort_keys=True))
PYEOF
}

# ── 1. Підготовка тестових даних (напряму в БД) ─────────────────────────────
export PGPASSWORD=VgxWd7MBJ10X
PG="psql -h localhost -U postgres -d pos_system -t -A -q"
SUP=$($PG -c "INSERT INTO suppliers (id,name,phone,created_at,updated_at) VALUES (gen_random_uuid(),'$SUP_NAME','000','2026-08-07',now()) RETURNING id;")
PROD=$($PG -c "INSERT INTO products (id,title,barcode,price,cost_price,markup,unit,is_fiscal,tax_group,tax_rate,created_at,updated_at) VALUES (gen_random_uuid(),'$PROD_TITLE','DIFF-INV-$TS',100.00,50.00,100.00,'шт',false,'А',0,'2026-08-07',now()) RETURNING id;")
log "тестові дані: sup=$SUP prod=$PROD"

# ── 2. v1 CREATE (parity) ───────────────────────────────────────────────────
BODY_PY="{\"number\":\"DIFF-PY-$TS\",\"supplier_id\":\"$SUP\",\"invoice_date\":\"2026-08-07T12:00:00\",\"is_fiscal\":false,\"notes\":\"diff v1 create\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.5,\"price\":12.4,\"total\":18.6}]}"
BODY_RS="{\"number\":\"DIFF-RS-$TS\",\"supplier_id\":\"$SUP\",\"invoice_date\":\"2026-08-07T12:00:00\",\"is_fiscal\":false,\"notes\":\"diff v1 create\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1.5,\"price\":12.4,\"total\":18.6}]}"
PY_CREATE=$(curl -s -X POST $PY/api/v1/invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$BODY_PY")
RS_CREATE=$(curl -s -X POST $RS/api/v1/invoices -H "$AUTH" -H 'Content-Type: application/json' -d "$BODY_RS")
if [ "$(echo "$PY_CREATE" | py_norm)" = "$(echo "$RS_CREATE" | py_norm)" ]; then ok "v1 create: parity (normalized)"; else bad "v1 create: parity"; echo "  PY: $(echo "$PY_CREATE" | head -c 300)"; echo "  RS: $(echo "$RS_CREATE" | head -c 300)"; fi
RS_INV=$(echo "$RS_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
PY_INV=$(echo "$PY_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")

# ── 3. v1 GET (parity) ──────────────────────────────────────────────────────
PY_GET=$(curl -s $PY/api/v1/invoices/$RS_INV -H "$AUTH")
RS_GET=$(curl -s $RS/api/v1/invoices/$RS_INV -H "$AUTH")
if [ "$(echo "$PY_GET" | py_norm)" = "$(echo "$RS_GET" | py_norm)" ]; then ok "v1 get: parity"; else bad "v1 get: parity"; echo "  PY: $(echo "$PY_GET" | head -c 200)"; echo "  RS: $(echo "$RS_GET" | head -c 200)"; fi

# ── 4. v1 LIST: supplier filter + pagination ────────────────────────────────
PY_L=$(curl -s "$PY/api/v1/invoices?supplier_id=$SUP&page=1&size=10" -H "$AUTH")
RS_L=$(curl -s "$RS/api/v1/invoices?supplier_id=$SUP&page=1&size=10" -H "$AUTH")
if [ "$(echo "$PY_L" | py_norm)" = "$(echo "$RS_L" | py_norm)" ]; then ok "v1 list supplier-filter: parity"; else bad "v1 list supplier-filter"; echo "  PY: $(echo "$PY_L" | head -c 200)"; echo "  RS: $(echo "$RS_L" | head -c 200)"; fi
PY_P=$(curl -s "$PY/api/v1/invoices?page=2&size=7" -H "$AUTH"); RS_P=$(curl -s "$RS/api/v1/invoices?page=2&size=7" -H "$AUTH")
if [ "$(echo "$PY_P" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'],d['page'],d['page_size'],d['pages'],len(d['items']))")" = "$(echo "$RS_P" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'],d['page'],d['page_size'],d['pages'],len(d['items']))")" ]; then ok "v1 list pagination: структура"; else bad "v1 list pagination"; fi

# ── 5. v1 UPDATE: scalar + items (parity) ───────────────────────────────────
PY_U=$(curl -s -X PUT $PY/api/v1/invoices/$RS_INV -H "$AUTH" -H 'Content-Type: application/json' -d '{"notes":"updated scalar"}')
RS_U=$(curl -s -X PUT $RS/api/v1/invoices/$RS_INV -H "$AUTH" -H 'Content-Type: application/json' -d '{"notes":"updated scalar"}')
if [ "$(echo "$PY_U" | py_norm)" = "$(echo "$RS_U" | py_norm)" ]; then ok "v1 update scalar: parity"; else bad "v1 update scalar"; echo "  PY: $(echo "$PY_U"|head -c 200)"; echo "  RS: $(echo "$RS_U"|head -c 200)"; fi
PY_UI=$(curl -s -X PUT $PY/api/v1/invoices/$RS_INV -H "$AUTH" -H 'Content-Type: application/json' -d "{\"items\":[{\"product_id\":\"$PROD\",\"quantity\":2,\"price\":7.5,\"total\":15.0}]}")
RS_UI=$(curl -s -X PUT $RS/api/v1/invoices/$RS_INV -H "$AUTH" -H 'Content-Type: application/json' -d "{\"items\":[{\"product_id\":\"$PROD\",\"quantity\":2,\"price\":7.5,\"total\":15.0}]}")
if [ "$(echo "$PY_UI" | py_norm)" = "$(echo "$RS_UI" | py_norm)" ]; then ok "v1 update items: parity (total=sum)"; else bad "v1 update items"; echo "  PY: $(echo "$PY_UI"|head -c 300)"; echo "  RS: $(echo "$RS_UI"|head -c 300)"; fi

# ── 6. v1 PAYMENT-INFO (parity) ─────────────────────────────────────────────
PY_PI=$(curl -s $PY/api/v1/invoices/$RS_INV/payment-info -H "$AUTH")
RS_PI=$(curl -s $RS/api/v1/invoices/$RS_INV/payment-info -H "$AUTH")
if [ "$(echo "$PY_PI" | py_norm)" = "$(echo "$RS_PI" | py_norm)" ]; then ok "v1 payment-info: parity"; else bad "v1 payment-info"; echo "  PY: $PY_PI"; echo "  RS: $RS_PI"; fi

# ── 7. v1 PRICE-CHANGES (parity) ────────────────────────────────────────────
PY_PC=$(curl -s $PY/api/v1/invoices/$RS_INV/price-changes -H "$AUTH" | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps([{k:(v if k!='product_id' else 'P') for k,v in x.items()} for x in d],ensure_ascii=False,sort_keys=True))")
RS_PC=$(curl -s $RS/api/v1/invoices/$RS_INV/price-changes -H "$AUTH" | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps([{k:(v if k!='product_id' else 'P') for k,v in x.items()} for x in d],ensure_ascii=False,sort_keys=True))")
if [ "$PY_PC" = "$RS_PC" ]; then ok "v1 price-changes: parity"; else bad "v1 price-changes"; echo "  PY: $PY_PC"; echo "  RS: $RS_PC"; fi

# ── 8. v1 PRINT-ITEMS на чернетці → 400 (parity) ────────────────────────────
TPL=$(psql -h localhost -U postgres -d pos_system -t -A -c "SELECT id FROM print_templates WHERE name LIKE 'Цінник стандартний' LIMIT 1")
PY_PR=$(curl -s -w "\n%{http_code}" -X POST $PY/api/v1/invoices/$RS_INV/print-items -H "$AUTH" -H 'Content-Type: application/json' -d "{\"print_type\":\"price_tag\",\"template_id\":\"$TPL\"}")
RS_PR=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v1/invoices/$RS_INV/print-items -H "$AUTH" -H 'Content-Type: application/json' -d "{\"print_type\":\"price_tag\",\"template_id\":\"$TPL\"}")
PY_CODE=$(echo "$PY_PR"|tail -1); RS_CODE=$(echo "$RS_PR"|tail -1)
PY_BODY=$(echo "$PY_PR"|head -1); RS_BODY=$(echo "$RS_PR"|head -1)
if [ "$PY_CODE" = "$RS_CODE" ] && [ "$(echo "$PY_BODY"|py_norm)" = "$(echo "$RS_BODY"|py_norm)" ]; then ok "v1 print-items draft→400: parity"; else bad "v1 print-items draft→400"; echo "  PY: $PY_CODE $PY_BODY"; echo "  RS: $RS_CODE $RS_BODY"; fi

# ── 9. v1 CONFIRM (confirmed) + БД-ефекти ───────────────────────────────────
PY_C=$(curl -s -X POST $PY/api/v1/invoices/$RS_INV/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_C=$(curl -s -X POST $RS/api/v1/invoices/$RS_INV/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
if [ "$(echo "$PY_C"|py_norm)" = "$(echo "$RS_C"|py_norm)" ]; then ok "v1 confirm: parity"; else bad "v1 confirm"; echo "  PY: $(echo "$PY_C"|head -c 250)"; echo "  RS: $(echo "$RS_C"|head -c 250)"; fi
STOCK_RS=$($PG -c "SELECT stock FROM products WHERE id='$PROD';")
LEDGER_RS=$($PG -c "SELECT count(*) FROM supplier_ledger WHERE document_id='$RS_INV' AND operation_type='invoice';")
if [ "$STOCK_RS" = "2.000" ] && [ "$LEDGER_RS" = "1" ]; then ok "v1 confirm БД: stock=2, ledger INVOICE=1"; else bad "v1 confirm БД (stock=$STOCK_RS ledger=$LEDGER_RS)"; fi
# повторний confirm → 400
PY_C2=$(curl -s -w "\n%{http_code}" -X POST $PY/api/v1/invoices/$RS_INV/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
RS_C2=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v1/invoices/$RS_INV/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"confirmed"}')
if [ "$(echo "$PY_C2"|tail -1)" = "$(echo "$RS_C2"|tail -1)" ] && [ "$(echo "$PY_C2"|head -1)" = "$(echo "$RS_C2"|head -1)" ]; then ok "v1 confirm повторний→400: parity"; else bad "v1 confirm повторний"; echo "  PY: $(echo "$PY_C2"|tail -1) $(echo "$PY_C2"|head -1)"; echo "  RS: $(echo "$RS_C2"|tail -1) $(echo "$RS_C2"|head -1)"; fi

# ── 10. v1 PRINT-ITEMS на підтвердженій (структура + HTML normalized) ───────
PY_PR2=$(curl -s -X POST $PY/api/v1/invoices/$RS_INV/print-items -H "$AUTH" -H 'Content-Type: application/json' -d "{\"print_type\":\"price_tag\",\"template_id\":\"$TPL\",\"only_changed\":false}")
RS_PR2=$(curl -s -X POST $RS/api/v1/invoices/$RS_INV/print-items -H "$AUTH" -H 'Content-Type: application/json' -d "{\"print_type\":\"price_tag\",\"template_id\":\"$TPL\",\"only_changed\":false}")
META_PY=$(echo "$PY_PR2"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total_labels'],d['total_pages'],d['changed_count'],d['total_count'])")
META_RS=$(echo "$RS_PR2"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total_labels'],d['total_pages'],d['changed_count'],d['total_count'])")
HTML_PY=$(echo "$PY_PR2"|python3 -c "import sys,json; print(json.load(sys.stdin)['html'])"|python3 -c "import sys,re; s=sys.stdin.read(); s=re.sub(r'<svg[^>]*>.*?</svg>','[SVG]',s,flags=re.S); print(len(s),hash(s))")
HTML_RS=$(echo "$RS_PR2"|python3 -c "import sys,json; print(json.load(sys.stdin)['html'])"|python3 -c "import sys,re; s=sys.stdin.read(); s=re.sub(r'<svg[^>]*>.*?</svg>','[SVG]',s,flags=re.S); print(len(s),hash(s))")
if [ "$META_PY" = "$META_RS" ] && [ "$HTML_PY" = "$HTML_RS" ]; then ok "v1 print-items confirmed: структура+HTML(SVG-норм.)"; else bad "v1 print-items confirmed"; echo "  META PY=$META_PY RS=$META_RS"; echo "  HTML PY=$HTML_PY RS=$HTML_RS"; fi
# print labels (sequential)
PY_PL=$(curl -s -X POST $PY/api/v1/invoices/$RS_INV/print-items -H "$AUTH" -H 'Content-Type: application/json' -d "{\"print_type\":\"label\",\"template_id\":\"$TPL\",\"print_mode\":\"system\"}")
RS_PL=$(curl -s -X POST $RS/api/v1/invoices/$RS_INV/print-items -H "$AUTH" -H 'Content-Type: application/json' -d "{\"print_type\":\"label\",\"template_id\":\"$TPL\",\"print_mode\":\"system\"}")
META_PL=$(echo "$PY_PL"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total_labels'],d['total_pages'],d['changed_count'],d['total_count'])")
META_RL=$(echo "$RS_PL"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total_labels'],d['total_pages'],d['changed_count'],d['total_count'])")
if [ "$META_PL" = "$META_RL" ]; then ok "v1 print-items label: структура"; else bad "v1 print-items label"; echo "  PY=$META_PL RS=$META_RL"; fi

# ── 11. v1 DELETE: confirmed → 400; 404; без auth → 401 ────────────────────
PY_D=$(curl -s -w "\n%{http_code}" -X DELETE $PY/api/v1/invoices/$RS_INV -H "$AUTH")
RS_D=$(curl -s -w "\n%{http_code}" -X DELETE $RS/api/v1/invoices/$RS_INV -H "$AUTH")
if [ "$(echo "$PY_D"|tail -1)" = "$(echo "$RS_D"|tail -1)" ] && [ "$(echo "$PY_D"|head -1)" = "$(echo "$RS_D"|head -1)" ]; then ok "v1 delete confirmed→400: parity"; else bad "v1 delete confirmed→400"; echo "  PY: $(echo "$PY_D"|tail -1) $(echo "$PY_D"|head -1)"; echo "  RS: $(echo "$RS_D"|tail -1) $(echo "$RS_D"|head -1)"; fi
PY_NF=$(curl -s -w "\n%{http_code}" $PY/api/v1/invoices/00000000-0000-0000-0000-000000000000 -H "$AUTH")
RS_NF=$(curl -s -w "\n%{http_code}" $RS/api/v1/invoices/00000000-0000-0000-0000-000000000000 -H "$AUTH")
if [ "$(echo "$PY_NF"|tail -1)" = "$(echo "$RS_NF"|tail -1)" ]; then ok "v1 get 404: parity"; else bad "v1 get 404"; fi

# ── 12. v2: LIST search/status/date (parity) ────────────────────────────────
# створимо ще один v2-подібний запис через v1 (Python), щоб було що шукати
PY_V2=$(curl -s -X POST $PY/api/v1/invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"number\":\"DIFF-V2SEARCH-$TS\",\"supplier_id\":\"$SUP\",\"invoice_date\":\"2026-08-07T12:00:00\",\"notes\":\"пошук v2 тест\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":1,\"price\":5,\"total\":5}]}")
PY_V2L=$(curl -s "$PY/api/v2/invoices?search=DIFF-V2SEARCH&size=5" -H "$AUTH")
RS_V2L=$(curl -s "$RS/api/v2/invoices?search=DIFF-V2SEARCH&size=5" -H "$AUTH")
if [ "$(echo "$PY_V2L"|py_norm)" = "$(echo "$RS_V2L"|py_norm)" ]; then ok "v2 list search: parity"; else bad "v2 list search"; echo "  PY: $(echo "$PY_V2L"|head -c 300)"; echo "  RS: $(echo "$RS_V2L"|head -c 300)"; fi
PY_V2S=$(curl -s "$PY/api/v2/invoices?status=draft&size=5" -H "$AUTH"); RS_V2S=$(curl -s "$RS/api/v2/invoices?status=draft&size=5" -H "$AUTH")
if [ "$(echo "$PY_V2S"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'],len(d['items']))")" = "$(echo "$RS_V2S"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'],len(d['items']))")" ]; then ok "v2 list status=draft: total parity"; else bad "v2 list status=draft"; fi
PY_V2D=$(curl -s "$PY/api/v2/invoices?date_from=2026-08-07T00:00:00&date_to=2026-08-07T23:59:59&size=5" -H "$AUTH"); RS_V2D=$(curl -s "$RS/api/v2/invoices?date_from=2026-08-07T00:00:00&date_to=2026-08-07T23:59:59&size=5" -H "$AUTH")
if [ "$(echo "$PY_V2D"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'])")" = "$(echo "$RS_V2D"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['total'])")" ]; then ok "v2 list date-range: total parity"; else bad "v2 list date-range"; fi

# ── 13. v2: GET / UPDATE / DELETE / PAYMENT / PRICE (parity) ────────────────
PY_V2G=$(curl -s $PY/api/v2/invoices/$RS_INV -H "$AUTH"); RS_V2G=$(curl -s $RS/api/v2/invoices/$RS_INV -H "$AUTH")
if [ "$(echo "$PY_V2G"|py_norm)" = "$(echo "$RS_V2G"|py_norm)" ]; then ok "v2 get: parity"; else bad "v2 get"; echo "  PY: $(echo "$PY_V2G"|head -c 200)"; echo "  RS: $(echo "$RS_V2G"|head -c 200)"; fi
PY_V2U=$(curl -s -X PUT $PY/api/v2/invoices/$RS_INV -H "$AUTH" -H 'Content-Type: application/json' -d '{"notes":"v2 updated"}')
RS_V2U=$(curl -s -X PUT $RS/api/v2/invoices/$RS_INV -H "$AUTH" -H 'Content-Type: application/json' -d '{"notes":"v2 updated"}')
if [ "$(echo "$PY_V2U"|py_norm)" = "$(echo "$RS_V2U"|py_norm)" ]; then ok "v2 update scalar: parity"; else bad "v2 update scalar"; echo "  PY: $(echo "$PY_V2U"|head -c 200)"; echo "  RS: $(echo "$RS_V2U"|head -c 200)"; fi
PY_V2P=$(curl -s $PY/api/v2/invoices/$RS_INV/payment-info -H "$AUTH"); RS_V2P=$(curl -s $RS/api/v2/invoices/$RS_INV/payment-info -H "$AUTH")
if [ "$(echo "$PY_V2P" | py_norm)" = "$(echo "$RS_V2P" | py_norm)" ]; then ok "v2 payment-info: parity"; else bad "v2 payment-info"; echo "  PY: $PY_V2P"; echo "  RS: $RS_V2P"; fi
PY_V2PC=$(curl -s $PY/api/v2/invoices/$RS_INV/price-changes -H "$AUTH" | python3 -c "import sys,json; print(json.dumps([{k:(v if k!='product_id' else 'P') for k,v in x.items()} for x in json.load(sys.stdin)],ensure_ascii=False,sort_keys=True))")
RS_V2PC=$(curl -s $RS/api/v2/invoices/$RS_INV/price-changes -H "$AUTH" | python3 -c "import sys,json; print(json.dumps([{k:(v if k!='product_id' else 'P') for k,v in x.items()} for x in json.load(sys.stdin)],ensure_ascii=False,sort_keys=True))")
if [ "$PY_V2PC" = "$RS_V2PC" ]; then ok "v2 price-changes: parity"; else bad "v2 price-changes"; fi

# ── 14. v2 CREATE (Python 500 — Rust робочий) ───────────────────────────────
RS_V2C=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v2/invoices -H "$AUTH" -H 'Content-Type: application/json' -d "{\"number\":\"DIFF-V2C-$TS\",\"supplier_id\":\"$SUP\",\"notes\":\"v2 create rust\",\"items\":[{\"product_id\":\"$PROD\",\"quantity\":2,\"price\":8.5,\"tax_rate\":20,\"name\":\"\"}]}")
CODE=$(echo "$RS_V2C"|tail -1); BODY=$(echo "$RS_V2C"|head -1)
V2C_ID=$(echo "$BODY"|python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
TOTAL=$(echo "$BODY"|python3 -c "import sys,json; print(json.load(sys.stdin).get('total'))" 2>/dev/null || echo "")
if [ "$CODE" = "201" ] && [ "$TOTAL" = "17.0" ]; then ok "v2 create: 201, total=17.0 (задумана семантика; Python 500 — аномалія зафіксована)"; else bad "v2 create"; echo "  code=$CODE body=$(echo "$BODY"|head -c 200)"; fi
# v2 confirm (Python 500 — Rust робочий)
RS_V2CF=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v2/invoices/confirm -H "$AUTH" -H 'Content-Type: application/json' -d "{\"invoice_id\":\"$V2C_ID\"}")
CODE=$(echo "$RS_V2CF"|tail -1); ST=$(echo "$RS_V2CF"|head -1|python3 -c "import sys,json; print(json.load(sys.stdin).get('status'))" 2>/dev/null || echo "")
STOCK2=$($PG -c "SELECT stock FROM products WHERE id='$PROD';")
if [ "$CODE" = "200" ] && [ "$ST" = "confirmed" ] && [ "$STOCK2" = "4.000" ]; then ok "v2 confirm: 200 confirmed, stock=4 (задумана; Python 500)"; else bad "v2 confirm (code=$CODE st=$ST stock=$STOCK2)"; fi
# v2 cancel (Python 500 — Rust робочий)
RS_V2CA=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v2/invoices/$V2C_ID/cancel -H "$AUTH")
CODE=$(echo "$RS_V2CA"|tail -1); ST=$(echo "$RS_V2CA"|head -1|python3 -c "import sys,json; print(json.load(sys.stdin).get('status'))" 2>/dev/null || echo "")
STOCK3=$($PG -c "SELECT stock FROM products WHERE id='$PROD';")
if [ "$CODE" = "200" ] && [ "$ST" = "cancelled" ] && [ "$STOCK3" = "2.000" ]; then ok "v2 cancel: 200 cancelled, stock=2 (задумана; Python 500)"; else bad "v2 cancel (code=$CODE st=$ST stock=$STOCK3)"; fi

# ── 15. Валідації ───────────────────────────────────────────────────────────
PY_BAD=$(curl -s -w "\n%{http_code}" "$PY/api/v1/invoices?size=1001" -H "$AUTH")
RS_BAD=$(curl -s -w "\n%{http_code}" "$RS/api/v1/invoices?size=1001" -H "$AUTH")
if [ "$(echo "$PY_BAD"|tail -1)" = "$(echo "$RS_BAD"|tail -1)" ]; then ok "v1 list size>1000 → 422: parity"; else bad "v1 list size>1000"; echo "  PY=$(echo "$PY_BAD"|tail -1) RS=$(echo "$RS_BAD"|tail -1)"; fi
PY_BAD2=$(curl -s -w "\n%{http_code}" "$PY/api/v1/invoices?page=0" -H "$AUTH")
RS_BAD2=$(curl -s -w "\n%{http_code}" "$RS/api/v1/invoices?page=0" -H "$AUTH")
if [ "$(echo "$PY_BAD2"|tail -1)" = "$(echo "$RS_BAD2"|tail -1)" ]; then ok "v1 list page=0 → 422: parity"; else bad "v1 list page=0"; fi
PY_BAD3=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v1/invoices/$RS_INV/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"bad"}')
RS_BAD3=$(curl -s -w "\n%{http_code}" -X POST $RS/api/v1/invoices/$RS_INV/confirm -H "$AUTH" -H 'Content-Type: application/json' -d '{"status":"bad"}')
if [ "$(echo "$PY_BAD3"|tail -1)" = "$(echo "$RS_BAD3"|tail -1)" ] && [ "$(echo "$PY_BAD3"|head -1)" = "$(echo "$RS_BAD3"|head -1)" ]; then ok "v1 confirm bad status → 400: parity"; else bad "v1 confirm bad status"; fi

# ── 16. Cleanup ─────────────────────────────────────────────────────────────
psql -h localhost -U postgres -d pos_system -q << SQL
DELETE FROM supplier_ledger WHERE document_id IN ('$RS_INV','$PY_INV','$V2C_ID');
DELETE FROM invoice_items WHERE invoice_id IN ('$RS_INV','$PY_INV','$V2C_ID');
DELETE FROM invoices WHERE id IN ('$RS_INV','$PY_INV','$V2C_ID') OR number LIKE 'DIFF-PY-$TS%' OR number LIKE 'DIFF-RS-$TS%' OR number LIKE 'DIFF-V2%';
DELETE FROM products WHERE id='$PROD';
DELETE FROM suppliers WHERE id='$SUP';
SQL
log "ПІДСУМОК: PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = "0" ] && echo "E2E INVOICES: ALL PASS ✅" || echo "E2E INVOICES: FAILURES ❌"
exit $FAIL
