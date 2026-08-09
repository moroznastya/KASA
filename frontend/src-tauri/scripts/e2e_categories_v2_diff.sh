#!/usr/bin/env bash
set -u
RUST=http://127.0.0.1:8002/api/v2
PY=http://127.0.0.1:8001/api/v2
RUSTV1=http://127.0.0.1:8002/api/v1
PYV1=http://127.0.0.1:8001/api/v1
TOKEN=$(cat /tmp/kasa_token)
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
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}
norm_name() {
  python3 -c "
import sys, json
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items() if k not in ('id','created_at','updated_at','name')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}

check() {
  if [ "$2" = "$3" ]; then echo "  OK $1"; else echo "  FAIL $1"; echo "    exp: $2"; echo "    got: $3"; FAIL=1; fi
}
echo "=== CATEGORIES V2 DIFF (ts=$TS) ==="
R_CREATE=$(curl -s -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat R '"$TS"'","description":"test desc"}')
P_CREATE=$(curl -s -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat P '"$TS"'","description":"test desc"}')
check "create 201 (R)" "201" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat R2 '"$TS"'"}')"
check "create 201 (P)" "201" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat P2 '"$TS"'"}')"
check "create parity (norm)" "$(echo "$P_CREATE" | norm_name)" "$(echo "$R_CREATE" | norm_name)"
R_ID=$(echo "$R_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
P_ID=$(echo "$P_CREATE" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
check "create 400 exists (R)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat R '"$TS"'"}')"
check "create 400 exists (P)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat P '"$TS"'"}')"
check "create 422 no name (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{}')"
check "create 422 no name (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{}')"
check "create 422 empty name (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{"name":""}')"
check "create 422 empty name (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{"name":""}')"
check "create 404 parent (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat X '"$TS"'","parent_id":"00000000-0000-0000-0000-000000000000"}')"
check "create 404 parent (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat Y '"$TS"'","parent_id":"00000000-0000-0000-0000-000000000000"}')"
check "create 422 bad parent uuid (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUST/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat Z '"$TS"'","parent_id":"not-uuid"}')"
check "create 422 bad parent uuid (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PY/categories" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat W '"$TS"'","parent_id":"not-uuid"}')"
check "get parity (norm)" "$(curl -s "$PY/categories/$P_ID" -H "$AUTH" | norm_name)" "$(curl -s "$RUST/categories/$R_ID" -H "$AUTH" | norm_name)"
check "get 404 (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' "$RUST/categories/00000000-0000-0000-0000-000000000000" -H "$AUTH")"
check "get 404 (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' "$PY/categories/00000000-0000-0000-0000-000000000000" -H "$AUTH")"
check "tree parity (norm)" "$(curl -s "$PY/categories/tree" -H "$AUTH" | norm)" "$(curl -s "$RUST/categories/tree" -H "$AUTH" | norm)"
check "list total (R)" "$(curl -s "$PY/categories?size=5" -H "$AUTH" | python3 -c 'import sys,json; print(json.load(sys.stdin)["total"])')" "$(curl -s "$RUST/categories?size=5" -H "$AUTH" | python3 -c 'import sys,json; print(json.load(sys.stdin)["total"])')"
check "list items parity" "$(curl -s "$PY/categories?size=3" -H "$AUTH" | norm)" "$(curl -s "$RUST/categories?size=3" -H "$AUTH" | norm)"
check "search parity" "$(curl -s "$PY/categories?search=Diff&size=10" -H "$AUTH" | norm)" "$(curl -s "$RUST/categories?search=Diff&size=10" -H "$AUTH" | norm)"
check "update 200 (R)" "200" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$RUST/categories/$R_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat RU '"$TS"'","description":"upd"}')"
check "update 200 (P)" "200" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$PY/categories/$P_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat PU '"$TS"'","description":"upd"}')"
check "update parity (norm)" "$(curl -s -X PUT "$PY/categories/$P_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat P3 '"$TS"'","description":"x"}' | norm_name)" "$(curl -s -X PUT "$RUST/categories/$R_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat R3 '"$TS"'","description":"x"}' | norm_name)"
check "update 404 (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$RUST/categories/00000000-0000-0000-0000-000000000000" -H "$AUTH" -H "$CT" -d '{"name":"X"}')"
check "update 404 (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$PY/categories/00000000-0000-0000-0000-000000000000" -H "$AUTH" -H "$CT" -d '{"name":"X"}')"
check "update 400 self-parent (R)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$RUST/categories/$R_ID" -H "$AUTH" -H "$CT" -d '{"parent_id":"'"$R_ID"'"}')"
check "update 400 self-parent (P)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$PY/categories/$P_ID" -H "$AUTH" -H "$CT" -d '{"parent_id":"'"$P_ID"'"}')"
check "update 400 exists (R)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$RUST/categories/$R_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat P3 '"$TS"'"}')"
check "update 400 exists (P)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$PY/categories/$P_ID" -H "$AUTH" -H "$CT" -d '{"name":"Diff Cat R3 '"$TS"'"}')"
check "update 404 parent (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$RUST/categories/$R_ID" -H "$AUTH" -H "$CT" -d '{"parent_id":"00000000-0000-0000-0000-000000000000"}')"
check "update 404 parent (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$PY/categories/$P_ID" -H "$AUTH" -H "$CT" -d '{"parent_id":"00000000-0000-0000-0000-000000000000"}')"
check "del 204 (R)" "204" "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$RUST/categories/$R_ID" -H "$AUTH")"
check "del 204 (P)" "204" "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$PY/categories/$P_ID" -H "$AUTH")"
check "del 404 (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$RUST/categories/$R_ID" -H "$AUTH")"
check "del 404 (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$PY/categories/$P_ID" -H "$AUTH")"
DNAME="Diff Pay R $TS"; DNAME2="Diff Pay P $TS"
DR=$(curl -s -X POST "$RUSTV1/debtors" -H "$AUTH" -H "$CT" -d '{"name":"'"$DNAME"'"}' | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
DP=$(curl -s -X POST "$PYV1/debtors" -H "$AUTH" -H "$CT" -d '{"name":"'"$DNAME2"'"}' | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
check "pay 422 amount<=0 (R)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUSTV1/debtors/$DR/pay" -H "$AUTH" -H "$CT" -d '{"amount":0}')"
check "pay 422 amount<=0 (P)" "422" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PYV1/debtors/$DP/pay" -H "$AUTH" -H "$CT" -d '{"amount":0}')"
check "pay 400 no debt (R)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUSTV1/debtors/$DR/pay" -H "$AUTH" -H "$CT" -d '{"amount":10}')"
check "pay 400 no debt (P)" "400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PYV1/debtors/$DP/pay" -H "$AUTH" -H "$CT" -d '{"amount":10}')"
check "pay 404 (R)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RUSTV1/debtors/00000000-0000-0000-0000-000000000000/pay" -H "$AUTH" -H "$CT" -d '{"amount":10}')"
check "pay 404 (P)" "404" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$PYV1/debtors/00000000-0000-0000-0000-000000000000/pay" -H "$AUTH" -H "$CT" -d '{"amount":10}')"
curl -s -X DELETE "$RUSTV1/debtors/$DR" -H "$AUTH" -o /dev/null 2>/dev/null
curl -s -X DELETE "$PYV1/debtors/$DP" -H "$AUTH" -o /dev/null 2>/dev/null
for ID in $(curl -s "$RUST/categories?search=Diff%20Cat&size=100" -H "$AUTH" | python3 -c "import sys,json; print(' '.join(i['id'] for i in json.load(sys.stdin)['items']))" 2>/dev/null); do
  curl -s -X DELETE "$RUST/categories/$ID" -H "$AUTH" -o /dev/null
done
for ID in $(curl -s "$PY/categories?search=Diff%20Cat&size=100" -H "$AUTH" | python3 -c "import sys,json; print(' '.join(i['id'] for i in json.load(sys.stdin)['items']))" 2>/dev/null); do
  curl -s -X DELETE "$PY/categories/$ID" -H "$AUTH" -o /dev/null
done
echo ""
if [ "$FAIL" = "0" ]; then echo "RESULT: ALL PASS"; else echo "RESULT: FAIL ($FAIL)"; exit 1; fi
