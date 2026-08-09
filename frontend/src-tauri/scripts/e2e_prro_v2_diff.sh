#!/usr/bin/env bash
# ============================================================================
# e2e_prro_v2_diff.sh — differential-тест ПРРО v2 (етап 8, група 8/9).
# Rust :8002 (TORGASHKA_RUST_PRRO_V2=1) vs Python :8003 (еталон, PRRO env),
# СПІЛЬНА БД + МОК gRPC ChkIncomeService :50051.
#
# Покриває (v2/prro.py — решта вже в 7.3 fiscal/*):
#   GET  /api/v2/prro/settings (паритет, пароль замасковано)
#   PUT  /api/v2/prro/settings (multipart: key_file+key_password+реквізити;
#        запис у backend/.prro_keystore.json — Fernet-сумісність PY<->RS)
#   POST /api/v2/prro/test-connection (ping на мок gRPC, status=1)
#   POST /api/v2/prro/receipts/{id}/fiscalize (sendChkV2 на мок gRPC:
#        real ДСТУ-підпис JKS+test2003, queue sent, лічильники, QR url)
#   + БД: receipts.fiscal_status='sent', prro_queue status='sent',
#     products.fiscal_stock зменшено.
#
# Динамічні значення (id/datetime/fiscal_number/url) нормалізуються.
# Тестові дані видаляються в кінці.
# ============================================================================
set -u
PY=http://127.0.0.1:8003
RS=http://127.0.0.1:8002
TOKEN=$(cat /tmp/torgashka_token 2>/dev/null || echo "")
AUTH="Authorization: Bearer $TOKEN"
PASS=0; FAIL=0
TS=$(date +%s)
UQ="e2eprro_${TS}"
KEY_SRC="/home/anastasia/Andriy/aegis_v3/Niko/Projects/torgashka/backend/certs/prro-test/pb_3791505547 (2).jks"
BACKEND="/home/anastasia/Andriy/aegis_v3/Niko/Projects/torgashka/backend"
TAURI_DIR="/home/anastasia/Andriy/aegis_v3/Niko/Projects/torgashka/frontend/src-tauri"

