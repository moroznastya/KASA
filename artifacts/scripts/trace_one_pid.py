#!/usr/bin/env python3
"""Повний слід ОДНОГО pid у вікні [запит..+3с]: чи є після 2-арг set_config
подальші запити (тобто це ctx-open іншого запиту, а не дубль скидання)."""
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
print("з'єднань каси під наглядом:", len(pids))

rows_seen = []
stop = threading.Event()


def smp():
    cc = mon.cursor()
    while not stop.is_set():
        t = time.perf_counter()
        try:
            cc.execute("select pid, state_change, state, regexp_replace(query,'\\s+',' ','g') "
                       "from pg_stat_activity where client_addr=%s", (OUR_IP,))
            rows_seen.append((t, cc.fetchall()))
        except Exception as e:
            print("sampler err:", e)
            return
        time.sleep(0.004)


th = threading.Thread(target=smp, daemon=True)
th.start()
time.sleep(0.8)
t0 = time.perf_counter()
req = urllib.request.Request(
    "http://127.0.0.1:8000/api/v1/products?size=20",
    headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
with urllib.request.urlopen(req, timeout=60) as r:
    r.read()
ms = (time.perf_counter() - t0) * 1000
time.sleep(3.5)
stop.set()
th.join(timeout=1)

# pid мого запиту = той, що виконав ctx-open з EXISTS
seq = {}
for t, rows in rows_seen:
    for pid, sc, st, q in rows:
        if sc is None:
            continue
        k = (pid, q, sc)
        if k not in seq:
            seq[k] = (t - t0, q, pid)
mine = next((pid for (pid, q, sc) in seq if "EXISTS(SELECT 1 FROM user_stores" in q), None)
items = sorted((dt, q) for (pid, q, sc), (dt2, q2, pid2) in
               ((k, v) for k, v in seq.items()) if pid == mine)
print(f"HTTP {ms:.0f} мс | pid запиту={mine} | подій на ньому за 3.5 с: {len(items)}")
for i, (dt, q) in enumerate(items, 1):
    kind = ("CTX-OPEN+RLS(3-арг)" if "EXISTS(SELECT 1 FROM user_stores" in q else
            "PAGE+WINDOW" if "count(*) OVER ()" in q else
            "SELECT..FROM products p" if "FROM products p" in q else
            "IMAGES" if "product_images" in q else
            "BARCODES" if "FROM barcodes" in q else
            "set_config(2-арг)" if "set_config('app.user_id', $1, false)" in q else
            "інше")
    print(f"  {i}. {dt*1000:7.1f} мс  {kind:22s} {q[:78]}")
