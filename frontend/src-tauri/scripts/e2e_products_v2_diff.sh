#!/usr/bin/env bash
# ============================================================================
# e2e_products_v2_diff.sh — differential-тест товарів v2 (етап 8, група 7).
# Rust :8002 (KASA_RUST_PRODUCTS_V2=1) vs Python :8001 (еталон), СПІЛЬНА БД.
#
# Покриває v2/products.py (10 роутів):
#   GET  /api/v2/products (list: search, category_id, пагінація, 422)
#   GET  /api/v2/products/barcode/{barcode} (основний + додатковий, 404)
#   GET  /api/v2/products/{id} (404)
#   POST /api/v2/products (201; 400 дублікат barcode/sku; 422 name/barcode/price)
#   PUT  /api/v2/products/{id} (404/400 дублікат; 422)
#   DELETE /api/v2/products/{id} (204; 404; 400 stock != 0 — Python float detail)
#   POST /api/v2/products/{id}/images (multipart file+is_main; 404; url+файл)
#   DELETE /api/v2/products/{id}/images/{image_id} (204; 404)
#   POST /api/v2/products/{id}/barcodes (200; 404 товар; 409 дублікат; 422)
#   DELETE /api/v2/products/{id}/barcodes/{barcode_id} (204; 404)
#   GET  /uploads/products/{id}/{file} (static serve: тіло + content-type 1:1)
#
# id/datetime нормалізуються; решта — exact (спільна БД).
# Тестові дані (товари, категорія, файли зображень) видаляються в кінці.
# ============================================================================
set -u
PY=http://127.0.0.1:8001
RS=http://127.0.0.1:8002
TOKEN=$(cat /tmp/kasa_token 2>/dev/null || echo "")
AUTH="Authorization: Bearer $TOKEN"
PASS=0; FAIL=0
TS=$(date +%s)
UQ="e2ep7_${TS}"
IMG_BASE="/tmp/e2e_p7_img_${TS}"

