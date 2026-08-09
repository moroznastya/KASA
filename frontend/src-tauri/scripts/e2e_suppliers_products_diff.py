#!/usr/bin/env python3
"""E2E differential suppliers products/movements: Rust (:8002) vs Python (:8001).

Сценарії:
  1. 404 — неіснуючий постачальник (products), неіснуючий товар/постачальник (movements)
  2. Пустий постачальник (без товарів) — parity відповіді
  3. Постачальник з товарами (direct supplier_id + з документів) — parity products,
     search parity, total_stock_value parity
  4. Рухи по 5 типах документів (invoice/return_invoice/receipt/write_off/transfer) — parity
  5. limit=0 / limit=501 → 422 (R==P)
Cleanup: тестові записи видаляються напряму з БД (psycopg2) у кінці.
Потрібно: Python :8001, фасад :8002 (KASA_RUST_READDIRS=1), /tmp/kasa_token.
"""
import json
import sys
import urllib.request
import urllib.error
import uuid
import datetime
import psycopg2

RUST = "http://127.0.0.1:8002"
PY = "http://127.0.0.1:8001"
TOKEN = open("/tmp/kasa_token").read().strip()
AUTH = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}
DB = dict(dbname="pos_system", user="postgres", password="VgxWd7MBJ10X", host="localhost", port=5432)

TS = int(datetime.datetime.now().timestamp())
FAIL = 0
created = {"suppliers": [], "products": [], "docs": []}


def req(base, method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(base + path, data=data, method=method, headers=AUTH)
    try:
        with urllib.request.urlopen(r) as resp:
            raw = resp.read()
            return resp.status, json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except Exception:
            return e.code, raw.decode(errors="replace")


def check(name, exp, got):
    global FAIL
    if exp == got:
        print(f"  OK {name}")
    else:
        print(f"  FAIL {name}")
        print(f"    exp: {json.dumps(exp, ensure_ascii=False)[:2000]}")
        print(f"    got: {json.dumps(got, ensure_ascii=False)[:2000]}")
        FAIL = 1


def get(base, path):
    return req(base, "GET", path)


print("=== SUPPLIERS PRODUCTS/MOVEMENTS DIFF (ts=%d) ===" % TS)

# ── 0. 404-сценарії (спершу, без даних) ────────────────────────────────────
z = str(uuid.uuid4())
s1_r, b1_r = get(RUST, f"/api/v1/suppliers/{z}/products")
s1_p, b1_p = get(PY, f"/api/v1/suppliers/{z}/products")
check("404 products (неіснуючий постачальник)", (s1_p, b1_p), (s1_r, b1_r))
s2_r, b2_r = get(RUST, f"/api/v1/suppliers/{z}/products/{z}/movements")
s2_p, b2_p = get(PY, f"/api/v1/suppliers/{z}/products/{z}/movements")
check("404 movements (неіснуючий постачальник)", (s2_p, b2_p), (s2_r, b2_r))

# ── 1. Пустий постачальник ─────────────────────────────────────────────────
st, sb = req(PY, "POST", "/api/v1/suppliers", {"name": f"Diff Sup Empty {TS}"})
S_EMPTY = sb["id"]
created["suppliers"].append(S_EMPTY)
r3_p, b3_p = get(PY, f"/api/v1/suppliers/{S_EMPTY}/products")
r3_r, b3_r = get(RUST, f"/api/v1/suppliers/{S_EMPTY}/products")
check("products пустого постачальника (parity)", (r3_p, b3_p), (r3_r, b3_r))

# ── 2. Постачальник з товарами ─────────────────────────────────────────────
st, sb = req(PY, "POST", "/api/v1/suppliers", {"name": f"Diff Sup Main {TS}"})
S = sb["id"]
created["suppliers"].append(S)

def mk_product(title, supplier_id=None, price="100.00", cost="50.00", stock="10.000"):
    body = {"title": title, "price": price, "cost_price": cost, "stock": stock, "unit": "шт"}
    if supplier_id:
        body["supplier_id"] = supplier_id
    st, pb = req(PY, "POST", "/api/v1/products", body)
    assert st == 201, (st, pb)
    created["products"].append(pb["id"])
    return pb["id"]

P1 = mk_product(f"Diff Prod A {TS}", S)      # direct supplier_id
P2 = mk_product(f"Diff Prod B {TS}")         # тільки через документи (немає supplier_id)
P3 = mk_product(f"Diff Prod C {TS}", S, price="200.00", cost="80.00", stock="5.000")

# ── 3. Документи (всі 5 типів руху для P1) ─────────────────────────────────
# invoice (прихід): P1×10, P2×5
inv_date = "2026-01-05T10:00:00"
st, ib = req(PY, "POST", "/api/v1/invoices", {
    "number": f"DIFF-INV-{TS}", "supplier_id": S, "invoice_date": inv_date,
    "items": [
        {"product_id": P1, "quantity": "10.000", "price": "50.00", "total": "500.00"},
        {"product_id": P2, "quantity": "5.000", "price": "30.00", "total": "150.00"},
    ],
})
assert st == 201, (st, ib)
created["docs"].append(("invoices", ib["id"]))
st, _ = req(PY, "POST", f"/api/v1/invoices/{ib['id']}/confirm", {"status": "confirmed"})
assert st == 200, st

# return_invoice (витрата): P1×2
ret_date = "2026-01-10T10:00:00"
st, rb = req(PY, "POST", "/api/v1/return-invoices", {
    "number": f"DIFF-RET-{TS}", "supplier_id": S, "return_date": ret_date,
    "items": [{"product_id": P1, "quantity": "2.000", "price": "50.00", "total": "100.00"}],
})
assert st == 201, (st, rb)
created["docs"].append(("return_invoices", rb["id"]))
st, _ = req(PY, "POST", f"/api/v1/return-invoices/{rb['id']}/confirm", {"status": "confirmed"})
assert st == 200, st

# receipt (продаж): P1×1
st, cb = req(PY, "POST", "/api/v1/receipts", {
    "receipt_number": f"DIFF-RCP-{TS}", "total_amount": "100.00", "paid_amount": "100.00",
    "items": [{"product_id": P1, "quantity": "1.000", "price": "100.00", "total": "100.00"}],
})
assert st == 201, (st, cb)
created["docs"].append(("receipts", cb["id"]))

# write_off: P1×1 → confirm
st, wb = req(PY, "POST", "/api/v1/write-offs", {
    "number": f"DIFF-WO-{TS}", "reason": "expired", "write_off_date": "2026-01-15T10:00:00",
    "items": [{"product_id": P1, "quantity": "1.000", "price": "100.00"}],
})
assert st == 201, (st, wb)
created["docs"].append(("write_offs", wb["id"]))
st, _ = req(PY, "POST", f"/api/v1/write-offs/{wb['id']}/confirm", {"status": "confirmed"})
assert st == 200, st

# transfer: P1×1 → confirm
st, tb = req(PY, "POST", "/api/v1/transfers", {
    "number": f"DIFF-TR-{TS}", "from_location": "Осн. склад", "to_location": "Торг. зал",
    "transfer_date": "2026-01-20T10:00:00",
    "items": [{"product_id": P1, "quantity": "1.000", "price": "100.00"}],
})
assert st == 201, (st, tb)
created["docs"].append(("transfers", tb["id"]))
st, _ = req(PY, "POST", f"/api/v1/transfers/{tb['id']}/confirm", {"status": "confirmed"})
assert st == 200, st

# ── 4. Products parity ──────────────────────────────────────────────────────
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products")
check("products status (R==P)", s_p, s_r)
check("products повний JSON (parity)", b_p, b_r)

# search parity (total_stock_value рахується по відфільтрованих товарах)
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products?search=Diff%20Prod%20A")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products?search=Diff%20Prod%20A")
check("products search (parity)", (s_p, b_p), (s_r, b_r))

