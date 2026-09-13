#!/usr/bin/env python3
"""Скільки reset-ів насправді породжує запит? Фонове тло vs запит.

reset-текст унікальний (NULL-літерали) → рахуємо його по всіх з'єднаннях каси.
Фаза A: 6 с без моїх запитів (фонове тло застосунку).
Фаза B: 6 с з 5 моїми запитами products.
Різниця / 5 = кількість reset-ів на запит.
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

hits = {"reset": set(), "ctxopen": set(), "page": set(), "other_open": set()}
stop = threading.Event()


def smp():
    cc = mon.cursor()
    while not stop.is_set():
        try:
            cc.execute("select pid, query_start, regexp_replace(query,'\\s+',' ','g') "
                       "from pg_stat_activity where client_addr=%s and pid <> pg_backend_pid()", (OUR_IP,))
            for pid, sc, q in cc.fetchall():
                if sc is None:
                    continue
                if "NULL, false" in q:
                    hits["reset"].add((pid, sc))
                elif "EXISTS(SELECT 1 FROM user_stores" in q:
                    hits["ctxopen"].add((pid, sc))
                elif "count(*) OVER ()" in q or "(SELECT count(*) FROM products p" in q:
                    hits["page"].add((pid, sc))
                elif "set_config('app.user_id', $1, false)" in q:
                    hits["other_open"].add((pid, sc))
        except Exception as e:
            print("sampler:", e)
            return
        time.sleep(0.003)


th = threading.Thread(target=smp, daemon=True)
th.start()
time.sleep(0.5)

base = {k: len(v) for k, v in hits.items()}
t_a0 = time.perf_counter()
time.sleep(6)
a = {k: len(v) - base[k] for k, v in hits.items()}
t_a = time.perf_counter() - t_a0

base2 = {k: len(v) for k, v in hits.items()}
N = 5
lat = []
for _ in range(N):
    req = urllib.request.Request(
        "http://127.0.0.1:8000/api/v1/products?size=20",
        headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=60) as r:
        r.read()
    lat.append((time.perf_counter() - t0) * 1000)
    time.sleep(0.05)
time.sleep(6)
b = {k: len(v) - base2[k] for k, v in hits.items()}

print(f"фаза A (без моїх запитів, {t_a:.1f} с): reset={a['reset']} ctx-open(RLS)={a['ctxopen']} "
      f"page={a['page']} інший-open={a['other_open']}")
print(f"фаза B ({N} запитів products за {t_a:.1f} с): reset={b['reset']} ctx-open(RLS)={b['ctxopen']} "
      f"page={b['page']} інший-open={b['other_open']}")
print(f"медіана HTTP: {sorted(lat)[len(lat)//2]:.0f} мс")
print(f"reset-ів ПОНАД фонове тло: {b['reset'] - a['reset']} на {N} запитів "
      f"→ {(b['reset'] - a['reset']) / N:.2f} на запит (ціль: 1)")