log()  { echo "[$(date +%H:%M:%S)] $*"; }
ok()   { PASS=$((PASS+1)); echo "  ✅ $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

py_norm() {
python3 -c "
import sys,json
def n(x):
    if isinstance(x,dict):
        return {k:('<id>' if k in ('id','product_id') and isinstance(v,str) and len(v)==36 else '<dt>' if k=='created_at' else '<url>' if k=='url' else n(v)) for k,v in x.items()}
    if isinstance(x,list): return [n(v) for v in x]
    return x
print(json.dumps(n(json.load(sys.stdin)),ensure_ascii=False,sort_keys=True))
"
}
py_exact() {
python3 -c "
import sys,json
print(json.dumps(json.load(sys.stdin),ensure_ascii=False,sort_keys=True))
"
}

# ─── 1. List parity (спільна БД) ────────────────────────────────────────────
log "LIST: базовий"
PY_L=$(curl -s "$PY/api/v2/products?page=1&size=5" -H "$AUTH")
RS_L=$(curl -s "$RS/api/v2/products?page=1&size=5" -H "$AUTH")
if [ "$(echo "$PY_L"|py_norm)" = "$(echo "$RS_L"|py_norm)" ]; then ok "v2 list page=1 size=5 parity"; else bad "v2 list base"; fi

# ─── 2. Create (через PY) → спільний товар A ────────────────────────────────
log "CREATE: PY 201, RS GET exact"
PY_C=$(curl -s -X POST "$PY/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' \
  -d "{\"name\":\"Товар P7 A ${UQ}\",\"barcode\":\"${UQ}A\",\"price\":150.5,\"cost_price\":100,\"quantity\":5,\"sku\":\"SKU${UQ}A\",\"description\":\"desc A\"}")
PID_A=$(echo "$PY_C"|python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
if [ "$(echo "$PY_C"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['name'],d['barcode'],d['price'],d['quantity'],d['unit'],d['is_active'],d['sku'],d['description'])" 2>/dev/null)" = "Товар P7 A ${UQ} ${UQ}A 150.5 5.0 шт True SKU${UQ}A desc A" ]; then ok "PY create 201 поля"; else bad "PY create поля"; fi
PY_G=$(curl -s "$PY/api/v2/products/$PID_A" -H "$AUTH")
RS_G=$(curl -s "$RS/api/v2/products/$PID_A" -H "$AUTH")
if [ "$(echo "$PY_G"|py_exact)" = "$(echo "$RS_G"|py_exact)" ]; then ok "v2 get exact parity (товар A)"; else bad "v2 get parity"; fi

# ─── 3. Create через RS — 201 + дублікат 400 ────────────────────────────────
log "CREATE RS: 201 + дублікат 400"
RS_C=$(curl -s -X POST "$RS/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' \
  -d "{\"name\":\"Товар P7 B ${UQ}\",\"barcode\":\"${UQ}B\",\"price\":99.99,\"quantity\":0}")
PID_B=$(echo "$RS_C"|python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
RS_CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RS/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"name\":\"x\",\"barcode\":\"${UQ}B\"}")
if [ "$RS_CODE" = "400" ]; then ok "RS create дублікат barcode → 400"; else bad "RS create dup 400 (got $RS_CODE)"; fi
RS_DUP=$(curl -s -X POST "$RS/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"name\":\"x\",\"barcode\":\"${UQ}B\"}")
PY_DUP=$(curl -s -X POST "$PY/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"name\":\"x\",\"barcode\":\"${UQ}B\"}")
if [ "$(echo "$RS_DUP"|py_exact)" = "$(echo "$PY_DUP"|py_exact)" ]; then ok "create дублікат barcode detail parity"; else bad "dup detail"; fi

# ─── 4. Create 422 (exact Pydantic) ──────────────────────────────────────────
log "CREATE 422: name/barcode/price"
for body in '{"name":""}' '{"barcode":""}' '{"price":0}' "{\"name\":\"$(printf 'a%.0s' {1..256})\"}"; do
  R1=$(curl -s -X POST "$RS/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d "$body")
  R2=$(curl -s -X POST "$PY/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d "$body")
  if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "create 422 parity: $body"; else bad "create 422: $body"; echo "   RS: $R1"; echo "   PY: $R2"; fi
done
R1=$(curl -s -X POST "$RS/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d '{"barcode":"123"}')
R2=$(curl -s -X POST "$PY/api/v2/products" -H "$AUTH" -H 'Content-Type: application/json' -d '{"barcode":"123"}')
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "create name missing 422 parity"; else bad "missing name"; fi

# ─── 5. List: search + 422 + 400 ─────────────────────────────────────────────
log "LIST: search/422/400"
PY_S=$(curl -s "$PY/api/v2/products?search=${UQ}A" -H "$AUTH")
RS_S=$(curl -s "$RS/api/v2/products?search=${UQ}A" -H "$AUTH")
if [ "$(echo "$PY_S"|py_norm)" = "$(echo "$RS_S"|py_norm)" ] && [ "$(echo "$PY_S"|python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")" = "1" ]; then ok "list search parity (total=1)"; else bad "list search"; fi
R1=$(curl -s "$RS/api/v2/products?page=0" -H "$AUTH"); R2=$(curl -s "$PY/api/v2/products?page=0" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "list 422 page=0 parity"; else bad "422 page=0"; fi
R1=$(curl -s "$RS/api/v2/products?size=0" -H "$AUTH"); R2=$(curl -s "$PY/api/v2/products?size=0" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "list 422 size=0 parity"; else bad "422 size=0"; fi
R1=$(curl -s "$RS/api/v2/products?size=101" -H "$AUTH"); R2=$(curl -s "$PY/api/v2/products?size=101" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "list 422 size=101 parity"; else bad "422 size=101"; fi
R1=$(curl -s "$RS/api/v2/products?category_id=abc" -H "$AUTH"); R2=$(curl -s "$PY/api/v2/products?category_id=abc" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "list category_id невалідний 400 parity"; else bad "400 category"; fi

# ─── 6. Категорія → list category_id parity ──────────────────────────────────
log "LIST: category_id фільтр"
CAT_ID=$(curl -s -X POST "$PY/api/v1/categories" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"name\":\"Cat ${UQ}\"}" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))")
if [ -n "$CAT_ID" ]; then
  curl -s -X PUT "$PY/api/v1/products/$PID_A" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"category_id\":\"$CAT_ID\"}" >/dev/null
  PY_CAT=$(curl -s "$PY/api/v2/products?category_id=$CAT_ID" -H "$AUTH")
  RS_CAT=$(curl -s "$RS/api/v2/products?category_id=$CAT_ID" -H "$AUTH")
  if [ "$(echo "$PY_CAT"|py_norm)" = "$(echo "$RS_CAT"|py_norm)" ] && [ "$(echo "$PY_CAT"|python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")" = "1" ]; then ok "list category_id parity (total=1)"; else bad "list category_id"; fi
else
  bad "category create"
fi

# ─── 7. Update: parity + 404 + 400 + 422 ─────────────────────────────────────
log "UPDATE: parity + 404 + 400 + 422"
R1=$(curl -s -X PUT "$RS/api/v2/products/$PID_A" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"name\":\"Товар P7 A ${UQ} upd\",\"price\":175.25}")
R2=$(curl -s -X PUT "$PY/api/v2/products/$PID_A" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"name\":\"Товар P7 A ${UQ} upd\",\"price\":175.25}")
if [ "$(echo "$R1"|py_norm)" = "$(echo "$R2"|py_norm)" ]; then ok "update parity (normalized)"; else bad "update parity"; fi
NOPE="00000000-0000-0000-0000-000000000000"
R1=$(curl -s -X PUT "$RS/api/v2/products/$NOPE" -H "$AUTH" -H 'Content-Type: application/json' -d '{"name":"x"}')
R2=$(curl -s -X PUT "$PY/api/v2/products/$NOPE" -H "$AUTH" -H 'Content-Type: application/json' -d '{"name":"x"}')
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "update 404 parity"; else bad "update 404"; fi
R1=$(curl -s -X PUT "$RS/api/v2/products/$PID_B" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}A\"}")
R2=$(curl -s -X PUT "$PY/api/v2/products/$PID_B" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}A\"}")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "update дублікат barcode 400 parity"; else bad "update dup 400"; fi
R1=$(curl -s -X PUT "$RS/api/v2/products/$PID_A" -H "$AUTH" -H 'Content-Type: application/json' -d '{"price":0}')
R2=$(curl -s -X PUT "$PY/api/v2/products/$PID_A" -H "$AUTH" -H 'Content-Type: application/json' -d '{"price":0}')
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "update 422 price=0 parity"; else bad "update 422"; fi

# ─── 8. Barcode search (основний + 404) ─────────────────────────────────────
log "BARCODE search"
R1=$(curl -s "$RS/api/v2/products/barcode/${UQ}A" -H "$AUTH")
R2=$(curl -s "$PY/api/v2/products/barcode/${UQ}A" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "barcode search exact parity"; else bad "barcode search"; fi
R1=$(curl -s "$RS/api/v2/products/barcode/NOPE${UQ}" -H "$AUTH"); R2=$(curl -s "$PY/api/v2/products/barcode/NOPE${UQ}" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "barcode 404 parity"; else bad "barcode 404"; fi

# ─── 9. Images: upload PY + RS, serve, delete ────────────────────────────────
log "IMAGES: upload/serve/delete"
printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82' > "${IMG_BASE}.png"
PY_UP=$(curl -s -X POST "$PY/api/v2/products/$PID_A/images" -H "$AUTH" -F "file=@${IMG_BASE}.png;type=image/png" -F "is_main=true")
IMG_URL=$(echo "$PY_UP"|python3 -c "import sys,json; print(json.load(sys.stdin)['url'])")
if echo "$IMG_URL" | grep -qE "^/uploads/products/$PID_A/[0-9a-f-]{36}\.png$"; then ok "PY upload url формат"; else bad "PY upload url ($IMG_URL)"; fi
RS_UP=$(curl -s -X POST "$RS/api/v2/products/$PID_B/images" -H "$AUTH" -F "file=@${IMG_BASE}.png;type=image/png" -F "is_main=true")
if [ "$(echo "$PY_UP"|py_norm)" = "$(echo "$RS_UP"|py_norm)" ]; then ok "upload структура parity (normalized)"; else bad "upload parity"; echo "  PY: $PY_UP"; echo "  RS: $RS_UP"; fi
PY_SRV=$(curl -s "$PY$IMG_URL" | md5sum | cut -d' ' -f1)
RS_SRV=$(curl -s "$RS$IMG_URL" | md5sum | cut -d' ' -f1)
if [ "$PY_SRV" = "$RS_SRV" ] && [ "$PY_SRV" != "" ]; then ok "serve файл parity (md5)"; else bad "serve parity"; fi
PY_CT=$(curl -s -o /dev/null -w '%{content_type}' "$PY$IMG_URL")
RS_CT=$(curl -s -o /dev/null -w '%{content_type}' "$RS$IMG_URL")
if [ "$PY_CT" = "$RS_CT" ] && [ "$PY_CT" = "image/png" ]; then ok "serve content-type image/png parity"; else bad "content-type ($PY_CT vs $RS_CT)"; fi
MAIN1=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT is_main FROM product_images WHERE product_id='$PID_A' AND url='$IMG_URL'")
if [ "$MAIN1" = "t" ]; then ok "is_main=true збережено"; else bad "is_main ($MAIN1)"; fi
R1=$(curl -s -X POST "$RS/api/v2/products/$NOPE/images" -H "$AUTH" -F "file=@${IMG_BASE}.png")
R2=$(curl -s -X POST "$PY/api/v2/products/$NOPE/images" -H "$AUTH" -F "file=@${IMG_BASE}.png")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "upload 404 parity"; else bad "upload 404"; fi
IMG2_ID=$(echo "$RS_UP"|python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
RC=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$RS/api/v2/products/$PID_B/images/$IMG2_ID" -H "$AUTH")
if [ "$RC" = "204" ]; then ok "delete image RS → 204"; else bad "delete image ($RC)"; fi
R1=$(curl -s -X DELETE "$RS/api/v2/products/$PID_B/images/$NOPE" -H "$AUTH")
R2=$(curl -s -X DELETE "$PY/api/v2/products/$PID_B/images/$NOPE" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "delete image 404 parity"; else bad "delete image 404"; fi
RS_URL2=$(echo "$RS_UP"|python3 -c "import sys,json; print(json.load(sys.stdin)['url'])")
RC=$(curl -s -o /dev/null -w '%{http_code}' "$RS$RS_URL2")
if [ "$RC" = "200" ]; then ok "serve після delete → 200 (файл лишається, як Python)"; else bad "serve deleted ($RC)"; fi

# ─── 10. Barcodes: add PY+RS, dup 409, search, delete ────────────────────────
log "BARCODES: add/dup/delete"
BC_ADD_PY=$(curl -s -X POST "$PY/api/v2/products/$PID_A/barcodes" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}BC1\",\"is_primary\":true}")
BC_ADD_RS=$(curl -s -X POST "$RS/api/v2/products/$PID_B/barcodes" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}BC2\",\"is_primary\":true}")
BC_PY_PID=$(echo "$BC_ADD_PY"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['product_id'],d['is_primary'],d['barcode'])")
BC_RS_PID=$(echo "$BC_ADD_RS"|python3 -c "import sys,json; d=json.load(sys.stdin); print(d['product_id'],d['is_primary'],d['barcode'])")
if [ "$BC_PY_PID" = "$PID_A True ${UQ}BC1" ]; then ok "PY add barcode поля (product_id/is_primary/barcode)"; else bad "PY add barcode ($BC_PY_PID)"; fi
if [ "$BC_RS_PID" = "$PID_B True ${UQ}BC2" ]; then ok "RS add barcode поля (product_id/is_primary/barcode)"; else bad "RS add barcode ($BC_RS_PID)"; fi
R1=$(curl -s -X POST "$RS/api/v2/products/$PID_A/barcodes" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}BC1\"}")
R2=$(curl -s -X POST "$PY/api/v2/products/$PID_A/barcodes" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}BC1\"}")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "add barcode дублікат 409 parity"; else bad "409 dup"; fi
R1=$(curl -s -X POST "$RS/api/v2/products/$NOPE/barcodes" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}BCX\"}")
R2=$(curl -s -X POST "$PY/api/v2/products/$NOPE/barcodes" -H "$AUTH" -H 'Content-Type: application/json' -d "{\"barcode\":\"${UQ}BCX\"}")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "add barcode 404 parity"; else bad "add barcode 404"; fi
R1=$(curl -s "$RS/api/v2/products/barcode/${UQ}BC1" -H "$AUTH")
R2=$(curl -s "$PY/api/v2/products/barcode/${UQ}BC1" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "barcode search додатковий parity"; else bad "додатковий search"; fi
BC_ID=$(echo "$BC_ADD_RS"|python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
RC=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$RS/api/v2/products/$PID_A/barcodes/$BC_ID" -H "$AUTH")
if [ "$RC" = "204" ]; then ok "delete barcode RS → 204"; else bad "delete barcode ($RC)"; fi
R1=$(curl -s -X DELETE "$RS/api/v2/products/$PID_A/barcodes/$NOPE" -H "$AUTH")
R2=$(curl -s -X DELETE "$PY/api/v2/products/$PID_A/barcodes/$NOPE" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "delete barcode 404 parity"; else bad "delete barcode 404"; fi

# ─── 11. Delete: 204, 404, 400 stock ─────────────────────────────────────────
log "DELETE: 204/404/400 stock"
RC=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$RS/api/v2/products/$PID_B" -H "$AUTH")
if [ "$RC" = "204" ]; then ok "delete RS → 204"; else bad "delete 204 ($RC)"; fi
R1=$(curl -s -X DELETE "$RS/api/v2/products/$PID_B" -H "$AUTH")
R2=$(curl -s -X DELETE "$PY/api/v2/products/$PID_B" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "delete 404 parity"; else bad "delete 404"; fi
R1=$(curl -s -X DELETE "$RS/api/v2/products/$PID_A" -H "$AUTH")
R2=$(curl -s -X DELETE "$PY/api/v2/products/$PID_A" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "delete stock!=0 400 parity"; else bad "delete stock"; echo "  RS: $R1"; echo "  PY: $R2"; fi

# ─── 12. get 404 ─────────────────────────────────────────────────────────────
R1=$(curl -s "$RS/api/v2/products/$NOPE" -H "$AUTH"); R2=$(curl -s "$PY/api/v2/products/$NOPE" -H "$AUTH")
if [ "$(echo "$R1"|py_exact)" = "$(echo "$R2"|py_exact)" ]; then ok "get 404 parity"; else bad "get 404"; fi

# ─── Cleanup ─────────────────────────────────────────────────────────────────
log "CLEANUP: тестові дані (товари/категорія/зображення)"
curl -s -X PUT "$PY/api/v1/products/$PID_A" -H "$AUTH" -H 'Content-Type: application/json' -d '{"stock":"0"}' >/dev/null
curl -s -X DELETE "$RS/api/v2/products/$PID_A" -H "$AUTH" >/dev/null
[ -n "${CAT_ID:-}" ] && curl -s -X DELETE "$PY/api/v1/categories/$CAT_ID" -H "$AUTH" >/dev/null
python3 - "$PID_A" "$IMG_BASE" << 'PYEOF'
import os, sys
pid, imgbase = sys.argv[1], sys.argv[2]
base = os.environ.get("KASA_UPLOADS_DIR", "uploads")
d = os.path.join(base, "products", pid)
if os.path.isdir(d):
    for f in os.listdir(d):
        p = os.path.join(d, f)
        try:
            os.unlink(p)
        except OSError:
            pass
    print(f"  файли uploads/products/{pid}/ видалено")
for suf in (".png",):
    p = imgbase + suf
    if os.path.exists(p):
        os.unlink(p)
PYEOF
CNT=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT count(*) FROM products WHERE barcode LIKE '${UQ}%' OR sku LIKE 'SKU${UQ}%'")
if [ "$CNT" = "0" ]; then ok "cleanup БД: товарів 0"; else bad "cleanup БД (left $CNT)"; fi
CNT2=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT count(*) FROM product_images pi JOIN products p ON p.id=pi.product_id WHERE p.barcode LIKE '${UQ}%'")
if [ "$CNT2" = "0" ]; then ok "cleanup БД: зображень 0"; else bad "cleanup images ($CNT2)"; fi

# ─── Підсумок ────────────────────────────────────────────────────────────────
log "ПІДСУМОК: PASS=$PASS FAIL=$FAIL"
if [ "$FAIL" = "0" ]; then echo "E2E PRODUCTS_V2: ALL PASS ✅"; else echo "E2E PRODUCTS_V2: FAILURES ❌"; fi
exit $FAIL
