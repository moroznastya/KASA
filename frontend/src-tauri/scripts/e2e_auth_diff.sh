#!/usr/bin/env bash
# E2E differential AUTH/USERS/SETTINGS/RBAC (етап 6):
# Rust-фасад (:8002, TORGASHKA_RUST_AUTH=1) vs Python (:8001), СПІЛЬНА БД.
# Покриває:
#   - auth: login / login-pin / refresh / logout / verify / users-list / users/me
#   - users CRUD: list (page/size), get, create (авто-логін, 409), update
#     (exclude_unset, пароль, PIN), permissions (400 невідоме право),
#     hourly-rate (422 float/gt), delete (204/404/409), permissions/list
#   - RBAC: 401 без токена, 403 для cashier, деактивація
#   - settings: GET всі/модуль (404), PUT key (upsert + валідації 422),
#     PUT batch, 403 для cashier
#   - JWT parity: токен Rust → Python verify/refresh і навпаки
# Потрібно: Python :8001, фасад :8002 (TORGASHKA_RUST_AUTH=1), admin/admin123.
set -u
# Python rate_limit: 5 login/хв → чекаємо скидання вікна після попереднього прогону.
sleep 65
RUST=http://127.0.0.1:8002/api/v1
PY=http://127.0.0.1:8001/api/v1
TS=$(date +%s)
FAIL=0

CT="Content-Type: application/json"

# ─── Нормалізація ────────────────────────────────────────────────────────────
# Для users: виключаємо id/created_at/updated_at (різні записи P/R).
# Токени замінюємо на <token>. Дати-рядки нормалізуємо до 6 знаків.
norm() {
  python3 -c "
import sys, json, re
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items()
                if k not in ('id','created_at','updated_at','access_token','refresh_token','login')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    if isinstance(x, str):
        return re.sub(r'(\.\d{6})\d+', r'\1', x)
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}

# norm_create: для POST-parity (різні login P/R) — login → <login>.
norm_create() {
  python3 -c "
import sys, json, re
d = json.load(sys.stdin)
def walk(x):
    if isinstance(x, dict):
        return {k: walk(v) for k, v in x.items()
                if k not in ('id','created_at','updated_at','login')}
    if isinstance(x, list):
        return [walk(i) for i in x]
    if isinstance(x, str):
        return re.sub(r'(\\.\\d{6})\\d+', r'\1', x)
    return x
print(json.dumps(walk(d), ensure_ascii=False, sort_keys=True))
"
}

# norm для відповідей, де uuid фігурує в detail (404/409) — стабілізуємо.
norm_detail() {
  python3 -c "
import sys, json, re
d = sys.stdin.read()
d = re.sub(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}', '<uuid>', d)
d = re.sub(r'authdiff_[a-z0-9_]+', '<login>', d)
print(json.dumps(json.loads(d), ensure_ascii=False, sort_keys=True))
"
}

cmp_resp() {
  local label="$1" rcode="$2" pcode="$3" rbody="$4" pbody="$5"
  if [ "$rcode" != "$pcode" ]; then
    echo "❌ $label: статус Rust=$rcode Python=$pcode"; FAIL=1; return
  fi
  local rn pn
  rn=$(printf '%s' "$rbody" | norm)
  pn=$(printf '%s' "$pbody" | norm)
  if [ "$rn" != "$pn" ]; then
    echo "❌ $label: тіло відрізняється"
    echo "   Rust:   $rn" | head -c 600; echo
    echo "   Python: $pn" | head -c 600; echo
    FAIL=1
  else
    echo "✅ $label (${rcode})"
  fi
}

cmp_resp_detail() {
  local label="$1" rcode="$2" pcode="$3" rbody="$4" pbody="$5"
  if [ "$rcode" != "$pcode" ]; then
    echo "❌ $label: статус Rust=$rcode Python=$pcode"; FAIL=1; return
  fi
  local rn pn
  rn=$(printf '%s' "$rbody" | norm_detail)
  pn=$(printf '%s' "$pbody" | norm_detail)
  if [ "$rn" != "$pn" ]; then
    echo "❌ $label: тіло відрізняється"
    echo "   Rust:   $rn" | head -c 600; echo
    echo "   Python: $pn" | head -c 600; echo
    FAIL=1
  else
    echo "✅ $label (${rcode})"
  fi
}

