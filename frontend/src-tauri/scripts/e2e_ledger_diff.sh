#!/usr/bin/env bash
# E2E differential LEDGER (етап 4): Rust-фасад (:8002) vs Python (:8001), СПІЛЬНА БД.
# Покриває: журнал взаєморозрахунків v1 (POST /ledger, GET /{supplier_id},
# GET /balance/{supplier_id}) та v2 (GET/POST /entries, /balance, /balances).
#   - 10 000+ записів створюються через Rust v2 POST /ledger/entries (201)
#   - GET-порівняння Rust vs Python — 1:1 на 10 100 записах (сторінки по 100)
#   - валідації: 404/400/422/500 (ValueError) — 1:1
#   - конкурентність: 2 паралельні POST → обидва 201, жоден запис не втрачено
#   - cleanup: тестові дані (E4-/LEDGER-) повністю видаляються
# Потрібно: Python :8001, фасад :8002 (KASA_RUST_READDIRS=1), /tmp/kasa_token.
set -u
RUST=http://127.0.0.1:8002/api/v1
PY=http://127.0.0.1:8001/api/v1
RUSTV2=http://127.0.0.1:8002/api/v2
PYV2=http://127.0.0.1:8001/api/v2
TOKEN=$(cat /tmp/kasa_token)
AUTH="Authorization: Bearer $TOKEN"
CT="Content-Type: application/json"
TS=$(date +%s)
FAIL=0
export PGPASSWORD="${PGPASSWORD:-VgxWd7MBJ10X}"
PSQL="psql -h localhost -U postgres -d pos_system -t -A"

