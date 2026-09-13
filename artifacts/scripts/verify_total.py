#!/usr/bin/env python3
"""Критерій 3: поле `total` у /api/v1/products мусить дорівнювати count(*) на хабі.

Перевіряємо сторінки 1 і 5 каталогу та пошук (обидві гілки: trgm ≥3 символи
та короткий патерн <3 символів).
"""
from _env import HUB_HOST, HUB_PG_PASSWORD
import json
import sqlite3
import subprocess
import urllib.parse
import urllib.request

SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
STORE_SCOPE = f"'{SID}'::uuid"
token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]


def hub_count(where=""):
    sql = ("select count(*) from products p left join stock st on st.product_id = p.id "
           f"and st.store_id = {STORE_SCOPE}")
    if where:
        sql += " where " + where
    out = subprocess.run(
        ["psql", "-h", HUB_HOST, "-U", "postgres", "-d", "pos_system", "-X", "-Atc", sql],
        capture_output=True, text=True, env={"PGPASSWORD": HUB_PG_PASSWORD, "PATH": "/usr/bin:/bin"},
    )
    return int(out.stdout.strip())


def api(path, **params):
    q = urllib.parse.urlencode(params)
    req = urllib.request.Request(
        f"http://127.0.0.1:8000{path}" + (f"?{q}" if q else ""),
        headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read())


def like_where(q):
    p = f"'%{q}%'"
    return (f"(p.title ilike {p} or p.barcode ilike {p} or p.sku ilike {p} "
            f"or exists (select 1 from barcodes b where b.product_id = p.id and b.barcode ilike {p}))")


CASES = [
    ("каталог, стор.1", {"page": 1, "size": 20}, ""),
    ("каталог, стор.5", {"page": 5, "size": 20}, ""),
    ("пошук 'молоко' (trgm)", {"page": 1, "size": 20, "query": "молоко"}, like_where("молоко")),
    ("пошук 'молоко', стор.3", {"page": 3, "size": 20, "query": "молоко"}, like_where("молоко")),
    ("пошук 'мо' (<3 симв.)", {"page": 1, "size": 20, "query": "мо"}, like_where("мо")),
    ("пошук 'ов' (<3 симв.)", {"page": 1, "size": 20, "query": "ов"}, like_where("ов")),
]

print("=== КРИТЕРІЙ 3: total у відповіді == count(*) на хабі ===")
bad = 0
for label, params, where in CASES:
    r = api("/api/v1/products", **params)
    api_total = r["total"]
    api_items = len(r["items"])
    hub = hub_count(where)
    ok = api_total == hub
    bad += 0 if ok else 1
    print(f"  {label:26s} total(API)={api_total:5d}  count(hub)={hub:5d}  "
          f"items={api_items:2d}  pages={r.get('pages')}  {'OK' if ok else 'РОЗБІЖНІСТЬ'}")
print(f"\n  вердикт: {'total коректний у всіх випадках' if bad == 0 else str(bad) + ' розбіжностей'}")
