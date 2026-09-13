#!/usr/bin/env python3
"""Верифікація критеріїв 3-4: коректність total і RLS."""
import sqlite3, urllib.parse, urllib.request

import psycopg2

SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]


def api(path, **params):
    q = urllib.parse.urlencode(params)
    req = urllib.request.Request(
        f"http://127.0.0.1:8000{path}" + (f"?{q}" if q else ""),
        headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
    import json
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


hub = psycopg2.connect(host=HUB_HOST, user="postgres",
                       password=HUB_PG_PASSWORD, dbname="pos_system")
hub.autocommit = True
hc = hub.cursor()


def db_count(where="", args=()):
    hc.execute(f"select count(*) from products {where}", args)
    return hc.fetchone()[0]


print("=== КРИТЕРІЙ 3: коректність поля total ===")
checks = [
    ("сторінка 1, без фільтра", {"size": "20", "page": "1"}, "", ()),
    ("сторінка 5, без фільтра", {"size": "20", "page": "5"}, "", ()),
    ("пошук 'молоко'", {"size": "20", "query": "молоко"}, "where title ilike %s", ("%молоко%",)),
    ("пошук 'ов'", {"size": "20", "query": "ов"}, "where title ilike %s", ("%ов%",)),
]
bad = 0
for label, params, where, args in checks:
    d = api("/api/v1/products", **params)
    exp = db_count(where, args)
    got = d["total"]
    ok = "OK" if got == exp else "РОЗБІЖНІСТЬ"
    if got != exp:
        bad += 1
    print(f"  {label:26s} API total={got:5d}  БД count={exp:5d}  {ok}  (рядків у видачі: {len(d['items'])})")

print(f"  → {'total коректний' if bad == 0 else str(bad) + ' розбіжностей'}")

print("\n=== КРИТЕРІЙ 4: RLS — доступ до чужої точки ===")
import uuid
for name, sid in [("своя точка (Білий)", SID),
                  ("чужа/неіснуюча точка", str(uuid.uuid4()))]:
    req = urllib.request.Request(
        "http://127.0.0.1:8000/api/v1/products?size=1",
        headers={"Authorization": f"Bearer {token}", "X-Store-Id": sid})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            print(f"  {name:24s} → HTTP {r.status}")
    except urllib.error.HTTPError as e:
        print(f"  {name:24s} → HTTP {e.code} (очікувано 403)")

from _env import HUB_HOST, HUB_PG_PASSWORD