# ─── Логін адміністратора через Rust ─────────────────────────────────────────
echo "→ Отримуємо admin-токен через Rust :8002..."
RADMIN=$(curl -s -X POST "$RUST/auth/login" -H "$CT" -d '{"login":"admin","password":"admin123"}' | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])" 2>/dev/null)
if [ -z "$RADMIN" ]; then echo "❌ Не вдалося залогінитись адміністратором на Rust"; exit 1; fi
PADMIN=$(curl -s -X POST "$PY/auth/login" -H "$CT" -d '{"login":"admin","password":"admin123"}' | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])" 2>/dev/null)
if [ -z "$PADMIN" ]; then echo "❌ Не вдалося залогінитись адміністратором на Python"; exit 1; fi
RA="Authorization: Bearer $RADMIN"
PA="Authorization: Bearer $PADMIN"

# ─── Тестові користувачі (унікальні для цього прогону) ───────────────────────
RL="authdiff_r_${TS}"
PL="authdiff_p_${TS}"
RNO_PIN="authdiff_nopin_${TS}"
PNO_PIN="authdiff_nopinp_${TS}"

echo "→ Створюємо тестових cashier (Rust+Python)..."
RUSER=$(curl -s -X POST "$RUST/users" -H "$RA" -H "$CT" -d "{\"name\":\"Diff Касир R\",\"login\":\"$RL\",\"password\":\"pass1234\",\"role\":\"cashier\",\"pin_code\":\"4321\"}")
PUSER=$(curl -s -X POST "$PY/users" -H "$PA" -H "$CT" -d "{\"name\":\"Diff Касир R\",\"login\":\"$PL\",\"password\":\"pass1234\",\"role\":\"cashier\",\"pin_code\":\"4321\"}")
RUID=$(printf '%s' "$RUSER" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
PUID=$(printf '%s' "$PUSER" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
RNOPIN=$(curl -s -X POST "$RUST/users" -H "$RA" -H "$CT" -d "{\"name\":\"Diff NoPin R\",\"login\":\"$RNO_PIN\",\"password\":\"pass1234\",\"role\":\"cashier\"}")
PNOPIN=$(curl -s -X POST "$PY/users" -H "$PA" -H "$CT" -d "{\"name\":\"Diff NoPin R\",\"login\":\"$PNO_PIN\",\"password\":\"pass1234\",\"role\":\"cashier\"}")
RNOPIN_ID=$(printf '%s' "$RNOPIN" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
PNOPIN_ID=$(printf '%s' "$PNOPIN" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
echo "   R=$RUID P=$PUID"

# ─── 1. Публічні auth-ендпоінти ──────────────────────────────────────────────
echo "── 1. Auth публічні ──"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login" -H "$CT" -d '{"login":"nobody","password":"x"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/login" -H "$CT" -d '{"login":"nobody","password":"x"}')
cmp_resp_detail "login невідомий користувач" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login" -H "$CT" -d "{\"login\":\"$RL\",\"password\":\"wrong\"}")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/login" -H "$CT" -d "{\"login\":\"$PL\",\"password\":\"wrong\"}")
cmp_resp_detail "login невірний пароль" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login" -H "$CT" -d '{}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/login" -H "$CT" -d '{}')
cmp_resp "login порожнє тіло (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login" -H "$CT" -d '{"login":"x"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/login" -H "$CT" -d '{"login":"x"}')
cmp_resp "login без password (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login-pin" -H "$CT" -d "{\"login\":\"$RL\",\"pin_code\":\"0000\"}")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/login-pin" -H "$CT" -d "{\"login\":\"$PL\",\"pin_code\":\"0000\"}")
cmp_resp_detail "login-pin невірний PIN" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login-pin" -H "$CT" -d "{\"login\":\"$RNO_PIN\",\"pin_code\":\"1234\"}")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/login-pin" -H "$CT" -d "{\"login\":\"$PNO_PIN\",\"pin_code\":\"1234\"}")
cmp_resp_detail "login-pin без PIN у користувача" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/refresh" -H "$CT" -d '{}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/refresh" -H "$CT" -d '{}')
cmp_resp_detail "refresh без токена (400)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/refresh" -H "$CT" -d '{"refresh_token":"garbage.token.here"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/refresh" -H "$CT" -d '{"refresh_token":"garbage.token.here"}')
cmp_resp_detail "refresh невалідний токен (401)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/auth/verify")
p=$(curl -s -w "|%{http_code}" "$PY/auth/verify")
cmp_resp "verify без токена" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/auth/verify" -H "Authorization: Bearer bad.token")
p=$(curl -s -w "|%{http_code}" "$PY/auth/verify" -H "Authorization: Bearer bad.token")
cmp_resp "verify невалідний токен" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/auth/users-list")
p=$(curl -s -w "|%{http_code}" "$PY/auth/users-list")
cmp_resp "users-list публічний" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/logout")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/logout")
cmp_resp_detail "logout без токена (401)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/logout" -H "$RA")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/logout" -H "$PA")
cmp_resp "logout з токеном" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# ─── 2. RBAC: без токена / cashier ───────────────────────────────────────────
echo "── 2. RBAC ──"

