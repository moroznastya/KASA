#!/usr/bin/env python3
"""Differential ALIAS: PY v1 (deprecated) vs RS v1/v2-аліаси — 1:1.

Перевіряє:
  GET /api/v1/receipts (list), /{id}, /{id}/items, /search, /stats/today,
      /products/{id}/returnable-quantity, /by-product/{q}/recent-sales
  POST/DELETE /api/v1/products/{id}/barcodes
  GET/POST /api/v2/prro/status|queue|sync (аліаси без /fiscal)

Метод: створює дані через PY v1 POST, потім порівнює GET PY v1 vs GET RS v1
на ТИХ САМИХ id (обидва читають з однієї БД).
"""
import json
import time
import urllib.request
import urllib.error
import uuid

PY = "http://127.0.0.1:8001"
RS = "http://127.0.0.1:8002"
TOKEN = open("/tmp/torgashka_token").read().strip()
PASS = 0
FAIL = 0


def req(base, method, path, body=None, raw=None, ctype="application/json"):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    headers = {"Authorization": f"Bearer {TOKEN}"}
    if ctype:
        headers["Content-Type"] = ctype
    r = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(r) as resp:
            raw = resp.read()
            return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        try:
            raw = e.read()
            return e.code, (json.loads(raw) if raw else None)
        except Exception:
            return e.code, None


def norm(v, drop=None):
    """Видаляє генераційні/ідентифікаційні поля для порівняння структури."""
    drop = set(drop or [])
    if isinstance(v, dict):
        out = {}
        for k, val in v.items():
            if k in drop:
                continue
            if k == "items" and isinstance(val, list):
                out[k] = [norm(x, drop) for x in val]
            elif k == "recent_sales" and isinstance(val, list):
                out[k] = [norm(x, drop) for x in val]
            elif isinstance(val, (dict, list)):
                out[k] = norm(val, drop)
            else:
                out[k] = val
        return out
    if isinstance(v, list):
        return [norm(x, drop) for x in v]
    return v


def check(name, a, b, drop=None):
    global PASS, FAIL
    na, nb = norm(a, drop), norm(b, drop)
    if na == nb:
        PASS += 1
        print(f"  OK {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}")
        print(f"    exp: {json.dumps(na, ensure_ascii=False, default=str)[:400]}")
        print(f"    got: {json.dumps(nb, ensure_ascii=False, default=str)[:400]}")


# ── Підготовка даних через PY ────────────────────────────────────────────────
st, prod = req(PY, "POST", "/api/v1/products", {
    "title": f"ALIAS-DIFF {int(time.time())}", "price": "50.00", "cost_price": "40.00",
    "stock": "10", "unit": "шт", "barcode": f"ALIAS-{int(time.time())}",
})
pid, barcode = prod["id"], prod["barcode"]

st, r1 = req(PY, "POST", "/api/v1/receipts", {
    "receipt_number": f"ALIAS-RCPT-{int(time.time())}", "receipt_type": "sale",
    "total_amount": "100.00", "paid_amount": "100.00", "payment_method": "cash",
    "items": [
        {"product_id": pid, "quantity": "1", "price": "50.00", "total": "50.00"},
        {"product_id": pid, "quantity": "1", "price": "50.00", "total": "50.00"},
    ],
})
rid = r1["id"]

st, r2 = req(PY, "POST", "/api/v1/receipts", {
    "receipt_number": f"ALIAS-RTN-{int(time.time())}", "receipt_type": "return",
    "total_amount": "50.00", "paid_amount": "50.00", "payment_method": "cash",
    "original_receipt_id": rid,
    "items": [{"product_id": pid, "quantity": "1", "price": "50.00", "total": "50.00"}],
})
print(f"чек sale={rid} return={r2.get('id')} status={st}")

DROP = {"id", "receipt_id", "created_at", "cashier_id", "debtor_id", "receipt_number",
        "total_amount", "paid_amount", "change_amount", "cashier_name"}

# ── GET /{id} ─────────────────────────────────────────────────────────────────
s1, g_py = req(PY, "GET", f"/api/v1/receipts/{rid}")
s2, g_rs = req(RS, "GET", f"/api/v1/receipts/{rid}")
check(f"GET /receipts/{{id}} status ({s1} R==P)", s1, s2)
check("GET /receipts/{id} parity", g_py, g_rs, DROP)

# ── GET /{id}/items ───────────────────────────────────────────────────────────
s1, i_py = req(PY, "GET", f"/api/v1/receipts/{rid}/items")
s2, i_rs = req(RS, "GET", f"/api/v1/receipts/{rid}/items")
check(f"GET /receipts/{{id}}/items status ({s1} R==P)", s1, s2)
check("GET /receipts/{id}/items parity", i_py, i_rs, {"id", "receipt_id", "created_at"})

# ── GET list ──────────────────────────────────────────────────────────────────
s1, l_py = req(PY, "GET", "/api/v1/receipts?page=1&size=20")
s2, l_rs = req(RS, "GET", "/api/v1/receipts?page=1&size=20")
check(f"GET /receipts list status ({s1} R==P)", s1, s2)
check("GET /receipts list pages/pagination", {"total": l_py.get("total"), "page": l_py.get("page"),
      "page_size": l_py.get("page_size"), "pages": l_py.get("pages")},
      {"total": l_rs.get("total"), "page": l_rs.get("page"),
       "page_size": l_rs.get("page_size"), "pages": l_rs.get("pages")})