# ── 5. Movements parity (P1 — всі 5 типів) ─────────────────────────────────
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products/{P1}/movements")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products/{P1}/movements")
check("movements status (R==P)", s_p, s_r)
check("movements повний JSON (parity)", b_p, b_r)

# рухи P2 — тільки invoice (товар без supplier_id)
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products/{P2}/movements")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products/{P2}/movements")
check("movements P2 (тільки invoice, parity)", (s_p, b_p), (s_r, b_r))

# 404: товар не існує
z2 = str(uuid.uuid4())
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products/{z2}/movements")
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products/{z2}/movements")
check("404 movements (неіснуючий товар)", (s_p, b_p), (s_r, b_r))

# ── 6. limit валідації (422) ────────────────────────────────────────────────
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products/{P1}/movements?limit=0")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products/{P1}/movements?limit=0")
check("movements limit=0 (422 R==P)", (s_p, b_p), (s_r, b_r))
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products/{P1}/movements?limit=501")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products/{P1}/movements?limit=501")
check("movements limit=501 (422 R==P)", (s_p, b_p), (s_r, b_r))
# limit=2 → обрізання до 2 записів (найновіші)
s_p, b_p = get(PY, f"/api/v1/suppliers/{S}/products/{P1}/movements?limit=2")
s_r, b_r = get(RUST, f"/api/v1/suppliers/{S}/products/{P1}/movements?limit=2")
check("movements limit=2 (parity)", (s_p, b_p), (s_r, b_r))

# ── Cleanup ─────────────────────────────────────────────────────────────────
with psycopg2.connect(**DB) as conn:
    conn.autocommit = True
    with conn.cursor() as cur:
        # документи (позиції каскадно) — зворотний порядок
        for tbl, doc_id in reversed(created["docs"]):
            cur.execute(f"DELETE FROM {tbl} WHERE id = %s", (doc_id,))
        # товари
        for pid in created["products"]:
            cur.execute("DELETE FROM products WHERE id = %s", (pid,))
        # ledger (invoice confirm створює записи supplier_ledger)
        for sid in created["suppliers"]:
            cur.execute("DELETE FROM supplier_ledger WHERE supplier_id = %s", (sid,))
        # постачальники
        for sid in created["suppliers"]:
            cur.execute("DELETE FROM suppliers WHERE id = %s", (sid,))
print("  cleanup: тестові дані видалено")

print("")
if FAIL == 0:
    print("RESULT: ALL PASS")
else:
    print(f"RESULT: FAIL ({FAIL})")
    sys.exit(1)