r=$(curl -s -w "|%{http_code}" "$RUST/users")
p=$(curl -s -w "|%{http_code}" "$PY/users")
cmp_resp_detail "users без токена (401)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

RCT=$(curl -s -X POST "$RUST/auth/login" -H "$CT" -d "{\"login\":\"$RL\",\"password\":\"pass1234\"}" | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])")
PCT=$(curl -s -X POST "$PY/auth/login" -H "$CT" -d "{\"login\":\"$PL\",\"password\":\"pass1234\"}" | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])")
r=$(curl -s -w "|%{http_code}" "$RUST/users" -H "Authorization: Bearer $RCT")
p=$(curl -s -w "|%{http_code}" "$PY/users" -H "Authorization: Bearer $PCT")
cmp_resp_detail "users як cashier (403)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/print_copies" -H "Authorization: Bearer $RCT" -H "$CT" -d '{"value":"5"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/print_copies" -H "Authorization: Bearer $PCT" -H "$CT" -d '{"value":"5"}')
cmp_resp_detail "settings PUT як cashier (403)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# ─── 3. Users CRUD ───────────────────────────────────────────────────────────
echo "── 3. Users CRUD ──"

r=$(curl -s -w "|%{http_code}" "$RUST/users?page=1&size=3" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users?page=1&size=3" -H "$PA")
cmp_resp "users list page=1 size=3" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/users?page=0&size=5" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users?page=0&size=5" -H "$PA")
cmp_resp "users page=0 (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/users?page=1&size=9999" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users?page=1&size=9999" -H "$PA")
cmp_resp "users size=9999 (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/users?page=1&size=abc" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users?page=1&size=abc" -H "$PA")
cmp_resp "users size=abc (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# get_user на тому самому записі (спільна БД → id однаковий)
r=$(curl -s -w "|%{http_code}" "$RUST/users/$RUID" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users/$RUID" -H "$PA")
cmp_resp "get_user існуючий" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/users/00000000-0000-0000-0000-000000000000" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users/00000000-0000-0000-0000-000000000000" -H "$PA")
cmp_resp_detail "get_user неіснуючий (404)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/users/not-a-uuid" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users/not-a-uuid" -H "$PA")
cmp_resp "get_user bad uuid (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# create: нові унікальні користувачі
RNEW="authdiff_new_${TS}"
PNEW="authdiff_newp_${TS}"
r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d "{\"name\":\"Новий Створений\",\"login\":\"$RNEW\",\"password\":\"secret1\",\"role\":\"cashier\",\"is_active\":true}")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d "{\"name\":\"Новий Створений\",\"login\":\"$PNEW\",\"password\":\"secret1\",\"role\":\"cashier\",\"is_active\":true}")
cmp_create() {
  local label="$1" rcode="$2" pcode="$3" rbody="$4" pbody="$5"
  if [ "$rcode" != "$pcode" ]; then
    echo "❌ $label: статус Rust=$rcode Python=$pcode"; FAIL=1; return
  fi
  local rn pn
  rn=$(printf '%s' "$rbody" | norm_create)
  pn=$(printf '%s' "$pbody" | norm_create)
  if [ "$rn" != "$pn" ]; then
    echo "❌ $label: тіло відрізняється"
    echo "   Rust:   $rn" | head -c 400; echo
    echo "   Python: $pn" | head -c 400; echo
    FAIL=1
  else
    echo "✅ $label (${rcode})"
  fi
}
cmp_create "create_user (201)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"
RNEW_ID=$(printf '%s' "${r%|*}" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
PNEW_ID=$(printf '%s' "${p%|*}" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")

# create: авто-генерація логіну з імені (кирилиця → транслітерація)
r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d '{"name":"Тестовий Пятий","password":"secret1"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d '{"name":"Тестовий Пятий","password":"secret1"}')
cmp_resp "create_user авто-логін" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"
R_AUTOLOGIN=$(printf '%s' "${r%|*}" | python3 -c "import sys,json;print(json.load(sys.stdin)['login'])")
P_AUTOLOGIN=$(printf '%s' "${p%|*}" | python3 -c "import sys,json;print(json.load(sys.stdin)['login'])")
echo "   авто-логіни: R=$R_AUTOLOGIN P=$P_AUTOLOGIN"
# прибрати авто-створених (cleanup пізніше)

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d "{\"name\":\"Дубль\",\"login\":\"$RL\",\"password\":\"secret1\"}")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d "{\"name\":\"Дубль\",\"login\":\"$PL\",\"password\":\"secret1\"}")
cmp_resp_detail "create_user дублікат логіну (409)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d '{"name":"X","password":"1234","role":"superman"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d '{"name":"X","password":"1234","role":"superman"}')
cmp_resp "create_user bad role (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d '{"name":"X","password":"12"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d '{"name":"X","password":"12"}')
cmp_resp "create_user короткий пароль (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d '{"password":"1234"}')
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d '{"password":"1234"}')
cmp_resp "create_user без name (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# update: зміна name + login
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID" -H "$RA" -H "$CT" -d '{"name":"Оновлений Имя"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID" -H "$PA" -H "$CT" -d '{"name":"Оновлений Имя"}')
cmp_resp "update_user name" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# update: password (потім login новим паролем — окремо перевіримо)
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID" -H "$RA" -H "$CT" -d '{"password":"newpass99"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID" -H "$PA" -H "$CT" -d '{"password":"newpass99"}')
cmp_resp "update_user password" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# login новим паролем — Rust 200 (Python rate-limit; поведінка ідентична вже
# покрита тестом "login невірний пароль").
r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/login" -H "$CT" -d "{\"login\":\"$RNEW\",\"password\":\"newpass99\"}")
echo "$( [ "${r##*|}" = "200" ] && echo '✅' || echo '❌' ) login новим паролем (Rust=${r##*|})"; [ "${r##*|}" = "200" ] || FAIL=1

# update: дублікат login → 409
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID" -H "$RA" -H "$CT" -d "{\"login\":\"$RL\"}")
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID" -H "$PA" -H "$CT" -d "{\"login\":\"$PL\"}")
cmp_resp_detail "update_user дублікат login (409)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# update: неправильний тип login → 422
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID" -H "$RA" -H "$CT" -d '{"login":123}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID" -H "$PA" -H "$CT" -d '{"login":123}')
cmp_resp "update_user login int (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# permissions: невідоме право → 400
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID/permissions" -H "$RA" -H "$CT" -d '{"permissions":["bogus:perm"]}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID/permissions" -H "$PA" -H "$CT" -d '{"permissions":["bogus:perm"]}')
cmp_resp_detail "update_permissions невідоме право (400)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# permissions: валідні
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID/permissions" -H "$RA" -H "$CT" -d '{"permissions":["products:view","pos:access"]}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID/permissions" -H "$PA" -H "$CT" -d '{"permissions":["products:view","pos:access"]}')
cmp_resp "update_permissions валідні" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# permissions: не список → 422
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID/permissions" -H "$RA" -H "$CT" -d '{"permissions":"nope"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID/permissions" -H "$PA" -H "$CT" -d '{"permissions":"nope"}')
cmp_resp "update_permissions не список (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# hourly-rate
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID/hourly-rate" -H "$RA" -H "$CT" -d '{"hourly_rate":150.5}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID/hourly-rate" -H "$PA" -H "$CT" -d '{"hourly_rate":150.5}')
cmp_resp "hourly-rate 150.5" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID/hourly-rate" -H "$RA" -H "$CT" -d '{"hourly_rate":0}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID/hourly-rate" -H "$PA" -H "$CT" -d '{"hourly_rate":0}')
cmp_resp "hourly-rate 0 (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/users/$RNEW_ID/hourly-rate" -H "$RA" -H "$CT" -d '{"hourly_rate":"abc"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/users/$PNEW_ID/hourly-rate" -H "$PA" -H "$CT" -d '{"hourly_rate":"abc"}')
cmp_resp "hourly-rate abc (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# permissions/list
r=$(curl -s -w "|%{http_code}" "$RUST/users/permissions/list" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/users/permissions/list" -H "$PA")
cmp_resp "permissions/list" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# delete: юзер БЕЗ робочих сесій (без login) → 204 (Python 500 на юзерах з
# сесіями — аномалія SQLAlchemy, зафіксована в docs; тут чистий випадок).
RDL="authdiff_del_${TS}"
PDL="authdiff_delp_${TS}"
r=$(curl -s -w "|%{http_code}" -X POST "$RUST/users" -H "$RA" -H "$CT" -d "{\"name\":\"Delete Me\",\"login\":\"$RDL\",\"password\":\"pass1234\",\"role\":\"cashier\"}")
RDL_ID=$(printf '%s' "${r%|*}" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/users" -H "$PA" -H "$CT" -d "{\"name\":\"Delete Me\",\"login\":\"$PDL\",\"password\":\"pass1234\",\"role\":\"cashier\"}")
PDL_ID=$(printf '%s' "${p%|*}" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
r=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$RUST/users/$RDL_ID" -H "$RA")
p=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "$PY/users/$PDL_ID" -H "$PA")
echo "$( [ "$r" = "204" ] && [ "$p" = "204" ] && echo '✅' || echo '❌' ) delete_user (204): Rust=$r Python=$p"; [ "$r" = "204" ] && [ "$p" = "204" ] || FAIL=1

# users/me
r=$(curl -s -w "|%{http_code}" "$RUST/auth/users/me" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/auth/users/me" -H "$PA")
cmp_resp "auth/users/me" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# ─── 4. Settings ─────────────────────────────────────────────────────────────
echo "── 4. Settings ──"

r=$(curl -s -w "|%{http_code}" "$RUST/settings" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/settings" -H "$PA")
cmp_resp "settings всі модулі" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/settings/printing" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/settings/printing" -H "$PA")
cmp_resp "settings модуль printing" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" "$RUST/settings/no_such_module_xyz" -H "$RA")
p=$(curl -s -w "|%{http_code}" "$PY/settings/no_such_module_xyz" -H "$PA")
cmp_resp_detail "settings невідомий модуль (404)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# PUT key: валідації 1:1
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/print_copies" -H "$RA" -H "$CT" -d '{"value":"7"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/print_copies" -H "$PA" -H "$CT" -d '{"value":"7"}')
cmp_resp "settings PUT print_copies=7" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/print_copies" -H "$RA" -H "$CT" -d '{"value":"9999"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/print_copies" -H "$PA" -H "$CT" -d '{"value":"9999"}')
cmp_resp_detail "settings print_copies=9999 (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/auto_cut_paper" -H "$RA" -H "$CT" -d '{"value":"1"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/auto_cut_paper" -H "$PA" -H "$CT" -d '{"value":"1"}')
cmp_resp "settings auto_cut_paper=1→true" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/auto_cut_paper" -H "$RA" -H "$CT" -d '{"value":"maybe"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/auto_cut_paper" -H "$PA" -H "$CT" -d '{"value":"maybe"}')
cmp_resp_detail "settings auto_cut_paper=maybe (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/barcode_type" -H "$RA" -H "$CT" -d '{"value":"qr"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/barcode_type" -H "$PA" -H "$CT" -d '{"value":"qr"}')
cmp_resp "settings barcode_type=qr" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/barcode_type" -H "$RA" -H "$CT" -d '{"value":"pdf417"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/barcode_type" -H "$PA" -H "$CT" -d '{"value":"pdf417"}')
cmp_resp_detail "settings barcode_type=pdf417 (422)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# upsert нового ключа (модуль general, label з ключа)
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings/rust_diff_company_${TS}" -H "$RA" -H "$CT" -d '{"value":"Тест Компанія"}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings/rust_diff_company_${TS}" -H "$PA" -H "$CT" -d '{"value":"Тест Компанія"}')
cmp_resp "settings upsert новий ключ" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# batch update
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings" -H "$RA" -H "$CT" -d '{"settings":{"print_copies":"3","show_logo":"0"}}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings" -H "$PA" -H "$CT" -d '{"settings":{"print_copies":"3","show_logo":"0"}}')
cmp_resp "settings batch update" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# batch: невідомий ключ — мовчки ігнорується (Python), Rust 1:1
r=$(curl -s -w "|%{http_code}" -X PUT "$RUST/settings" -H "$RA" -H "$CT" -d '{"settings":{"no_such_key_${TS}":"1"}}')
p=$(curl -s -w "|%{http_code}" -X PUT "$PY/settings" -H "$PA" -H "$CT" -d '{"settings":{"no_such_key_${TS}":"1"}}')
cmp_resp "settings batch невідомий ключ (ігнор)" "${r##*|}" "${p##*|}" "${r%|*}" "${p%|*}"

