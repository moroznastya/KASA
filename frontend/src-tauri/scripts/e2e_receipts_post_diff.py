#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Differential-тест: POST /api/v1/receipts — Rust == Python (1:1).

Покриває (з docs/rust_deactivation_audit.md — останній CRIT):
  - звичайний SALE чек (повна оплата, 2 позиції)
  - борговий чек (debtor_id, paid < total → борг += різниця)
  - оплата боргу через касу (debt_payment):
      * повна → боржник автознищується (каскад видаляє debtor_payments)
      * часткова → борг зменшується, запис debtor_payments лишається
  - RETURN чек (original_receipt_id, returnable-валідація)
  - помилки: 404 боржника (debt_payment), 400 сума > борг, 404 товару,
    400 paid<0, 400 недостатньо залишку (allow_negative_stock=false)
  - 422 Pydantic: missing total_amount, enum receipt_type,
    decimal_max_places price
  - заокруглення price_rounding (поточне значення з БД — впливає на обидва)

Порядок у parity-перевірках: спершу Python (еталон, гарантує commit),
потім Rust — як у попередніх differential-тестах.
"""

import json
import sys
import time
import urllib.error
import urllib.request
from uuid import uuid4

PY = "http://127.0.0.1:8001"
RUST = "http://127.0.0.1:8002"

TOKEN = open("/tmp/kasa_token").read().strip()
AUTH = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}

# ID товару "Борг" (barcode: DEBT-PAYMENT) — константа Python v1.
DEBT_PRODUCT_ID = "c230fe32-78ef-4501-a21d-71467a668fc4"

PASS = 0
FAIL = 0
created = {"receipts": [], "products": [], "debtors": []}


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


def db(cur, sql, params=None):
    cur.execute(sql, params or ())
    if cur.description:
        return cur.fetchall()
    return []


def normalize(resp, drop_keys=None):
    """Видаляє генераційні поля (id, created_at) — залишає семантику 1:1."""
    drop = set(drop_keys or []) | {"id", "receipt_id", "created_at"}
    if isinstance(resp, dict):
        r = {}
        for k, v in resp.items():
            if k in drop:
                continue
            if k == "items" and isinstance(v, list):
                r[k] = [normalize(it, drop_keys) for it in v]
            else:
                r[k] = normalize(v, drop_keys)
        return r
    if isinstance(resp, list):
        return [normalize(x, drop_keys) for x in resp]
    return resp


def check(name, exp, got, exact=True):
    global PASS, FAIL
    ok = exp == got
    if ok:
        PASS += 1
        print(f"  OK {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}")
        print(f"    exp: {json.dumps(exp, ensure_ascii=False)[:1500]}")
        print(f"    got: {json.dumps(got, ensure_ascii=False)[:1500]}")


TS = int(time.time())

# ─── Підготовка: товари + боржник (через Python API) ───────────────────────
def mk_product(title, price, cost, stock, tax=0.00):
    st, b = req(PY, "POST", "/api/v1/products", {
        "title": title, "price": str(price), "cost_price": str(cost),
        "stock": str(stock), "unit": "шт", "tax_rate": str(tax),
    })
    assert st == 201, (st, b)
    created["products"].append(b["id"])
    return b["id"]


def mk_debtor(name):
    st, b = req(PY, "POST", "/api/v1/debtors", {"name": name})
    assert st == 201, (st, b)
    created["debtors"].append(b["id"])
    return b["id"]


print(f"=== RECEIPTS POST DIFF (ts={TS}) ===")

P1 = mk_product(f"DIFF-RCPT-A {TS}", 50.00, 40.00, 100)
P2 = mk_product(f"DIFF-RCPT-B {TS}", 30.00, 20.00, 100)
P0 = mk_product(f"DIFF-RCPT-ZERO {TS}", 10.00, 5.00, 0)     # нульовий залишок
PTAX = mk_product(f"DIFF-RCPT-TAX {TS}", 60.00, 30.00, 100, tax=20.00)
D = mk_debtor(f"DIFF-RCPT-DEBTOR {TS}")

# ─── 1. Звичайний SALE чек (повна оплата, 2 позиції, один товар) ────────────
body1 = {
    "receipt_number": f"DIFF-RCPT-{TS}-A",
    "receipt_type": "sale",
    "total_amount": "100.00",
    "paid_amount": "100.00",
    "payment_method": "cash",
    "items": [
        {"product_id": P1, "quantity": "1", "price": "50.00", "total": "50.00"},
        {"product_id": P1, "quantity": "1", "price": "50.00", "total": "50.00"},
    ],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body1)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body1)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
RCPT1_PY_ID = b_p.get("id")
check("sale повна оплата status (201 R==P)", (s_p, s_r), (201, 201))
check("sale повна оплата parity", normalize(b_p), normalize(b_r))
check("sale payment_method/type", (b_r.get("payment_method"), b_r.get("receipt_type")), ("cash", "sale"))

# ─── 2. Борговий чек (debtor_id, paid < total → борг += 60) ─────────────────
body2 = {
    "receipt_number": f"DIFF-RCPT-{TS}-B",
    "receipt_type": "sale",
    "total_amount": "100.00",
    "paid_amount": "40.00",
    "debtor_id": D,
    "payment_method": "cash",
    "items": [{"product_id": P2, "quantity": "1", "price": "100.00", "total": "100.00"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body2)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body2)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
check("борговий чек status (201 R==P)", (s_p, s_r), (201, 201))
check("борговий чек parity", normalize(b_p), normalize(b_r))
check("борговий чек debtor_id", (b_p.get("debtor_id"), b_r.get("debtor_id")), (D, D))

import psycopg2
DB = dict(dbname="pos_system", user="postgres", password="VgxWd7MBJ10X",
          host="localhost", port=5432)
with psycopg2.connect(**DB) as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT total_debt::text FROM debtors WHERE id = %s", (D,))
        debt_after2 = cur.fetchone()[0]
check("борг після 2 чеків = 120.00", debt_after2, "120.00")

# ─── 3. Оплата боргу (повна: 120.00 → боржник автознищується) ───────────────
# PY і RUST працюють зі СВОЇМИ боржниками (інакше перший знищить борг другого).
DP = mk_debtor(f"DIFF-RCPT-DEBTOR-PAY-PY {TS}")
DR = mk_debtor(f"DIFF-RCPT-DEBTOR-PAY-RS {TS}")
with psycopg2.connect(**DB) as conn:
    with conn.cursor() as cur:
        cur.execute("UPDATE debtors SET total_debt = 120.00 WHERE id IN (%s, %s)", (DP, DR))
body3 = {
    "receipt_number": f"DIFF-RCPT-{TS}-C",
    "receipt_type": "sale",
    "total_amount": "120.00",
    "paid_amount": "120.00",
    "payment_method": "cash",
    "debt_payment": {"debtor_id": DP, "amount": "120.00"},
    "items": [{"product_id": PTAX, "quantity": "1", "price": "60.00", "total": "60.00"}],
}
body3r = {**body3, "debt_payment": {"debtor_id": DR, "amount": "120.00"}}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body3)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body3r)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
check("оплата боргу status (201 R==P)", (s_p, s_r), (201, 201))
check("оплата боргу parity (включно з auto-add DEBT item)", normalize(b_p, ["debtor_id"]), normalize(b_r, ["debtor_id"]))
# Боржники мають бути знищені; debtor_payments — каскадно видалені.
with psycopg2.connect(**DB) as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM debtors WHERE id IN (%s, %s)", (DP, DR))
        debtors_left = cur.fetchone()[0]
        cur.execute("SELECT count(*) FROM debtor_payments WHERE debtor_id IN (%s, %s)", (DP, DR))
        payments_left = cur.fetchone()[0]
check("боржники знищені при боргу 0", debtors_left, 0)
check("debtor_payments каскадно видалені", payments_left, 0)

# ─── 4. Часткова оплата боргу (боржник живий, борг зменшується) ─────────────
D2P = mk_debtor(f"DIFF-RCPT-DEBTOR2-PY {TS}")
D2R = mk_debtor(f"DIFF-RCPT-DEBTOR2-RS {TS}")
with psycopg2.connect(**DB) as conn:
    with conn.cursor() as cur:
        cur.execute("UPDATE debtors SET total_debt = 100.00 WHERE id IN (%s, %s)", (D2P, D2R))
body4 = {
    "receipt_number": f"DIFF-RCPT-{TS}-D",
    "receipt_type": "sale",
    "total_amount": "40.00",
    "paid_amount": "40.00",
    "payment_method": "cash",
    "debt_payment": {"debtor_id": D2P, "amount": "40.00"},
    "items": [],
}
body4r = {**body4, "debt_payment": {"debtor_id": D2R, "amount": "40.00"}}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body4)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body4r)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
check("часткова оплата боргу status (201 R==P)", (s_p, s_r), (201, 201))
check("часткова оплата боргу parity", normalize(b_p, ["debtor_id"]), normalize(b_r, ["debtor_id"]))
with psycopg2.connect(**DB) as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT total_debt::text FROM debtors WHERE id IN (%s, %s)", (D2P, D2R))
        debts = [r[0] for r in cur.fetchall()]
        cur.execute("SELECT count(*), COALESCE(sum(amount::numeric),0)::text FROM debtor_payments WHERE debtor_id IN (%s, %s)", (D2P, D2R))
        pay_cnt, pay_sum = cur.fetchone()
check("борг обох D2 = 60.00", sorted(debts), ["60.00", "60.00"])
check("debtor_payments D2: 2 записи по 40.00", (pay_cnt, pay_sum), (2, "80.00"))

# ─── 5. RETURN чек (original_receipt_id, returnable-валідація) ──────────────
# Продали P1: 2 чеків по 2 шт (кейс 1: PY+RUST) → stock 100-4=96.
# Повертаємо 1 шт через PY і 1 шт через RUST.
body5 = {
    "receipt_number": f"DIFF-RCPT-{TS}-R1",
    "receipt_type": "return",
    "total_amount": "50.00",
    "paid_amount": "50.00",
    "payment_method": "cash",
    "original_receipt_id": RCPT1_PY_ID,
    "items": [{"product_id": P1, "quantity": "1", "price": "50.00", "total": "50.00"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body5)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body5)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
check("return чек status (201 R==P)", (s_p, s_r), (201, 201))
check("return чек parity", normalize(b_p), normalize(b_r))
check("return receipt_type", b_r.get("receipt_type"), "return")

# ─── 6. RETURN з перевищенням returnable → 400 ───────────────────────────────
body6 = {
    "receipt_number": f"DIFF-RCPT-{TS}-R2",
    "receipt_type": "return",
    "total_amount": "250.00",
    "paid_amount": "250.00",
    "items": [{"product_id": P1, "quantity": "5", "price": "50.00", "total": "250.00"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body6)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body6)
check("return перевищення status (400 R==P)", (s_p, s_r), (400, 400))
check("return перевищення detail parity", b_p, b_r)

# ─── 7. Помилки: боржник не існує (debt_payment) → 404 ───────────────────────
body7 = {
    "receipt_number": f"DIFF-RCPT-{TS}-E1",
    "total_amount": "10.00",
    "debt_payment": {"debtor_id": str(uuid4()), "amount": "10.00"},
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body7)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body7)
check("debt_payment неіснуючий боржник (404 R==P)", (s_p, s_r), (404, 404))
check("debt_payment 404 detail parity", b_p, b_r)

# ─── 8. Сума оплати боргу > поточний борг → 400 ─────────────────────────────
D3 = mk_debtor(f"DIFF-RCPT-DEBTOR3 {TS}")
body8 = {
    "receipt_number": f"DIFF-RCPT-{TS}-E2",
    "total_amount": "10.00",
    "debt_payment": {"debtor_id": D3, "amount": "50.00"},
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body8)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body8)
check("сума > борг (400 R==P)", (s_p, s_r), (400, 400))
check("сума > борг detail parity", b_p, b_r)

# ─── 9. Неіснуючий товар у SALE → 404 ────────────────────────────────────────
body9 = {
    "receipt_number": f"DIFF-RCPT-{TS}-E3",
    "total_amount": "10.00",
    "items": [{"product_id": str(uuid4()), "quantity": "1", "price": "10.00"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body9)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body9)
check("неіснуючий товар (500 IntegrityError R==P)", (s_p, s_r), (500, 500))
check("неіснуючий товар detail parity", b_p, b_r)

# ─── 10. paid_amount < 0 → 400 ───────────────────────────────────────────────
body10 = {
    "receipt_number": f"DIFF-RCPT-{TS}-E4",
    "total_amount": "10.00",
    "paid_amount": "-5.00",
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body10)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body10)
check("paid<0 (400 R==P)", (s_p, s_r), (400, 400))
check("paid<0 detail parity", b_p, b_r)

# ─── 11. Недостатньо товару (allow_negative_stock=false) → 400 ──────────────
body11 = {
    "receipt_number": f"DIFF-RCPT-{TS}-E5",
    "total_amount": "10.00",
    "paid_amount": "10.00",
    "items": [{"product_id": P0, "quantity": "1", "price": "10.00"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body11)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body11)
check("недостатньо товару (400 R==P)", (s_p, s_r), (400, 400))
check("недостатньо товару detail parity", b_p, b_r)

# ─── 12. 422 Pydantic: missing total_amount ──────────────────────────────────
body12 = {"items": [{"product_id": P1, "quantity": "1", "price": "1.00"}]}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body12)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body12)
check("422 missing total_amount (R==P)", (s_p, s_r), (422, 422))
check("422 missing total_amount detail parity", b_p, b_r)

# ─── 13. 422 enum receipt_type ───────────────────────────────────────────────
body13 = {"receipt_type": "foo", "total_amount": "1.00"}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body13)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body13)
check("422 enum receipt_type (R==P)", (s_p, s_r), (422, 422))
check("422 enum receipt_type detail parity", b_p, b_r)

# ─── 14. 422 decimal_max_places price ────────────────────────────────────────
body14 = {
    "total_amount": "1.00",
    "items": [{"product_id": P1, "quantity": "1", "price": "47.333"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body14)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body14)
check("422 decimal_max_places (R==P)", (s_p, s_r), (422, 422))
check("422 decimal_max_places detail parity", b_p, b_r)

# ─── 15. Заокруглення price_rounding (поточне з БД; впливає на обидва) ──────
with psycopg2.connect(**DB) as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT value FROM system_settings WHERE key='price_rounding' AND is_active=true")
        row = cur.fetchone()
rounding = int(row[0]) if row else 1
body15 = {
    "receipt_number": f"DIFF-RCPT-{TS}-ROUND",
    "receipt_type": "sale",
    "total_amount": "47.33",
    "paid_amount": "47.33",
    "payment_method": "cash",
    "items": [{"product_id": P2, "quantity": "1", "price": "47.33", "total": "47.33"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body15)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body15)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
check(f"rounding code={rounding} status (201 R==P)", (s_p, s_r), (201, 201))
check(f"rounding code={rounding} parity", normalize(b_p), normalize(b_r))

# ─── 16. TAX: чек з товаром tax_rate=20% → vat_amount parity ────────────────
body16 = {
    "receipt_number": f"DIFF-RCPT-{TS}-TAX",
    "receipt_type": "sale",
    "total_amount": "60.00",
    "paid_amount": "60.00",
    "payment_method": "cash",
    "items": [{"product_id": PTAX, "quantity": "1", "price": "60.00", "total": "60.00"}],
}
s_p, b_p = req(PY, "POST", "/api/v1/receipts", body16)
s_r, b_r = req(RUST, "POST", "/api/v1/receipts", body16)
created["receipts"] += [b_p.get("id"), b_r.get("id")]
check("tax чек status (201 R==P)", (s_p, s_r), (201, 201))
check("tax чек parity (vat_amount)", normalize(b_p), normalize(b_r))
check("tax vat_amount = 10.0", b_r.get("vat_amount"), "10.0")

# ─── Cleanup ─────────────────────────────────────────────────────────────────
with psycopg2.connect(**DB) as conn:
    conn.autocommit = True
    with conn.cursor() as cur:
        # debtors (каскад видалить debtor_payments)
        for did in created["debtors"]:
            db(cur, "DELETE FROM debtors WHERE id = %s", (did,))
        # чеки (каскад видалить receipt_items)
        for rid in created["receipts"]:
            if rid:
                db(cur, "DELETE FROM receipts WHERE id = %s", (rid,))
        # товари
        for pid in created["products"]:
            db(cur, "DELETE FROM products WHERE id = %s", (pid,))
        # залишок: чеки/боржники DIFF-RCPT-* (аварійні 400/404 без id у списку)
        cur.execute("DELETE FROM receipts WHERE receipt_number LIKE 'DIFF-RCPT-%'")
        cur.execute("DELETE FROM debtors WHERE name LIKE 'DIFF-RCPT-DEBTOR%'")
        cur.execute("DELETE FROM products WHERE title LIKE 'DIFF-RCPT-%'")
print("  cleanup: тестові дані видалено")

print(f"\nRESULT: {'ALL PASS' if FAIL == 0 else f'FAIL ({FAIL})'}  ({PASS} passed, {FAIL} failed)")
sys.exit(0 if FAIL == 0 else 1)
