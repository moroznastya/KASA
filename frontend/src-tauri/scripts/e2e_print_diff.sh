#!/usr/bin/env bash
# ============================================================================
# e2e_print_diff.sh — differential-тест друку (етап 8, група 6).
# Rust :8002 (KASA_RUST_PRINT=1) vs Python :8001 (еталон).
#
# Покриває:
#   print.py (4 роути): price-tags/render (HTML A4 grid + meta),
#     labels/render (sequential + print_mode escpos), printers (lpstat -e),
#     test (receipt exact / price_tag / label)
#   print_templates.py (9 роутів): list active (пагінація), all (admin),
#     default, get, create (201), update (exclude_unset), delete (soft 204),
#     set-default, render (replace {{var}} + font)
#   Pydantic 422: page/size, barcode_type, width_mm, products empty,
#     print_type, name empty
#
# SVG-блоки нормалізуються (<svg>...</svg> → [SVG]) — різні генератори
# (python-barcode vs власний Rust Code128-рендер); решта HTML — exact.
# datetime: created_at/updated_at 1:1 з Python Pydantic UTC (%.6f + Z) —
# перевіряється exact на фіксованому SQL-шаблоні (та сама БД).
# ============================================================================
set -u
PY=http://127.0.0.1:8001
RS=http://127.0.0.1:8002
TOKEN=$(cat /tmp/kasa_token 2>/dev/null || echo "")
AUTH="Authorization: Bearer $TOKEN"
PASS=0; FAIL=0
TS=$(date +%s)
PT_ID="11111111-1111-1111-1111-11111111${TS: -4}"