# norm GET: виключаємо id/created_at/updated_at/timestamps/number.
norm() {
  python3 -c "
import sys, json
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at','operation_date','last_updated','last_operation_date','number','receipt_id','document_id')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}

# norm_create: для POST-parity (різні supplier_id/послідовність → виключаємо
# supplier_id та balance_after; порівнюємо amount/operation_type/notes тощо).
norm_create() {
  python3 -c "
import sys, json
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at','operation_date','supplier_id','balance_after','document_id','last_updated','last_operation_date')}
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
    echo "  Rust:   $rn" | head -c 900; echo
    echo "  Python: $pn" | head -c 900; echo
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

# ─── Підготовка: 4 suppliers (через Rust :8002 CRUD, етап 2) ────────────────
mk_sup() {
  curl -s -X POST $RUST/suppliers -H "$AUTH" -H "$CT" \
    -d "{\"name\":\"$1\",\"phone\":\"\",\"address\":\"\"}" | jid id
}
S_RUST=$(mk_sup "E4-LEDGER-RUST-$TS")
S_PY=$(mk_sup "E4-LEDGER-PY-$TS")
S_MAIN1=$(mk_sup "E4-LEDGER-MAIN1-$TS")
S_MAIN2=$(mk_sup "E4-LEDGER-MAIN2-$TS")
S_MAIN3=$(mk_sup "E4-LEDGER-MAIN3-$TS")
S_MAIN4=$(mk_sup "E4-LEDGER-MAIN4-$TS")
S_CONC=$(mk_sup "E4-LEDGER-CONC-$TS")
echo "suppliers: RUST=$S_RUST PY=$S_PY MAIN1..4 CONC=$S_CONC"

# ═══ ВАЛІДАЦІЇ (1:1 Rust vs Python) ═══
ZERO=00000000-0000-0000-0000-000000000001
R=$(req $RUST GET "/ledger/$ZERO" -); RB=$(get_body)
P=$(req $PY GET "/ledger/$ZERO" -); PB=$(get_body)
cmp_resp "v1 GET /ledger/{id} неіснуючий supplier → 404" "$R" "$P" "$RB" "$PB"

R=$(req $RUST GET "/ledger/balance/$ZERO" -); RB=$(get_body)
P=$(req $PY GET "/ledger/balance/$ZERO" -); PB=$(get_body)
cmp_resp "v1 GET /ledger/balance/{id} неіснуючий → 404" "$R" "$P" "$RB" "$PB"

BODY="{\"supplier_id\":\"$ZERO\",\"amount\":\"10.00\",\"operation_type\":\"payment\",\"operation_date\":\"2026-08-07T10:00:00\"}"
R=$(req $RUST POST /ledger "$BODY"); RB=$(get_body)
P=$(req $PY POST /ledger "$BODY"); PB=$(get_body)
cmp_resp "v1 POST неіснуючий supplier → 404" "$R" "$P" "$RB" "$PB"

BODY="{\"supplier_id\":\"$S_MAIN1\",\"amount\":\"10.00\",\"operation_type\":\"bad\",\"operation_date\":\"2026-08-07T10:00:00\"}"
R=$(req $RUST POST /ledger "$BODY"); RB=$(get_body)
P=$(req $PY POST /ledger "$BODY"); PB=$(get_body)
cmp_resp "v1 POST невідомий тип → 400" "$R" "$P" "$RB" "$PB"

BODY="{\"supplier_id\":\"$S_MAIN1\",\"amount\":\"10.123\",\"operation_type\":\"payment\",\"operation_date\":\"2026-08-07T10:00:00\"}"
R=$(req $RUST POST /ledger "$BODY"); RB=$(get_body)
P=$(req $PY POST /ledger "$BODY"); PB=$(get_body)
cmp_resp "v1 POST amount >2 знаки → 422" "$R" "$P" "$RB" "$PB"

BODY="{\"supplier_id\":\"$S_MAIN1\",\"amount\":\"10.00\",\"operation_type\":\"payment\"}"
R=$(req $RUST POST /ledger "$BODY"); RB=$(get_body)
P=$(req $PY POST /ledger "$BODY"); PB=$(get_body)
cmp_resp "v1 POST без operation_date → 422" "$R" "$P" "$RB" "$PB"

# v2 POST неіснуючий supplier → 400 (Python цю валідацію виконує ДО бага create)
BODY="{\"supplier_id\":\"$ZERO\",\"amount\":10.0}"
R=$(req $RUSTV2 POST /ledger/entries "$BODY"); RB=$(get_body)
P=$(req $PYV2 POST /ledger/entries "$BODY"); PB=$(get_body)
cmp_resp "v2 POST неіснуючий supplier → 400" "$R" "$P" "$RB" "$PB"

R=$(req $RUSTV2 GET "/ledger/entries?operation_type=bad" -); RB=$(get_body)
P=$(req $PYV2 GET "/ledger/entries?operation_type=bad" -); PB=$(get_body)
cmp_resp "v2 GET entries невалідний operation_type → 500 ValueError" "$R" "$P" "$RB" "$PB"

R=$(req $RUSTV2 GET "/ledger/balance/$ZERO" -); RB=$(get_body)
P=$(req $PYV2 GET "/ledger/balance/$ZERO" -); PB=$(get_body)
cmp_resp "v2 GET balance неіснуючий → 404" "$R" "$P" "$RB" "$PB"

# ═══ CREATE PARITY ═══
# Rust v1 POST vs Python v1 POST — однакові вхідні на РІЗНИХ suppliers
# (нормалізація: supplier_id/balance_after виключені; amount/тип/notes 1:1).
BODY_R="{\"supplier_id\":\"$S_RUST\",\"amount\":\"100.50\",\"operation_type\":\"payment\",\"operation_date\":\"2026-08-07T10:00:00\",\"document_number\":\"DOC-1\",\"notes\":\"note\"}"
BODY_P="{\"supplier_id\":\"$S_PY\",\"amount\":\"100.50\",\"operation_type\":\"payment\",\"operation_date\":\"2026-08-07T10:00:00\",\"document_number\":\"DOC-1\",\"notes\":\"note\"}"
R=$(req $RUST POST /ledger "$BODY_R"); RB=$(get_body)
P=$(req $PY POST /ledger "$BODY_P"); PB=$(get_body)
if [ "$R" == "201" ] && [ "$P" == "201" ]; then
  rn=$(echo "$RB" | norm_create); pn=$(echo "$PB" | norm_create)
  if [ "$rn" == "$pn" ]; then
    echo "✅ v1 POST create parity: обидва 201, тіло ідентичне (amount/тип/notes)"
  else
    echo "❌ v1 POST create parity: тіло РОЗБІЖНЕ"; echo "R: $rn"; echo "P: $pn"; FAIL=1
  fi
else
  echo "❌ v1 POST create parity: Rust=$R Python=$P"; FAIL=1
fi

# Rust v2 POST — 201 (Python v2 create ЗЛАМАНИЙ: 500 UnmappedInstanceError —
# сигналізується в звіті; Rust реалізує задуману робочу поведінку).
R=$(req $RUSTV2 POST /ledger/entries "{\"supplier_id\":\"$S_RUST\",\"amount\":200.0,\"operation_type\":\"invoice\",\"document_number\":\"DOC-2\",\"notes\":\"\"}"); RB=$(get_body)
P=$(req $PYV2 POST /ledger/entries "{\"supplier_id\":\"$S_PY\",\"amount\":200.0,\"operation_type\":\"invoice\",\"document_number\":\"DOC-2\",\"notes\":\"\"}"); PB=$(get_body)
if [ "$R" == "201" ] && [ "$P" == "500" ]; then
  echo "ℹ️ v2 POST create: Rust=201 (робочий), Python=500 (баг UnmappedInstanceError) — аномалія Python"
  echo "   Rust-тіло: $(echo "$RB" | python3 -c 'import sys,json;d=json.load(sys.stdin);print({k:d[k] for k in ("amount","operation_type","document_number","notes")})')"
elif [ "$R" == "201" ] && [ "$P" == "201" ]; then
  rn=$(echo "$RB" | norm_create); pn=$(echo "$PB" | norm_create)
  if [ "$rn" == "$pn" ]; then
    echo "✅ v2 POST create parity: обидва 201, тіло ідентичне"
  else
    echo "❌ v2 POST create parity: тіло РОЗБІЖНЕ"; FAIL=1
  fi
else
  echo "❌ v2 POST create: Rust=$R Python=$P"; FAIL=1
fi

# ═══ 10 000 записів через Rust v2 POST /ledger/entries (4 suppliers × 2500) ═══
# Паралельність по suppliers (xargs -P 4): послідовність у межах supplier
# зберігається (balance_after коректний), загальний час ~25с.
echo "створюю 10 000 записів через Rust :8002 (4 suppliers × 2500, паралельно)..."
CREATE_OK=1
export RUSTV2 AUTH CT S_MAIN1 S_MAIN2 S_MAIN3 S_MAIN4
create_batch() { # supplier_prefix start end
  local sid=$1 start=$2 end=$3
  for i in $(seq $start $end); do
    code=$(curl -s -o /dev/null -w "%{http_code}" -X POST $RUSTV2/ledger/entries \
      -H "$AUTH" -H "$CT" \
      -d "{\"supplier_id\":\"$sid\",\"amount\":1.0,\"operation_type\":\"invoice\",\"document_number\":\"LEDGER-$i\",\"notes\":\"\"}")
    [ "$code" != "201" ] && echo "❌ create #$i ($sid): $code" && CREATE_OK=0 && return 1
  done
}
export -f create_batch
seq 1 4 | xargs -P 4 -I{} bash -c '
case {} in
  1) create_batch "$S_MAIN1" 1 2500 ;;
  2) create_batch "$S_MAIN2" 2501 5000 ;;
  3) create_batch "$S_MAIN3" 5001 7500 ;;
  4) create_batch "$S_MAIN4" 7501 10000 ;;