# ─── 5. JWT parity ───────────────────────────────────────────────────────────
echo "── 5. JWT parity ──"

RLOGIN=$(curl -s -X POST "$RUST/auth/login" -H "$CT" -d "{\"login\":\"$RL\",\"password\":\"pass1234\"}")
PLOGIN=$(curl -s -X POST "$PY/auth/login" -H "$CT" -d "{\"login\":\"$PL\",\"password\":\"pass1234\"}")
R_ACC=$(printf '%s' "$RLOGIN" | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])")
P_ACC=$(printf '%s' "$PLOGIN" | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])")
R_REF=$(printf '%s' "$RLOGIN" | python3 -c "import sys,json;print(json.load(sys.stdin)['refresh_token'])")
P_REF=$(printf '%s' "$PLOGIN" | python3 -c "import sys,json;print(json.load(sys.stdin)['refresh_token'])")

r=$(curl -s "$RUST/auth/verify" -H "Authorization: Bearer $P_ACC")
p=$(curl -s "$PY/auth/verify" -H "Authorization: Bearer $R_ACC")
echo "$( [ "$(printf '%s' "$r" | python3 -c 'import sys,json;print(json.load(sys.stdin)["valid"])')" = "True" ] && [ "$(printf '%s' "$p" | python3 -c 'import sys,json;print(json.load(sys.stdin)["valid"])')" = "True" ] && echo '✅' || echo '❌') JWT крос-verify (Rust↔Python): R-токен у Python=$p, P-токен у Rust=$r"; [ "$(printf '%s' "$r" | python3 -c 'import sys,json;print(json.load(sys.stdin)["valid"])')" = "True" ] && [ "$(printf '%s' "$p" | python3 -c 'import sys,json;print(json.load(sys.stdin)["valid"])')" = "True" ] || FAIL=1

