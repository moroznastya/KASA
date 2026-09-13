#!/usr/bin/env python3
"""Хто саме виконує другий set_config(2-арг) після запиту products?

Семплеримо pg_stat_activity на всіх з'єднаннях каси у вікні [запит + 3 с].
Якщо після другого 2-арг set_config на тому ж pid ідуть інші запити
(напр. sync: products WHERE server_version > $1) — це ctx-open ІНШОГО запиту,
а не друге скидання нашого.
"""
from _env import HUB_HOST, HUB_PG_PASSWORD
import sqlite3, threading, time, urllib.request, psycopg2

OUR_IP = "10.179.55.165"
SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]

mon = psycopg2.connect(host=HUB_HOST, user="postgres", password=HUB_PG_PASSWORD,
                       dbname="pos_system")
mon.autocommit = True
c = mon.cursor()
c.execute("select pid from pg_stat_activity where client_addr=%s", (OUR_IP,))
pids = [r[0] for r in c.fetchall() if r[0] != mon.get_backend_pid()]

samples = []
stop = threading.Event()


def smp():
    cc = mon.cursor()
    while not stop.is_set():
        t = time.perf_counter()
        try:
            cc.execute(
                "select pid, state_change, regexp_replace(query,'\\s+',' ','g') "
                "from pg_stat_activity where pid=any(%s)", (pids,))
            samples.append((t, cc.fetchall()))
        except Exception:
            pass
        time.sleep(0.004)


th = threading.Thread(target=smp, daemon=True)
th.start()
time.sleep(0.6)
marker = time.time()
samples.clear()

req = urllib.request.Request(
    "http://127.0.0.1:8000/api/v1/products?size=20",
    headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
t0 = time.perf_counter()
with urllib.request.urlopen(req, timeout=60) as r:
    r.read()
ms = (time.perf_counter() - t0) * 1000
time.sleep(3.0)                      # довге вікно: видно, що робить pid далі
stop.set()
th.join(timeout=1)

seen = {}
for t, rows in samples:
    for pid, sc, q in rows:
        if sc and sc.timestamp() >= marker - 0.002:
            k = (pid, q, sc)
            if k not in seen:
                seen[k] = (t - t0, q, pid)

mine = None
for (pid, q, sc), _ in seen.items():
    if "EXISTS(SELECT 1 FROM user_stores" in q:
        mine = pid
        break

items = sorted((dt, q) for (pid, q, sc), (dt, q2, pid2) in
               ((k, v) for k, v in seen.items()) if pid == mine)
print(f"HTTP {ms:.0f} мс | pid запиту={mine} | подій на ньому: {len(items)}")
for i, (dt, q) in enumerate(items, 1):
    kind = ("CTX-OPEN+RLS(3-арг)" if "EXISTS(SELECT 1 FROM user_stores" in q else
            "PAGE+WINDOW" if "count(*) OVER ()" in q else
            "PAGE(query)" if "FROM products p" in q else
            "IMAGES" if "FROM product_images" in q else
            "BARCODES" if "FROM barcodes" in q else
            "set_config(2-арг)" if "set_config('app.user_id', $1, false)" in q else "інше")
    print(f"  {i}. {dt*1000:7.1f} мс  {kind:22s} {q[:70]}")
