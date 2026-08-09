#!/usr/bin/env bash
# E2E differential DEBTORS (етап 8, група 1): Rust-фасад (:8002) vs Python (:8001).
# Покриває: search, list, create, get, update, pay (часткове/повне), receipts,
# payments + валідації 404/400/422.
# Cleanup: тестові дані (Diff Деб%) видаляються наприкінці.
# Потрібно: Python :8001, фасад :8002 (TORGASHKA_RUST_DEBTORS=1), /tmp/torgashka_token.
set -u
RUST=http://127.0.0.1:8002/api/v1
PY=http://127.0.0.1:8001/api/v1
TOKEN=$(cat /tmp/torgashka_token 2>/dev/null)
AUTH="Authorization: Bearer $TOKEN"
CT="Content-Type: application/json"
TS=$(date +%s)
FAIL=0
export PGPASSWORD="${PGPASSWORD:-VgxWd7MBJ10X}"
PSQL="psql -h localhost -U postgres -d pos_system -t -A"

norm() {  # виключаємо id/created_at/updated_at
  python3 -c "
import sys, json
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at','name','debtor_id')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}

check() {  # $1=name $2=expected $3=actual
  if [ "$2" = "$3" ]; then echo "  ✅ $1"; else echo "  ❌ $1"; echo "    exp: $2"; echo "    got: $3"; FAIL=1; fi
}

echo "═══ DEBTORS DIFF (ts=$TS) ═══"

# ─── Підготовка: створюємо боржника через Rust і через Python ───────────────
RNAME="Diff Деб R-$TS"; PNAME="Diff Деб P-$TS"
R_CREATE=$(curl -s -X POST "$RUST/debtors" -H "$AUTH" -H "$CT" -d "{\"name\":\"$RNAME\",\"phone\":\"38067\",\"notes\":\"test\"}")
P_CREATE=$(curl -s -X POST "$PY/debtors" -H "$AUTH" -H "$CT" -d "{\"name\":\"$PNAME\",\"phone\":\"38067\",\"notes\":\"test\"}")
R_ID=$(echo "$R_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null)
P_ID=$(echo "$P_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null)
if [ -z "$R_ID" ] || [ -z "$P_ID" ]; then echo "❌ create failed: R=$R_CREATE P=$P_CREATE"; exit 1; fi
check "create 201 (R)" "201" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/debtors" -H "$AUTH" -H "$CT" -d "{\"name\":\"$RNAME-x\"}")"
check "create parity (norm)" "$(echo "$P_CREATE" | norm)" "$(echo "$R_CREATE" | norm)"

# ─── Search ───────────────────────────────────────────────────────────────────
Q=$(python3 -c "import urllib.parse; print(urllib.parse.quote('Diff Деб'))")
R_SEARCH=$(curl -s "$RUST/debtors/search?query=$Q&limit=10" -H "$AUTH" | norm)
P_SEARCH=$(curl -s "$PY/debtors/search?query=$Q&limit=10" -H "$AUTH" | norm)
check "search parity" "$P_SEARCH" "$R_SEARCH"

# ─── List (пагінація) ────────────────────────────────────────────────────────
R_LIST=$(curl -s "$RUST/debtors?page=1&size=1000" -H "$AUTH")
P_LIST=$(curl -s "$PY/debtors?page=1&size=1000" -H "$AUTH")
check "list total parity" "$(echo "$P_LIST" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")" "$(echo "$R_LIST" | python3 -c "import sys,json; print(json.load(sys.stdin)['total'])")"
R_ME=$(echo "$R_LIST" | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps([x for x in d['items'] if x['name'].startswith('Diff Деб')], ensure_ascii=False, sort_keys=True))")
P_ME=$(echo "$P_LIST" | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps([x for x in d['items'] if x['name'].startswith('Diff Деб')], ensure_ascii=False, sort_keys=True))")
check "list items parity (norm)" "$(echo "$P_ME" | norm)" "$(echo "$R_ME" | norm)"

# ─── Get ──────────────────────────────────────────────────────────────────────
R_GET=$(curl -s "$RUST/debtors/$R_ID" -H "$AUTH"); P_GET=$(curl -s "$PY/debtors/$P_ID" -H "$AUTH")
check "get parity (norm)" "$(echo "$P_GET" | norm)" "$(echo "$R_GET" | norm)"

# ─── Update ───────────────────────────────────────────────────────────────────
R_UPD=$(curl -s -X PUT "$RUST/debtors/$R_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Деб RU-'$TS'","phone":"123","notes":"upd"}')
P_UPD=$(curl -s -X PUT "$PY/debtors/$P_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Деб PU-'$TS'","phone":"123","notes":"upd"}')
check "update parity (norm)" "$(echo "$P_UPD" | norm)" "$(echo "$R_UPD" | norm)"

# ─── Валідації ───────────────────────────────────────────────────────────────
check "404 get (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' "$RUST/debtors/00000000-0000-0000-0000-000000000000" -H "$AUTH")"
check "404 get (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' "$PY/debtors/00000000-0000-0000-0000-000000000000" -H "$AUTH")"
check "422 not-uuid (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' "$RUST/debtors/not-a-uuid" -H "$AUTH")"
check "422 not-uuid (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' "$PY/debtors/not-a-uuid" -H "$AUTH")"
check "422 empty name (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/debtors" -H "$AUTH" -H "$CT" -d '{"name":""}')"
check "422 empty name (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/debtors" -H "$AUTH" -H "$CT" -d '{"name":""}')"
check "422 empty search (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' "$RUST/debtors/search?query=" -H "$AUTH")"
check "422 empty search (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' "$PY/debtors/search?query=" -H "$AUTH")"