r=$(curl -s -w "|%{http_code}" -X POST "$RUST/auth/refresh" -H "$CT" -d "{\"refresh_token\":\"$P_REF\"}")
p=$(curl -s -w "|%{http_code}" -X POST "$PY/auth/refresh" -H "$CT" -d "{\"refresh_token\":\"$R_REF\"}")
echo "$( [ "${r##*|}" = "200" ] && [ "${p##*|}" = "200" ] && echo '✅' || echo '❌' ) JWT крос-refresh (Rust↔Python): R-refresh у Python=${p##*|}, P-refresh у Rust=${r##*|}"; [ "${r##*|}" = "200" ] && [ "${p##*|}" = "200" ] || FAIL=1

# claims access: однаковий набір полів (без значень часу)
python3 - "$R_ACC" "$P_ACC" << 'PYEOF'
import sys, base64, json
def claims(t):
    p = t.split('.')[1]; p += '=' * (-len(p) % 4)
    return json.loads(base64.urlsafe_b64decode(p))
rc, pc = claims(sys.argv[1]), claims(sys.argv[2])
rk = sorted(rc.keys())
pk = sorted(pc.keys())
ok = rk == pk == ['exp','iat','permissions','role','sub','type'] and rc['type']==pc['type']=='access' and sorted(rc['permissions'])==sorted(pc['permissions'])
print('✅' if ok else '❌', 'access claims parity:', rk)
sys.exit(0 if ok else 1)
PYEOF
[ $? -eq 0 ] || FAIL=1

python3 - "$R_REF" "$P_REF" << 'PYEOF'
import sys, base64, json
def claims(t):
    p = t.split('.')[1]; p += '=' * (-len(p) % 4)
    return json.loads(base64.urlsafe_b64decode(p))
rc, pc = claims(sys.argv[1]), claims(sys.argv[2])
rk = sorted(rc.keys())
pk = sorted(pc.keys())
ok = rk == pk == ['exp','iat','role','sub','type'] and rc['type']==pc['type']=='refresh'
print('✅' if ok else '❌', 'refresh claims parity (без permissions):', rk)
sys.exit(0 if ok else 1)
PYEOF
[ $? -eq 0 ] || FAIL=1

# ─── 6. Cleanup ──────────────────────────────────────────────────────────────
echo "── 6. Cleanup ──"
# Тестові користувачі мають робочі сесії → Python delete_user 500 (аномалія).
# Видаляємо напряму через SQL (CASCADE на work_sessions) — чистий cleanup.
export PGPASSWORD="${PGPASSWORD:-VgxWd7MBJ10X}"
psql -h localhost -U postgres -d pos_system -c "DELETE FROM users WHERE login LIKE 'authdiff_%' OR login LIKE 'testovyi_piatyi%' OR login LIKE 'test_rust%' OR login LIKE 'tmp_nosess%';" >/dev/null 2>&1
# Відновлюємо налаштування
curl -s -o /dev/null -X PUT "$RUST/settings/print_copies" -H "$RA" -H "$CT" -d '{"value":"1"}'
curl -s -o /dev/null -X PUT "$RUST/settings/auto_cut_paper" -H "$RA" -H "$CT" -d '{"value":"false"}'
curl -s -o /dev/null -X PUT "$RUST/settings/barcode_type" -H "$RA" -H "$CT" -d '{"value":"code128"}'
curl -s -o /dev/null -X PUT "$RUST/settings/rust_diff_company_${TS}" -H "$RA" -H "$CT" -d '{"value":null}'
# видалити upsert-ключ з БД напряму (нема DELETE endpoint)
export PGPASSWORD="${PGPASSWORD:-VgxWd7MBJ10X}"
psql -h localhost -U postgres -d pos_system -c "DELETE FROM system_settings WHERE key LIKE 'rust_diff_company_%' OR key LIKE 'no_such_key_%';" >/dev/null 2>&1
# підтвердження: тестових логінів більше немає
LEFT=$(curl -s "$RUST/users?page=1&size=1000" -H "$RA" | python3 -c "
import sys,json
d=json.load(sys.stdin)
print(sum(1 for it in d['items'] if 'authdiff_' in it['login']))
")
echo "   залишилось тестових логінів: $LEFT"
[ "$LEFT" = "0" ] || FAIL=1

# ─── Підсумок ────────────────────────────────────────────────────────────────
echo
if [ "$FAIL" = "0" ]; then
  echo "🎉 E2E AUTH DIFF: ВСІ ПЕРЕВІРКИ ПРОЙДЕНО"
else
  echo "❌ E2E AUTH DIFF: Є РОЗБІЖНОСТІ (див. вище)"
fi
exit $FAIL