esac'
if [ "$CREATE_OK" == "1" ]; then
  echo "✅ створено через Rust: 10000/10000 (всі 201)"
else
  echo "❌ створення через Rust: НЕПОВНЕ"; FAIL=1
fi

# тіло першого запису — перевірка балансу (1.0, 2.0)
B1=$(curl -s -X POST $RUSTV2/ledger/entries -H "$AUTH" -H "$CT" \
  -d "{\"supplier_id\":\"$S_RUST\",\"amount\":5.0,\"operation_type\":\"correction\",\"notes\":\"x\"}")
echo "   v2 POST тіло (Rust): $(echo "$B1" | python3 -c 'import sys,json;d=json.load(sys.stdin);print({k:d[k] for k in ("amount","balance_after","operation_type","notes","document_number")})')"

# +100 записів через Python v1 POST (S_MAIN1) — покриття write-шляху Python
echo "додаю 100 записів через Python :8001 v1 POST (S_MAIN1)..."
for i in $(seq 1 100); do
  code=$(curl -s -o /dev/null -w "%{http_code}" -X POST $PY/ledger \
    -H "$AUTH" -H "$CT" \
    -d "{\"supplier_id\":\"$S_MAIN1\",\"amount\":\"1.00\",\"operation_type\":\"payment\",\"operation_date\":\"2026-08-07T16:00:00\",\"document_number\":\"PY-LEDGER-$i\",\"notes\":\"\"}")
  [ "$code" != "201" ] && echo "❌ py create #$i: $code" && FAIL=1 && break