# ─── Pay: борг без боргу → 400 (обидва) ──────────────────────────────────────
check "pay no-debt 400 (R)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/debtors/$R_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"5.00"}')"
check "pay no-debt 400 (P)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/debtors/$P_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"5.00"}')"

# ─── Pay: встановлюємо борг 100.00 напряму (тестові дані) ────────────────────
$PSQL -c "UPDATE debtors SET total_debt=100.00 WHERE id='$R_ID' OR id='$P_ID'" >/dev/null
check "pay amount<=0 422 (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/debtors/$R_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"0.00"}')"
check "pay amount<=0 422 (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/debtors/$P_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"0.00"}')"
check "pay over-debt 400 (R)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/debtors/$R_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"150.00"}')"
check "pay over-debt 400 (P)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/debtors/$P_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"150.00"}')"

# ─── Pay часткове: 30.00 → 70.00 ──────────────────────────────────────────────
R_PAY=$(curl -s -X POST "$RUST/debtors/$R_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"30.00","payment_method":"cash"}')
P_PAY=$(curl -s -X POST "$PY/debtors/$P_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"30.00","payment_method":"cash"}')
check "pay partial parity (norm)" "$(echo "$P_PAY" | norm)" "$(echo "$R_PAY" | norm)"
check "pay partial total_debt=70.00 (R)" "70.00" "$(echo "$R_PAY" | python3 -c "import sys,json; print(json.load(sys.stdin)['total_debt'])")"

# ─── Payments: історія після часткової оплати ────────────────────────────────
R_PAYM=$(curl -s "$RUST/debtors/$R_ID/payments" -H "$AUTH" | norm)
P_PAYM=$(curl -s "$PY/debtors/$P_ID/payments" -H "$AUTH" | norm)
check "payments parity (norm)" "$P_PAYM" "$R_PAYM"

# ─── Pay повне: 70.00 → боржник видаляється ──────────────────────────────────
R_PAYF=$(curl -s -X POST "$RUST/debtors/$R_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"70.00","payment_method":"cash"}')
P_PAYF=$(curl -s -X POST "$PY/debtors/$P_ID/pay" -H "$AUTH" -H "$CT" -d '{"amount":"70.00","payment_method":"cash"}')
check "pay full total_debt=0.00 (R)" "0.00" "$(echo "$R_PAYF" | python3 -c "import sys,json; print(json.load(sys.stdin)['total_debt'])")"
check "pay full deleted (R → 404)" "404" "$(curl -s -o /dev/null -w '%{http_code}' "$RUST/debtors/$R_ID" -H "$AUTH")"
check "pay full deleted (P → 404)" "404" "$(curl -s -o /dev/null -w '%{http_code}' "$PY/debtors/$P_ID" -H "$AUTH")"

# ─── Receipts: реальний боржник з чеками ─────────────────────────────────────
DEBTOR_WITH_RCPTS=$($PSQL -c "SELECT d.id FROM debtors d JOIN receipts r ON r.debtor_id=d.id GROUP BY d.id LIMIT 1")
if [ -n "$DEBTOR_WITH_RCPTS" ]; then
  R_RCPT=$(curl -s "$RUST/debtors/$DEBTOR_WITH_RCPTS/receipts" -H "$AUTH" | norm)
  P_RCPT=$(curl -s "$PY/debtors/$DEBTOR_WITH_RCPTS/receipts" -H "$AUTH" | norm)
  check "receipts parity (norm)" "$P_RCPT" "$R_RCPT"
else
  echo "  ⚠️  боржників з чеками немає — receipts parity пропущено"
fi

# ─── Cleanup ──────────────────────────────────────────────────────────────────
$PSQL -c "DELETE FROM debtors WHERE name LIKE 'Diff Деб%'" >/dev/null
echo "─── RESULT: $([ $FAIL -eq 0 ] && echo 'PASS' || echo 'FAIL') ───"
exit $FAIL
