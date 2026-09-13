#!/usr/bin/env python3
"""Критерій 2: мережевих подій на запит products ≤ 5.

Скидання контексту тепер має ВІДМІННИЙ текст (NULL-літерали), тож у
pg_stat_activity видно окремо: ctx-open (3-арг+EXISTS), сторінка, зв'язки,
reset (NULL-літерали). Рахуємо події ЛИШЕ на з'єднанні запиту.
"""
from _env import HUB_HOST, HUB_PG_PASSWORD
import sqlite3
import threading
import time
import urllib.request

import psycopg2

OUR_IP = "10.179.55.165"
SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]

mon = psycopg2.connect(host=HUB_HOST, user="postgres", password=HUB_PG_PASSWORD,
                       dbname="pos_system")
mon.autocommit = True
c = mon.cursor()
c.execute("select pid from pg_stat_activity where client_addr=%s and pid <> pg_backend_pid()", (OUR_IP,))
pids = [r[0] for r in c.fetchall() if r[0] != mon.get_backend_pid()]

events = []
stop = threading.Event()


def smp():
    cc = mon.cursor()
    while not stop.is_set():
        try:
            cc.execute("select pid, state, query_start, regexp_replace(query,'\\s+',' ','g') "
                       "from pg_stat_activity where client_addr=%s and pid <> pg_backend_pid()", (OUR_IP,))
            events.append((time.perf_counter(), cc.fetchall()))
        except Exception as e:
            print("sampler:", e)
            return
        time.sleep(0.003)


th = threading.Thread(target=smp, daemon=True)
th.start()
time.sleep(0.8)
marker = time.time()
events.clear()

req = urllib.request.Request(
    "http://127.0.0.1:8000/api/v1/products?size=20",
    headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
t0 = time.perf_counter()
with urllib.request.urlopen(req, timeout=60) as r:
    r.read()
ms = (time.perf_counter() - t0) * 1000
time.sleep(1.2)
stop.set()
th.join(timeout=1)


def kind_of(q):
    if "EXISTS(SELECT 1 FROM user_stores" in q:
        return "CTX-OPEN+RLS"
    if "NULL, false" in q:
        return "RESET(idle)"
    if "count(*) OVER ()" in q or "(SELECT count(*) FROM products p" in q:
        return "PAGE+total"
    if "FROM product_images" in q:
        return "RELATIONS(images+barcodes)"
    if "FROM products p" in q:
        return "інший запит каталогу"
    return None


uniq = {}
for t, rows in events:
    for pid, st, sc, q in rows:
        if sc is None or sc.timestamp() < marker - 0.002:
            continue
        k = (pid, q, sc)
        if k not in uniq:
            uniq[k] = (t - t0, q, pid, st)

mine = next((pid for (pid, q, sc), _ in uniq.items()
             if "EXISTS(SELECT 1 FROM user_stores" in q), None)
items = sorted((dt, q, st) for (pid, q, sc), (dt, q2, pid2, st) in uniq.items()
               if pid == mine)

print(f"HTTP {ms:.0f} мс | pid запиту={mine}")
count = 0
for dt, q, st in items:
    k = kind_of(q)
    if k is None:
        print(f"   {dt*1000:7.1f} мс  [стан {st:6s}] (чужий запит, не рахуємо)")
        continue
    count += 1
    print(f"   {dt*1000:7.1f} мс  [{st:6s}] {k}")
print(f"\n  подій на запит: {count}  (ціль ≤ 5)  → {'PASS' if count <= 5 else 'FAIL'}")
