#!/usr/bin/env bash
# e2e_ocr_diff.sh — differential-тест OCR (етап 8, група 9/9).
set -u
PY=http://127.0.0.1:8003
RS=http://127.0.0.1:8002
PASS=0; FAIL=0
TS=$(date +%s)
UQ="e2eocr_${TS}"
BACKEND="/home/anastasia/Andriy/aegis_v3/Niko/Projects/torgashka/backend"
TAURI_DIR="/home/anastasia/Andriy/aegis_v3/Niko/Projects/torgashka/frontend/src-tauri"
IMG=/tmp/e2eocr_invoice.png
KEY_FILE="$BACKEND/keys.txt"
log()  { echo "[$(date +%H:%M:%S)] $*"; }
ok()   { PASS=$((PASS+1)); echo "  OK $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  BAD $1"; }


# 1:1 JSON (сортування ключів + числа 10.0→10)
py_norm_num() {
python3 -c "
import sys,json
def n(x):
    if isinstance(x,dict): return {k:n(v) for k,v in x.items()}
    if isinstance(x,list): return [n(v) for v in x]
    if isinstance(x,float) and x.is_integer(): return int(x)
    return x
print(json.dumps(n(json.load(sys.stdin)),ensure_ascii=False,sort_keys=True))
"
}
echo "part1 ok"

log "Підготовка процесів"
pkill -f "mock_gemini.py" 2>/dev/null; sleep 0.3
pkill -f "app.main:app --host 127.0.0.1 --port 8003" 2>/dev/null; sleep 0.3
pkill -f "target/debug/facade" 2>/dev/null; sleep 0.3
nohup python3 /tmp/mock_gemini.py > /tmp/mock_gemini.log 2>&1 &
sleep 1
log "мок Gemini :5099"
(cd "$BACKEND" && GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:5099/ \
  nohup venv/bin/python -m uvicorn app.main:app --host 127.0.0.1 --port 8003 > /tmp/torgashka_py_8003.log 2>&1 &)
sleep 5
(cd "$TAURI_DIR" && TORGASHKA_RUST_OCR=1 TORGASHKA_FACADE_ADDR=127.0.0.1:8002 \
  TORGASHKA_OCR_BASE_URL=http://127.0.0.1:5099/ TORGASHKA_OCR_KEYS_FILE="$KEY_FILE" \
  nohup ./target/debug/facade > /tmp/torgashka_facade_8002.log 2>&1 &)
sleep 3
echo "part2 ok"

TOKEN=$(curl -s -X POST "$PY/api/v1/auth/login" -H "Content-Type: application/json" \
  -d '{"login":"admin","password":"admin123"}' | python3 -c "import sys,json; print(json.load(sys.stdin).get('access_token',''))" 2>/dev/null)
AUTH="Authorization: Bearer $TOKEN"
PID_M=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT gen_random_uuid()" | tr -d ' ')
PID_B=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT gen_random_uuid()" | tr -d ' ')
BC_OCR="482$(date +%s)"
BC2="483$(date +%s)"
TITLE2="Хліб Білий $UQ"
PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q << SQL
INSERT INTO products (id, title, sku, barcode, price, cost_price, stock, fiscal_stock, tax_rate, markup, created_at, updated_at)
VALUES ('$PID_M', 'Молоко 2.6% с/п ТМ Селянське', 'SKU-M-$UQ', '$BC_OCR', 45.50, 35.00, 50, 0, 20, 20.00, now(), now());
INSERT INTO products (id, title, sku, barcode, price, cost_price, stock, fiscal_stock, tax_rate, markup, created_at, updated_at)
VALUES ('$PID_B', '$TITLE2', 'SKU-B-$UQ', NULL, 32.00, 20.00, 100, 0, 20, 15.00, now(), now());
INSERT INTO barcodes (product_id, barcode, is_primary, created_at, updated_at)
VALUES ('$PID_M', '$BC2', false, now(), now());
SQL
log "товари: молоко=$PID_M хліб=$PID_B bc=$BC_OCR bc2=$BC2"
printf "%s" "$BC_OCR" > /tmp/mock_ocr_bc
printf "%s" "$BC2" > /tmp/mock_ocr_bc2
printf "%s" "$TITLE2" > /tmp/mock_ocr_title
echo "part3 ok"

python3 - << 'PYEOF'
import struct, zlib
def png_1x1():
    sig = b'\x89PNG\r\n\x1a\n'
    def chunk(t, d):
        c = struct.pack('>I', len(d)) + t + d
        return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    ihdr = struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0)
    idat = zlib.compress(b'\x00\xff\x00\x00')
    return sig + chunk(b'IHDR', ihdr) + chunk(b'IDAT', idat) + chunk(b'IEND', b'')
open('/tmp/e2eocr_invoice.png', 'wb').write(png_1x1())
print("PNG створено")
PYEOF
echo "part4 ok"

PY_O=$(curl -s -X POST "$PY/api/v1/ocr/invoice" -H "$AUTH" -F "file=@$IMG;type=image/png")
RS_O=$(curl -s -X POST "$RS/api/v1/ocr/invoice" -H "$AUTH" -F "file=@$IMG;type=image/png")
if [ "$(echo "$PY_O"|py_norm_num)" = "$(echo "$RS_O"|py_norm_num)" ]; then ok "ocr/invoice exact parity"; else bad "ocr/invoice"; echo "  PY: $PY_O"; echo "  RS: $RS_O"; fi
PY_A=$(curl -s -X POST "$PY/api/v1/invoice-ocr/analyze" -H "$AUTH" -F "file=@$IMG;type=image/png")
RS_A=$(curl -s -X POST "$RS/api/v1/invoice-ocr/analyze" -H "$AUTH" -F "file=@$IMG;type=image/png")
if [ "$(echo "$PY_A"|py_norm_num)" = "$(echo "$RS_A"|py_norm_num)" ]; then ok "invoice-ocr/analyze parity"; else bad "invoice-ocr/analyze"; echo "  PY: $(echo "$PY_A"|py_norm_num)"; echo "  RS: $(echo "$RS_A"|py_norm_num)"; fi
echo "part5 ok"

log "Очищення"
PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q << SQL
DELETE FROM barcodes WHERE product_id IN ('$PID_M','$PID_B');
DELETE FROM products WHERE id IN ('$PID_M','$PID_B');
SQL
pkill -f "mock_gemini.py" 2>/dev/null
pkill -f "app.main:app --host 127.0.0.1 --port 8003" 2>/dev/null
pkill -f "target/debug/facade" 2>/dev/null
echo ""
log "РЕЗУЛЬТАТ: PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = "0" ]