log()  { echo "[$(date +%H:%M:%S)] $*"; }
ok()   { PASS=$((PASS+1)); echo "  ✅ $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

# Нормалізація JSON: SVG→[SVG], id/дати→<norm>.
py_norm() {
python3 -c "
import sys,json,re
def strip_svg(x):
    if isinstance(x,str): return re.sub(r'<svg[^>]*>.*?</svg>','[SVG]',x,flags=re.S)
    if isinstance(x,dict):
        return {k:('<id>' if k=='id' and isinstance(v,str) and len(v)==36 else '<dt>' if k in('created_at','updated_at') else strip_svg(v)) for k,v in x.items()}
    if isinstance(x,list): return [strip_svg(v) for v in x]
    return x
o=json.load(sys.stdin)
print(json.dumps(strip_svg(o),ensure_ascii=False,sort_keys=True))
"
}
# Exact JSON parity (без нормалізації id/datetime).
py_exact() {
python3 -c "
import sys,json,re
def strip_svg(x):
    if isinstance(x,str): return re.sub(r'<svg[^>]*>.*?</svg>','[SVG]',x,flags=re.S)
    if isinstance(x,dict): return {k:strip_svg(v) for k,v in x.items()}
    if isinstance(x,list): return [strip_svg(v) for v in x]
    return x
o=json.load(sys.stdin)
print(json.dumps(strip_svg(o),ensure_ascii=False,sort_keys=True))
"
}
# HTML-only нормалізація (для render-порівнянь).
html_norm() {
python3 -c "
import sys,re,hashlib
s=sys.stdin.read()
s=re.sub(r'<svg[^>]*>.*?</svg>','[SVG]',s,flags=re.S)
print(len(s),hashlib.sha256(s.encode()).hexdigest()[:16])
"
}

# ── 1. PRINT/PRINTERS (публічний; обидва lpstat -e) ─────────────────────────
P=$(curl -s -m 5 "$PY/api/v1/print/printers")
R=$(curl -s -m 5 "$RS/api/v1/print/printers")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "printers parity (lpstat -e)"; else bad "printers parity"; echo "  PY: $P"; echo "  RS: $R"; fi

# ── 2. ФІКСОВАНИЙ ШАБЛОН через SQL → get exact (datetime 1:1) ───────────────
FIXED_CT="2026-06-15 12:34:56.789012"
PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q << SQL
INSERT INTO print_templates (id, name, type, content, variables, is_default, is_active, created_at, updated_at)
VALUES ('$PT_ID', 'Diff PT Fix $TS', 'custom', '<h1>{{shop_name}}</h1><p>{{total}} грн</p>', '[{"key":"shop_name"}]', false, true, '$FIXED_CT+00', '$FIXED_CT+00')
ON CONFLICT (id) DO UPDATE SET name=EXCLUDED.name, content=EXCLUDED.content, updated_at=EXCLUDED.updated_at;
SQL
P=$(curl -s -m 5 "$PY/api/v1/print-templates/$PT_ID" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates/$PT_ID" -H "$AUTH")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "get фіксованого: exact parity (datetime %.6f+Z 1:1)"; else bad "get фіксованого: parity"; echo "  PY: $(echo "$P" | head -c 250)"; echo "  RS: $(echo "$R" | head -c 250)"; fi
if echo "$R" | grep -q "2026-06-15T12:34:56.789012Z"; then ok "datetime формат UTC %.6f+Z"; else bad "datetime формат: $(echo "$R" | grep -o '"created_at":"[^"]*"' | head -1)"; fi

# ── 3. LIST ACTIVE (пагінація) ───────────────────────────────────────────────
P=$(curl -s -m 5 "$PY/api/v1/print-templates?page=1&size=2" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates?page=1&size=2" -H "$AUTH")
if [ "$(echo "$P" | py_norm)" = "$(echo "$R" | py_norm)" ]; then ok "list active page=1&size=2 parity (items normalized)"; else bad "list active parity"; echo "  PY: $(echo "$P" | head -c 200)"; echo "  RS: $(echo "$R" | head -c 200)"; fi

# ── 4. LIST ALL (admin) ──────────────────────────────────────────────────────
P=$(curl -s -m 5 "$PY/api/v1/print-templates/all" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates/all" -H "$AUTH")
if [ "$(echo "$P" | py_norm)" = "$(echo "$R" | py_norm)" ]; then ok "list all parity (admin, items normalized)"; else bad "list all parity"; fi

# ── 5. DEFAULT (get_default_for_type) ────────────────────────────────────────
P=$(curl -s -m 5 "$PY/api/v1/print-templates/default?type=receipt_58mm" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates/default?type=receipt_58mm" -H "$AUTH")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "default type=receipt_58mm parity (exact)"; else bad "default parity"; fi
P=$(curl -s -m 5 -w "\n%{http_code}" "$PY/api/v1/print-templates/default?type=nonexistent_type" -H "$AUTH")
R=$(curl -s -m 5 -w "\n%{http_code}" "$RS/api/v1/print-templates/default?type=nonexistent_type" -H "$AUTH")
PC=${P##*$'\n'}; RC=${R##*$'\n'}; PB=${P%$'\n'*}; RB=${R%$'\n'*}
if [ "$PC" = "404" ] && [ "$RC" = "404" ] && [ "$PB" = "$RB" ]; then ok "default 404 detail parity"; else bad "default 404: PY=$PC RS=$RC"; fi

# ── 6. CREATE (201) + get (normalized) ───────────────────────────────────────
CB='{"name":"Diff PT Create '$TS'","type":"custom","content":"<h1>{{shop_name}}</h1>","variables":[{"key":"shop_name","label":"Назва"}],"is_default":false}'
P=$(curl -s -m 5 -w "\n%{http_code}" -X POST "$PY/api/v1/print-templates" -H "$AUTH" -H "Content-Type: application/json" -d "$CB")
R=$(curl -s -m 5 -w "\n%{http_code}" -X POST "$RS/api/v1/print-templates" -H "$AUTH" -H "Content-Type: application/json" -d "$CB")
PC=${P##*$'\n'}; RC=${R##*$'\n'}; PB=${P%$'\n'*}; RB=${R%$'\n'*}
if [ "$PC" = "201" ] && [ "$RC" = "201" ]; then ok "create статус 201 (PY=$PC RS=$RC)"; else bad "create статус: PY=$PC RS=$RC"; fi
if [ "$(echo "$PB" | py_norm)" = "$(echo "$RB" | py_norm)" ]; then ok "create parity (id/дати нормалізовані)"; else bad "create parity"; echo "  PY: $(echo "$PB" | head -c 200)"; echo "  RS: $(echo "$RB" | head -c 200)"; fi
PID_P=$(echo "$PB" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
PID_R=$(echo "$RB" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
# get свого створеного через PY і RS — exact (та сама БД)
P=$(curl -s -m 5 "$PY/api/v1/print-templates/$PID_P" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates/$PID_P" -H "$AUTH")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "get створеного (спільна БД) exact parity"; else bad "get створеного exact"; fi

# ── 7. UPDATE (exclude_unset + is_default знімає з інших) ────────────────────
UP='{"name":"Diff PT Updated '$TS'","content":"<h1>{{shop_name}} NEW</h1><p>{{total}}</p>","is_default":true}'
P=$(curl -s -m 5 -X PUT "$PY/api/v1/print-templates/$PID_P" -H "$AUTH" -H "Content-Type: application/json" -d "$UP")
R=$(curl -s -m 5 -X PUT "$RS/api/v1/print-templates/$PID_R" -H "$AUTH" -H "Content-Type: application/json" -d "$UP")
if [ "$(echo "$P" | py_norm)" = "$(echo "$R" | py_norm)" ]; then ok "update parity (name/content/is_default)"; else bad "update parity"; fi
# is_default знято з інших custom (9d9b552d мав is_default=true)
CHK=$(PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -tAc "SELECT count(*) FROM print_templates WHERE type='custom' AND is_default=true")
if [ "$CHK" = "1" ]; then ok "update is_default: тільки 1 custom default (БД=$CHK)"; else bad "update is_default: БД=$CHK (очікувано 1)"; fi
# get через PY (той самий запис, exact)
P=$(curl -s -m 5 "$PY/api/v1/print-templates/$PID_P" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates/$PID_P" -H "$AUTH")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "update → get exact parity (спільна БД)"; else bad "update → get exact"; fi

# ── 8. SET-DEFAULT (через RS; перевірка через PY) ───────────────────────────
P=$(curl -s -m 5 -X POST "$PY/api/v1/print-templates/$PID_P/set-default" -H "$AUTH")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print-templates/$PID_R/set-default" -H "$AUTH")
if [ "$(echo "$P" | py_norm)" = "$(echo "$R" | py_norm)" ]; then ok "set-default parity"; else bad "set-default parity"; fi
R=$(curl -s -m 5 "$RS/api/v1/print-templates/$PID_R" -H "$AUTH")
if echo "$R" | grep -q '"is_default":true'; then ok "set-default через RS: is_default=true (get)"; else bad "set-default is_default: $R"; fi

# ── 9. RENDER шаблону (replace {{var}} + font) — exact ──────────────────────
RD='{"data":{"shop_name":"Kasa Тест","total":"777.50","fiscal_block":"FB-123"}}'
P=$(curl -s -m 5 -X POST "$PY/api/v1/print-templates/$PID_P/render" -H "$AUTH" -H "Content-Type: application/json" -d "$RD")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print-templates/$PID_P/render" -H "$AUTH" -H "Content-Type: application/json" -d "$RD")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "render parity (exact, font apply)"; else bad "render parity"; echo "  PY: $(echo "$P" | head -c 200)"; echo "  RS: $(echo "$R" | head -c 200)"; fi

# ── 10. DELETE (soft, 204) + 404 після ──────────────────────────────────────
P=$(curl -s -m 5 -o /dev/null -w "%{http_code}" -X DELETE "$PY/api/v1/print-templates/$PID_P" -H "$AUTH")
R=$(curl -s -m 5 -o /dev/null -w "%{http_code}" -X DELETE "$RS/api/v1/print-templates/$PID_R" -H "$AUTH")
if [ "$P" = "204" ] && [ "$R" = "204" ]; then ok "delete 204 (PY=$P RS=$R)"; else bad "delete 204: PY=$P RS=$R"; fi
P=$(curl -s -m 5 -w "\n%{http_code}" "$PY/api/v1/print-templates/$PID_P" -H "$AUTH")
R=$(curl -s -m 5 -w "\n%{http_code}" "$RS/api/v1/print-templates/$PID_P" -H "$AUTH")
PC=${P##*$'\n'}; RC=${R##*$'\n'}; PB=${P%$'\n'*}; RB=${R%$'\n'*}
if [ "$PC" = "200" ] && [ "$RC" = "200" ] && [ "$(echo "$PB" | py_exact)" = "$(echo "$RB" | py_exact)" ] && echo "$PB" | grep -q '"is_active":false'; then ok "delete → get 200 is_active=false (soft delete, parity)"; else bad "delete → get: PY=$PC RS=$RC"; fi

# ── 11. PRICE-TAGS/RENDER (HTML normalized SVG) ──────────────────────────────
PR='{"template_id":"a0000000-0000-0000-0000-000000000010","products":[{"id":"00000000-0000-0000-0000-000000000001","title":"Хліб білий","price":"25.00","barcode":"4820012345678","article":"ХЛ-001","category":"Хлібобулочні","copies":2},{"id":"00000000-0000-0000-0000-000000000002","title":"Молоко","price":"45.50","barcode":"4820012345679","copies":1}]}'
P=$(curl -s -m 8 -X POST "$PY/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
R=$(curl -s -m 8 -X POST "$RS/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
PP=$(echo "$P"|python3 -c "import sys,json;d=json.load(sys.stdin);print(d['total_pages'],d['total_labels'])")
RP=$(echo "$R"|python3 -c "import sys,json;d=json.load(sys.stdin);print(d['total_pages'],d['total_labels'])")
if [ "$PP" = "$RP" ]; then ok "price-tags meta (pages/labels: $PP)"; else bad "price-tags meta: PY=$PP RS=$RP"; fi
HP=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['html'])"|html_norm)
HR=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['html'])"|html_norm)
if [ "$HP" = "$HR" ]; then ok "price-tags HTML parity (SVG normalized)"; else bad "price-tags HTML"; fi

# ── 12. LABELS/RENDER (print_mode escpos) ────────────────────────────────────
LB='{"template_id":"a0000000-0000-0000-0000-000000000011","products":[{"id":"00000000-0000-0000-0000-000000000001","title":"Хліб білий","price":"25.00","barcode":"4820012345678","copies":2},{"id":"00000000-0000-0000-0000-000000000002","title":"Молоко","price":"45.50","copies":1}],"print_mode":"escpos"}'
P=$(curl -s -m 8 -X POST "$PY/api/v1/print/labels/render" -H "$AUTH" -H "Content-Type: application/json" -d "$LB")
R=$(curl -s -m 8 -X POST "$RS/api/v1/print/labels/render" -H "$AUTH" -H "Content-Type: application/json" -d "$LB")
PL=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['total_labels'])")
RL=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['total_labels'])")
if [ "$PL" = "$RL" ]; then ok "labels total ($PL)"; else bad "labels total: PY=$PL RS=$RL"; fi
HP=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['html'])"|html_norm)
HR=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['html'])"|html_norm)
if [ "$HP" = "$HR" ]; then ok "labels HTML parity (escpos 48mm, SVG normalized)"; else bad "labels HTML"; fi

# ── 13. PRINT/TEST ───────────────────────────────────────────────────────────
# receipt — exact (без SVG)
T='{"print_type":"receipt","template_type":"receipt_58mm"}'
P=$(curl -s -m 8 -X POST "$PY/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$T")
R=$(curl -s -m 8 -X POST "$RS/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$T")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "test receipt parity (exact, message+html)"; else bad "test receipt parity"; fi
# price_tag / label — SVG normalized
for T in price_tag label; do
  TST="{\"print_type\":\"$T\"}"
  P=$(curl -s -m 8 -X POST "$PY/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$TST")
  R=$(curl -s -m 8 -X POST "$RS/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$TST")
  PM=$(echo "$P"|python3 -c "import sys,json;d=json.load(sys.stdin);print(d['message']+'|'+str(d['template_name']))")
  RM=$(echo "$R"|python3 -c "import sys,json;d=json.load(sys.stdin);print(d['message']+'|'+str(d['template_name']))")
  HP=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['preview_html'])"|html_norm)
  HR=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['preview_html'])"|html_norm)
  if [ "$PM" = "$RM" ] && [ "$HP" = "$HR" ]; then ok "test $T parity (message+template+HTML)"; else bad "test $T parity"; echo "  msg: $PM vs $RM"; fi
done
# test невідомий template_type → 404
T='{"print_type":"receipt","template_type":"unknown_xyz"}'
P=$(curl -s -m 5 -w "\n%{http_code}" -X POST "$PY/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$T")
R=$(curl -s -m 5 -w "\n%{http_code}" -X POST "$RS/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$T")
PC=${P##*$'\n'}; RC=${R##*$'\n'}; PB=${P%$'\n'*}; RB=${R%$'\n'*}
if [ "$PC" = "404" ] && [ "$RC" = "404" ] && [ "$PB" = "$RB" ]; then ok "test unknown template_type 404 parity (code+detail)"; else bad "test 404: PY=$PC RS=$RC $PB vs $RB"; fi

# ── 14. 404: неактивний шаблон / неіснуючий ─────────────────────────────────
INACTIVE="9d9b552d-ccdf-44d7-b9bb-d3cc2240117a"  # custom, is_active=false
PR='{"template_id":"'$INACTIVE'","products":[{"id":"00000000-0000-0000-0000-000000000001","title":"Х","price":"1.00"}]}'
P=$(curl -s -m 5 -X POST "$PY/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ] && echo "$P" | grep -q "не знайдено або він неактивний"; then ok "price-tags render неактивний 404 parity"; else bad "render неактивний 404: $P vs $R"; fi
GONE="00000000-0000-0000-0000-00000000dead"
P=$(curl -s -m 5 -w "\n%{http_code}" "$PY/api/v1/print-templates/$GONE" -H "$AUTH")
R=$(curl -s -m 5 -w "\n%{http_code}" "$RS/api/v1/print-templates/$GONE" -H "$AUTH")
PC=${P##*$'\n'}; RC=${R##*$'\n'}; PB=${P%$'\n'*}; RB=${R%$'\n'*}
if [ "$PC" = "404" ] && [ "$RC" = "404" ] && [ "$(echo "$PB" | py_exact)" = "$(echo "$RB" | py_exact)" ]; then ok "get неіснуючий 404 parity"; else bad "get 404: PY=$PC RS=$RC"; fi

# ── 15. Pydantic 422 (detail 1:1) ────────────────────────────────────────────
# list page=0
P=$(curl -s -m 5 "$PY/api/v1/print-templates?page=0" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates?page=0" -H "$AUTH")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 list page=0 parity"; else bad "422 page=0: $P vs $R"; fi
# list size=0
P=$(curl -s -m 5 "$PY/api/v1/print-templates?size=0" -H "$AUTH")
R=$(curl -s -m 5 "$RS/api/v1/print-templates?size=0" -H "$AUTH")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 list size=0 parity"; else bad "422 size=0"; fi
# price-tags barcode_type=foo
PR='{"template_id":"a0000000-0000-0000-0000-000000000010","products":[{"id":"00000000-0000-0000-0000-000000000001","title":"Х","price":"1.00"}],"barcode_type":"foo"}'
P=$(curl -s -m 5 -X POST "$PY/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 barcode_type=foo parity"; else bad "422 barcode_type"; fi
# price-tags width=5
PR='{"template_id":"a0000000-0000-0000-0000-000000000010","products":[{"id":"00000000-0000-0000-0000-000000000001","title":"Х","price":"1.00"}],"width_mm":5}'
P=$(curl -s -m 5 -X POST "$PY/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 width_mm=5 parity"; else bad "422 width"; fi
# products empty
PR='{"template_id":"a0000000-0000-0000-0000-000000000010","products":[]}'
P=$(curl -s -m 5 -X POST "$PY/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 products empty parity"; else bad "422 products empty"; fi
# test print_type=bad
P=$(curl -s -m 5 -X POST "$PY/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d '{"print_type":"bad"}')
R=$(curl -s -m 5 -X POST "$RS/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d '{"print_type":"bad"}')
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 test print_type=bad parity"; else bad "422 print_type"; fi
# create name empty
CB='{"name":"","type":"custom","content":"x"}'
P=$(curl -s -m 5 -X POST "$PY/api/v1/print-templates" -H "$AUTH" -H "Content-Type: application/json" -d "$CB")
R=$(curl -s -m 5 -X POST "$RS/api/v1/print-templates" -H "$AUTH" -H "Content-Type: application/json" -d "$CB")
if [ "$(echo "$P" | py_exact)" = "$(echo "$R" | py_exact)" ]; then ok "422 create name empty parity"; else bad "422 name empty"; fi

# ── 16. PRODUCTS без barcode → "" (1:1) ─────────────────────────────────────
PR='{"template_id":"a0000000-0000-0000-0000-000000000010","products":[{"id":"00000000-0000-0000-0000-000000000001","title":"Без штрихкоду","price":"9.99"}]}'
P=$(curl -s -m 8 -X POST "$PY/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
R=$(curl -s -m 8 -X POST "$RS/api/v1/print/price-tags/render" -H "$AUTH" -H "Content-Type: application/json" -d "$PR")
HP=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['html'])"|html_norm)
HR=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['html'])"|html_norm)
if [ "$HP" = "$HR" ]; then ok "products без barcode HTML parity"; else bad "без barcode HTML"; fi

# ── 17. TEST з template_id (явний) ───────────────────────────────────────────
T='{"print_type":"price_tag","template_id":"a0000000-0000-0000-0000-000000000010","width_mm":50,"height_mm":30}'
P=$(curl -s -m 8 -X POST "$PY/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$T")
R=$(curl -s -m 8 -X POST "$RS/api/v1/print/test" -H "$AUTH" -H "Content-Type: application/json" -d "$T")
PM=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['message'])")
RM=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['message'])")
HP=$(echo "$P"|python3 -c "import sys,json;print(json.load(sys.stdin)['preview_html'])"|html_norm)
HR=$(echo "$R"|python3 -c "import sys,json;print(json.load(sys.stdin)['preview_html'])"|html_norm)
if [ "$PM" = "$RM" ] && [ "$HP" = "$HR" ]; then ok "test з template_id+розмірами parity"; else bad "test template_id"; fi

# ── CLEANUP ──────────────────────────────────────────────────────────────────
PGPASSWORD=VgxWd7MBJ10X psql -h localhost -U postgres -d pos_system -q -c "DELETE FROM print_templates WHERE id IN ('$PT_ID','$PID_P','$PID_R') OR name LIKE 'Diff PT %' OR name LIKE 'Diff PT%'" 2>/dev/null
echo "cleanup done"

echo ""
echo "═══ РЕЗУЛЬТАТ: PASS=$PASS FAIL=$FAIL ═══"
[ "$FAIL" = "0" ] && echo "E2E PRINT: ALL PASS ✅" || echo "E2E PRINT: FAILURES ❌"