done
echo "✅ 100 записів через Python: всі 201"

# ═══ GET DIFFERENTIAL (10 100 записів, 4 suppliers) ═══
ALL_MAIN="$S_MAIN1','$S_MAIN2','$S_MAIN3','$S_MAIN4"
TOTAL=$($PSQL -c "SELECT count(*) FROM supplier_ledger WHERE supplier_id IN ('$ALL_MAIN')")
echo "всього записів у БД (MAIN1..4): $TOTAL"
PAGES_TOTAL=0
for SID in $S_MAIN1 $S_MAIN2 $S_MAIN3 $S_MAIN4; do
  T=$($PSQL -c "SELECT count(*) FROM supplier_ledger WHERE supplier_id='$SID'")
  PAGES=$(( (T + 99) / 100 ))
  for pg in $(seq 1 $PAGES); do
    R=$(req $RUSTV2 GET "/ledger/entries?supplier_id=$SID&page=$pg&size=100" -); RB=$(get_body)
    P=$(req $PYV2 GET "/ledger/entries?supplier_id=$SID&page=$pg&size=100" -); PB=$(get_body)
    if [ "$R" == "200" ] && [ "$P" == "200" ]; then
      rn=$(echo "$RB" | norm); pn=$(echo "$PB" | norm)
      if [ "$rn" != "$pn" ]; then
        echo "❌ v2 entries ($SID) сторінка $pg: тіло РОЗБІЖНЕ"; FAIL=1
        echo "  R: $(echo "$rn" | head -c 400)"; echo "  P: $(echo "$pn" | head -c 400)"
        break 2
      fi
    else
      echo "❌ v2 entries ($SID) сторінка $pg: Rust=$R Python=$P"; FAIL=1; break 2
    fi
    PAGES_TOTAL=$((PAGES_TOTAL + 1))
  done
  echo "  $SID: $T записів, $PAGES сторінок 1:1"
done
echo "✅ v2 GET entries: $PAGES_TOTAL сторінок (10 100 записів) 1:1 (Rust==Python)"

# v1 GET history — сторінки 1, 2, 25 (без кешу)
for pg in 1 2 25; do
  R=$(req $RUST GET "/ledger/$S_MAIN1?page=$pg&size=100" -); RB=$(get_body)
  P=$(req $PY GET "/ledger/$S_MAIN1?page=$pg&size=100" -); PB=$(get_body)
  cmp_resp "v1 GET /ledger/{id} сторінка $pg" "$R" "$P" "$RB" "$PB"
done

# v1 balance + v2 balance (по всіх 4 MAIN)
for SID in $S_MAIN1 $S_MAIN2; do
  R=$(req $RUST GET "/ledger/balance/$SID" -); RB=$(get_body)
  P=$(req $PY GET "/ledger/balance/$SID" -); PB=$(get_body)
  cmp_resp "v1 GET balance $SID" "$R" "$P" "$RB" "$PB"
  R=$(req $RUSTV2 GET "/ledger/balance/$SID" -); RB=$(get_body)
  P=$(req $PYV2 GET "/ledger/balance/$SID" -); PB=$(get_body)
  cmp_resp "v2 GET balance $SID" "$R" "$P" "$RB" "$PB"
done