log()  { echo "[$(date +%H:%M:%S)] $*"; }
ok()   { PASS=$((PASS+1)); echo "  ✅ $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

py_norm() {
python3 -c "
import sys,json
DYN={'receipt_id','id','fiscal_number','fiscal_sent_at','fiscal_date','fiscal_check_url','created_at','opened_at','sent_at'}
def n(x):
    if isinstance(x,dict): return {k:('<id>' if k in DYN and isinstance(v,str) else '<dt>' if k in ('fiscal_sent_at','fiscal_date') and v else '<url>' if k=='fiscal_check_url' and v else n(v)) for k,v in x.items()}
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
py_norm2() {
python3 -c "
import sys,json
def n(x):
    if isinstance(x,dict):
        r={}
        for k,v in x.items():
            if k=='key_file': r[k]='<key_file>'
            elif k in ('fiscal_sent_at','fiscal_date'): r[k]='<dt>' if v else v
            elif k=='fiscal_check_url': r[k]='<url>' if v else v
            elif k in ('fiscal_number','receipt_id'): r[k]='<id>' if v else v
            else: r[k]=n(v)
        return r
    if isinstance(x,list): return [n(v) for v in x]
    return x
print(json.dumps(n(json.load(sys.stdin)),ensure_ascii=False,sort_keys=True))
"
}

# ─── 0. Процеси ──────────────────────────────────────────────────────────────
log "Підготовка процесів"
pkill -f "mock_prro_server.py" 2>/dev/null; sleep 0.3
pkill -f "app.main:app --host 127.0.0.1 --port 8003" 2>/dev/null; sleep 0.3
pkill -f "target/debug/facade" 2>/dev/null; sleep 0.3
PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q -c "DELETE FROM prro_settings WHERE key_name='prro_stub_mode'" 2>/dev/null

nohup python3 /tmp/mock_prro_server.py 50051 > /tmp/mock_prro.log 2>&1 &
sleep 1
log "мок gRPC :50051 — $(ss -tln | grep -c 50051) портів"

(cd "$BACKEND" && PRRO_TEST_URL=127.0.0.1:50051 PRRO_USE_SSL=false \
  nohup venv/bin/python -m uvicorn app.main:app --host 127.0.0.1 --port 8003 \
  > /tmp/torgashka_py_8003.log 2>&1 &)
sleep 5
log "Python :8003 — $(ss -tln | grep -c 8003) портів"

(cd "$TAURI_DIR" && TORGASHKA_RUST_PRRO=1 TORGASHKA_RUST_PRRO_V2=1 \
  TORGASHKA_FACADE_ADDR=127.0.0.1:8002 \
  PRRO_TEST_URL=127.0.0.1:50051 PRRO_GRPC_INSECURE=1 \
  PRRO_KEYSTORE_PATH="$BACKEND/app/infrastructure/.prro_keystore.json" \
  PRRO_MASTER_KEY_PATH="$BACKEND/app/infrastructure/.prro_master.key" \
  PRRO_CERTS_DIR="$BACKEND/certs" \
  nohup ./target/debug/facade > /tmp/torgashka_facade_8002.log 2>&1 &)
sleep 3
log "Rust :8002 — $(ss -tln | grep -c 8002) портів"

# ─── 1. GET /settings (порожні) ─────────────────────────────────────────────
log "SETTINGS GET (без конфігурації)"
PY_S=$(curl -s "$PY/api/v2/prro/settings" -H "$AUTH")
RS_S=$(curl -s "$RS/api/v2/prro/settings" -H "$AUTH")
if [ "$(echo "$PY_S"|py_norm2)" = "$(echo "$RS_S"|py_norm2)" ]; then ok "GET settings parity (порожні)"; else bad "GET settings"; echo "  PY: $PY_S"; echo "  RS: $RS_S"; fi

# ─── 2. PUT /settings через PY ──────────────────────────────────────────────
log "SETTINGS PUT (PY, multipart key_file)"
PY_PUT=$(curl -s -X PUT "$PY/api/v2/prro/settings" -H "$AUTH" \
  -F "key_file=@${KEY_SRC};filename=pb_test.jks" \
  -F "key_password=test2003" \
  -F "prro_fn=4000000001" \
  -F "prro_tn=1234567890" \
  -F "prro_zn=ZN-TEST-01" \
  -F "mode=test" \
  -F "auto_fiscalize=true")
echo "$PY_PUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print('  PUT ok:', d.get('key_password_masked'), d.get('prro_fn'), 'mode='+str(d.get('mode')), 'auto='+str(d.get('auto_fiscalize')))" 2>/dev/null || echo "  PUT: $PY_PUT"

PY_S2=$(curl -s "$PY/api/v2/prro/settings" -H "$AUTH")
RS_S2=$(curl -s "$RS/api/v2/prro/settings" -H "$AUTH")
if [ "$(echo "$PY_S2"|py_norm2)" = "$(echo "$RS_S2"|py_norm2)" ]; then ok "GET settings parity після PUT (keystore спільний)"; else bad "GET settings після PUT"; echo "  PY: $(echo "$PY_S2"|py_norm2)"; echo "  RS: $(echo "$RS_S2"|py_norm2)"; fi

# ─── 3. POST /test-connection ───────────────────────────────────────────────
log "TEST-CONNECTION (ping → мок :50051)"
PY_T=$(curl -s -X POST "$PY/api/v2/prro/test-connection" -H "$AUTH")
RS_T=$(curl -s -X POST "$RS/api/v2/prro/test-connection" -H "$AUTH")
if [ "$(echo "$PY_T"|py_exact)" = "$(echo "$RS_T"|py_exact)" ]; then ok "test-connection exact parity"; else
  PY_OK=$(echo "$PY_T" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d['status'],d['ok'])")
  RS_OK=$(echo "$RS_T" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d['status'],d['ok'])")
  if [ "$PY_OK" = "$RS_OK" ] && [ "$PY_OK" = "1 True" ]; then ok "test-connection parity (status=1 ok=true)"; else bad "test-connection"; echo "  PY: $PY_T"; echo "  RS: $RS_T"; fi
fi

# ─── 4. Дані: зміна + 2 чеки + товар ────────────────────────────────────────
log "Підготовка БД (зміна + чеки)"
CASHIER=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT id FROM users LIMIT 1" | tr -d ' ')
PID=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT gen_random_uuid()" | tr -d ' ')
RID_A=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT gen_random_uuid()" | tr -d ' ')
RID_B=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT gen_random_uuid()" | tr -d ' ')

PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q << SQL
INSERT INTO products (id, title, sku, barcode, price, cost_price, stock, fiscal_stock, tax_rate, created_at, updated_at)
VALUES ('$PID', 'Товар P8 $UQ', 'SKU$UQ', 'e2eprro_$TS', 50.00, 30.00, 100, 100, 20, now(), now());
INSERT INTO receipts (id, receipt_number, receipt_type, cashier_id, total_amount, paid_amount, change_amount, is_return, payment_method, cash_amount, card_amount, fiscal_status, is_fiscal, created_at)
VALUES ('$RID_A', 'P8A-$TS', 'sale', '$CASHIER', 100.00, 100.00, 0.00, false, 'cash', 100.00, 0.00, 'pending', false, now()),
       ('$RID_B', 'P8B-$TS', 'sale', '$CASHIER', 100.00, 100.00, 0.00, false, 'cash', 100.00, 0.00, 'pending', false, now());
INSERT INTO receipt_items (id, receipt_id, product_id, quantity, price, total, purchase_price, fiscal_quantity, created_at)
VALUES (gen_random_uuid(), '$RID_A', '$PID', 2, 50.00, 100.00, 30.00, 2, now()),
       (gen_random_uuid(), '$RID_B', '$PID', 2, 50.00, 100.00, 30.00, 2, now());
SQL
log "  product=$PID cashier=$CASHIER receiptA=$RID_A receiptB=$RID_B"

log "OPEN SHIFT (PY → мок gRPC)"
PY_OS=$(curl -s -X POST "$PY/api/v2/prro/shift/open" -H "$AUTH")
echo "$PY_OS" | python3 -c "import sys,json;d=json.load(sys.stdin);print('  shift #'+str(d.get('shift_number')), 'id='+str(d.get('id'))[:8])" 2>/dev/null || echo "  open shift: $PY_OS"
SHIFT_ID=$(echo "$PY_OS" | python3 -c "import sys,json;print(json.load(sys.stdin).get('id',''))" 2>/dev/null)

# ─── 5. POST /receipts/{id}/fiscalize ───────────────────────────────────────
log "FISCALIZE (PY чек A, RS чек B — real ДСТУ-підпис + мок gRPC)"
PY_F=$(curl -s -X POST "$PY/api/v2/prro/receipts/$RID_A/fiscalize" -H "$AUTH")
RS_F=$(curl -s -X POST "$RS/api/v2/prro/receipts/$RID_B/fiscalize" -H "$AUTH")
if [ "$(echo "$PY_F"|py_norm)" = "$(echo "$RS_F"|py_norm)" ]; then ok "fiscalize parity (normalized)"; else
  echo "  ❌ fiscalize mismatch"; echo "  PY: $(echo "$PY_F"|py_norm)"; echo "  RS: $(echo "$RS_F"|py_norm)"; FAIL=$((FAIL+1))
fi
echo "  PY: $(echo "$PY_F" | python3 -c 'import sys,json;d=json.load(sys.stdin);print("status="+d.get("fiscal_status"),"num="+str(d.get("fiscal_number")),"serial="+str(d.get("fiscal_serial")),"url="+str(bool(d.get("fiscal_check_url"))),"err="+str(d.get("error")))' 2>/dev/null)"
echo "  RS: $(echo "$RS_F" | python3 -c 'import sys,json;d=json.load(sys.stdin);print("status="+d.get("fiscal_status"),"num="+str(d.get("fiscal_number")),"serial="+str(d.get("fiscal_serial")),"url="+str(bool(d.get("fiscal_check_url"))),"err="+str(d.get("error")))' 2>/dev/null)"

# ─── 6. Перевірка БД ────────────────────────────────────────────────────────
log "Перевірка БД"
ST_A=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT fiscal_status||'|'||coalesce(fiscal_number,'') FROM receipts WHERE id='$RID_A'")
ST_B=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT fiscal_status||'|'||coalesce(fiscal_number,'') FROM receipts WHERE id='$RID_B'")
if [[ "$ST_A" == sent\|* && "$ST_B" == sent\|* ]]; then ok "БД: receipts A+B fiscal_status=sent ($ST_A / $ST_B)"; else bad "БД receipts: $ST_A / $ST_B"; fi
Q_A=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT count(*) FROM prro_queue_items WHERE receipt_id='$RID_A' AND status='sent'")
Q_B=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT count(*) FROM prro_queue_items WHERE receipt_id='$RID_B' AND status='sent'")
if [ "$Q_A" = "1" ] && [ "$Q_B" = "1" ]; then ok "БД: prro_queue sent A=$Q_A B=$Q_B"; else bad "БД queue: A=$Q_A B=$Q_B"; fi
FSTOCK=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT fiscal_stock FROM products WHERE id='$PID'")
FSTOCK_N=$(echo "$FSTOCK" | cut -d. -f1)
if [ "$FSTOCK_N" = "96" ]; then ok "БД: fiscal_stock зменшено 100→96 (2 чеки × 2 шт, raw=$FSTOCK)"; else bad "БД fiscal_stock=$FSTOCK (очікувано 96)"; fi

# ─── 7. GET /settings після fiscalize ───────────────────────────────────────
log "SETTINGS GET після fiscalize"
PY_S3=$(curl -s "$PY/api/v2/prro/settings" -H "$AUTH")
RS_S3=$(curl -s "$RS/api/v2/prro/settings" -H "$AUTH")
if [ "$(echo "$PY_S3"|py_norm2)" = "$(echo "$RS_S3"|py_norm2)" ]; then ok "GET settings parity (shift_open/online)"; else bad "GET settings після fiscalize"; echo "  PY: $(echo "$PY_S3"|py_norm2)"; echo "  RS: $(echo "$RS_S3"|py_norm2)"; fi

# ─── 8. Очищення ────────────────────────────────────────────────────────────
log "Очищення"
PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q << SQL
DELETE FROM receipt_items WHERE receipt_id IN ('$RID_A','$RID_B');
DELETE FROM receipts WHERE id IN ('$RID_A','$RID_B');
DELETE FROM products WHERE id='$PID';
DELETE FROM prro_queue_items WHERE receipt_id IN ('$RID_A','$RID_B');
DELETE FROM prro_shifts WHERE id='$SHIFT_ID';
DELETE FROM prro_settings WHERE key_name IN ('prro_fn','prro_tn','prro_zn','mode','auto_fiscalize','last_shift_number','last_packet_id','last_mac_number','prro_stub_mode');
SQL
python3 - "$BACKEND" << 'PYEOF'
import os, sys
base = sys.argv[1]
for f in (".prro_keystore.json", ".prro_master.key", "certs/prro-test/pb_test.jks", "app/infrastructure/.prro_keystore.json", "app/infrastructure/.prro_master.key"):
    p = os.path.join(base, f)
    try: os.unlink(p)
    except OSError: pass
print("cleanup: keystore/certs видалено")
PYEOF
pkill -f "mock_prro_server.py" 2>/dev/null
pkill -f "uvicorn app.main:app --port 8003" 2>/dev/null
pkill -f "target/debug/facade" 2>/dev/null

echo ""
log "РЕЗУЛЬТАТ: PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = "0" ]