# Порівняти структури всіх чеків у списку (drop генераційних)
check("GET /receipts list items parity", l_py.get("items", []), l_rs.get("items", []), DROP)

# ── GET list з фільтром receipt_type=return ──────────────────────────────────
s1, l_py = req(PY, "GET", "/api/v1/receipts?receipt_type=return&size=20")
s2, l_rs = req(RS, "GET", "/api/v1/receipts?receipt_type=return&size=20")
check("list фільтр receipt_type=return total", l_py.get("total"), l_rs.get("total"))
check("list фільтр receipt_type=return parity", l_py.get("items", []), l_rs.get("items", []), DROP)

# ── GET search ────────────────────────────────────────────────────────────────
s1, q_py = req(PY, "GET", "/api/v1/receipts/search?q=ALIAS-DIFF&size=20")
s2, q_rs = req(RS, "GET", "/api/v1/receipts/search?q=ALIAS-DIFF&size=20")
check(f"GET /receipts/search status ({s1} R==P)", s1, s2)
check("GET /receipts/search total", q_py.get("total"), q_rs.get("total"))
check("GET /receipts/search items parity", q_py.get("items", []), q_rs.get("items", []),
      {"id", "created_at", "total_amount", "receipt_number"})

# ── GET stats/today ───────────────────────────────────────────────────────────
s1, st_py = req(PY, "GET", "/api/v1/receipts/stats/today")
s2, st_rs = req(RS, "GET", "/api/v1/receipts/stats/today")
check(f"GET /receipts/stats/today status ({s1} R==P)", s1, s2)
check("GET /receipts/stats/today parity", st_py, st_rs)

# ── GET returnable-quantity ───────────────────────────────────────────────────
s1, rq_py = req(PY, "GET", f"/api/v1/receipts/products/{pid}/returnable-quantity")
s2, rq_rs = req(RS, "GET", f"/api/v1/receipts/products/{pid}/returnable-quantity")
check(f"GET returnable-quantity status ({s1} R==P)", s1, s2)
check("GET returnable-quantity parity", rq_py, rq_rs, {"product_id"})

# ── GET by-product recent-sales ───────────────────────────────────────────────
s1, rs_py = req(PY, "GET", f"/api/v1/receipts/by-product/{barcode}/recent-sales?limit=5")
s2, rs_rs = req(RS, "GET", f"/api/v1/receipts/by-product/{barcode}/recent-sales?limit=5")
check(f"GET recent-sales status ({s1} R==P)", s1, s2)
check("GET recent-sales total", rs_py.get("total"), rs_rs.get("total"))
check("GET recent-sales items parity", rs_py.get("items", []), rs_rs.get("items", []),
      {"id", "receipt_id", "created_at", "receipt_number"})

# ── products barcodes ─────────────────────────────────────────────────────────
b_py_body = {"barcode": f"BAR-{uuid.uuid4().hex[:10]}", "is_primary": False}
b_rs_body = {"barcode": f"BAR-{uuid.uuid4().hex[:10]}", "is_primary": False}
s1, b_py = req(PY, "POST", f"/api/v1/products/{pid}/barcodes", b_py_body)
s2, b_rs = req(RS, "POST", f"/api/v1/products/{pid}/barcodes", b_rs_body)
check(f"POST products/{{id}}/barcodes status ({s1} R==P)", s1, s2)
check("POST products/{id}/barcodes parity", b_py, b_rs, {"id", "product_id", "created_at", "barcode"})
bid_py = (b_py or {}).get("id")
bid_rs = (b_rs or {}).get("id")
if bid_py and bid_rs:
    s1, _ = req(PY, "DELETE", f"/api/v1/products/{pid}/barcodes/{bid_py}")
    s2, _ = req(RS, "DELETE", f"/api/v1/products/{pid}/barcodes/{bid_rs}")
    check(f"DELETE products/{{id}}/barcodes status ({s1} R==P)", s1, s2)

# ── prro аліаси без /fiscal ───────────────────────────────────────────────────
for path in ["/api/v2/prro/status", "/api/v2/prro/queue?page=1&size=5"]:
    s1, p_py = req(PY, "GET", path)
    s2, p_rs = req(RS, "GET", path)
    check(f"GET {path} status ({s1} R==P)", s1, s2)
    check(f"GET {path} parity", p_py, p_rs)
s1, _ = req(PY, "POST", "/api/v2/prro/sync?limit=5")
s2, _ = req(RS, "POST", "/api/v2/prro/sync?limit=5")
check(f"POST /api/v2/prro/sync status ({s1} R==P)", s1, s2)

# ── Cleanup ───────────────────────────────────────────────────────────────────
for rid_ in (rid, r2.get("id")):
    if rid_:
        req(PY, "DELETE", f"/api/v1/receipts/{rid_}")
req(PY, "DELETE", f"/api/v1/products/{pid}")
print(f"\nRESULT: {'ALL PASS' if FAIL == 0 else f'FAIL ({FAIL})'}  ({PASS} passed, {FAIL} failed)")