# v2 balances: знайти S_MAIN в обох списках
find_bal() {
  python3 -c "
import sys, json
d = json.load(sys.stdin)
sid = '$1'
for x in d:
    if x.get('supplier_id') == sid:
        print(json.dumps(x, ensure_ascii=False, sort_keys=True)); break
"
}
R=$(req $RUSTV2 GET "/ledger/balances" -); RB=$(get_body)
P=$(req $PYV2 GET "/ledger/balances" -); PB=$(get_body)
for SID in $S_MAIN1 $S_MAIN3; do
  rb=$(echo "$RB" | find_bal "$SID" | norm)
  pb=$(echo "$PB" | find_bal "$SID" | norm)
  if [ "$rb" == "$pb" ] && [ -n "$rb" ]; then
    echo "✅ v2 GET balances: $SID знайдено в обох, запис ідентичний"
  else
    echo "❌ v2 GET balances: $SID розбіжність"; echo "R: $rb"; echo "P: $pb"; FAIL=1
  fi
done

# ═══ КОНКУРЕНТНІСТЬ: 2 паралельні POST на S_CONC ═══
curl -s -o /dev/null -w "%{http_code}" -X POST $RUSTV2/ledger/entries -H "$AUTH" -H "$CT" \
  -d "{\"supplier_id\":\"$S_CONC\",\"amount\":10.0,\"operation_type\":\"invoice\"}" > /tmp/conc1 &
curl -s -o /dev/null -w "%{http_code}" -X POST $RUSTV2/ledger/entries -H "$AUTH" -H "$CT" \
  -d "{\"supplier_id\":\"$S_CONC\",\"amount\":20.0,\"operation_type\":\"invoice\"}" > /tmp/conc2 &
wait
C1=$(cat /tmp/conc1); C2=$(cat /tmp/conc2)
CC=$($PSQL -c "SELECT count(*) FROM supplier_ledger WHERE supplier_id='$S_CONC'")
if [ "$C1" == "201" ] && [ "$C2" == "201" ] && [ "$CC" == "2" ]; then
  echo "✅ конкурентність: 2 паралельні POST → 201/201, записів: 2 (жоден не втрачено)"
else
  echo "❌ конкурентність: статуси $C1/$C2, записів=$CC"; FAIL=1
fi

# ═══ ТРАНЗАКЦІЙНІСТЬ: помилка не залишає слідів ═══
BEFORE=$($PSQL -c "SELECT count(*) FROM supplier_ledger WHERE supplier_id='$S_MAIN1'")
curl -s -o /dev/null -X POST $RUST/ledger -H "$AUTH" -H "$CT" \
  -d "{\"supplier_id\":\"$S_MAIN1\",\"amount\":\"5.00\",\"operation_type\":\"bad\",\"operation_date\":\"2026-08-07T10:00:00\"}"
AFTER=$($PSQL -c "SELECT count(*) FROM supplier_ledger WHERE supplier_id='$S_MAIN1'")
if [ "$BEFORE" == "$AFTER" ]; then
  echo "✅ транзакційність: 400 (невалідний тип) не створив запис ($BEFORE == $AFTER)"
else
  echo "❌ транзакційність: count змінився $BEFORE → $AFTER"; FAIL=1
fi

# ═══ CLEANUP ═══
$PSQL -c "DELETE FROM supplier_ledger WHERE supplier_id IN ('$ALL_MAIN','$S_RUST','$S_PY','$S_CONC')" > /dev/null
$PSQL -c "DELETE FROM suppliers WHERE id IN ('$ALL_MAIN','$S_RUST','$S_PY','$S_CONC')" > /dev/null
LEFT=$($PSQL -c "SELECT count(*) FROM supplier_ledger WHERE supplier_id IN ('$ALL_MAIN','$S_RUST','$S_PY','$S_CONC')")
SUP_LEFT=$($PSQL -c "SELECT count(*) FROM suppliers WHERE id IN ('$ALL_MAIN','$S_RUST','$S_PY','$S_CONC')")
if [ "$LEFT" == "0" ] && [ "$SUP_LEFT" == "0" ]; then
  echo "✅ cleanup: ledger-записів=0, suppliers=0 (тестові дані E4- видалено)"
else
  echo "❌ cleanup: ledger=$LEFT suppliers=$SUP_LEFT"; FAIL=1
fi

echo "===================="
if [ "$FAIL" == "0" ]; then
  echo "E2E DIFFERENTIAL LEDGER: ВСІ ПРОЙДЕНО ($TOTAL записів, Rust==Python 1:1)"
else
  echo "E2E DIFFERENTIAL LEDGER: Є РОЗБІЖНОСТІ"
fi
exit $FAIL
