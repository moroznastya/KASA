#!/usr/bin/env python3
"""v2: показує ЛИШЕ нову SQL-активність за час запиту (фільтр state_change).

Для кожного бекенда беремо state_change; усе, що старше старту запиту —
це «останній відомий запит», ігноруємо.
"""
from _env import HUB_HOST, HUB_PG_PASSWORD
import os, sqlite3, threading, time, urllib.request
import psycopg2

OUR_IP = "10.179.55.165"
SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
SAMPLE_MS = 6

token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]

mon = psycopg2.connect(host=HUB_HOST, user="postgres",
                       password=HUB_PG_PASSWORD, dbname="pos_system")
mon.autocommit = True
my_pid = mon.get_backend_pid()
c0 = mon.cursor()
c0.execute("select pid from pg_stat_activity where client_addr=%s", (OUR_IP,))
APP_PIDS = [r[0] for r in c0.fetchall() if r[0] != my_pid]


def hit(path, **params):
    q = "&".join(f"{k}={v}" for k, v in params.items())
    req = urllib.request.Request(
        f"http://127.0.0.1:8000{path}" + (f"?{q}" if q else ""),
        headers={"Authorization": f"Bearer {token}", "X-Store-Id": SID})
    t = time.perf_counter()
    with urllib.request.urlopen(req, timeout=60) as r:
        r.read()
    return (time.perf_counter() - t) * 1000


def run(label, path, **params):
    # «водяний знак»: усе, що старше за цей момент — стара активність
    wm = None
    samples = []
    stop = threading.Event()

    def sampler():
        c = mon.cursor()
        while not stop.is_set():
            t = time.perf_counter()
            try:
                c.execute("""select pid, coalesce(state,'?'), state_change,
                                    coalesce(regexp_replace(query,'\\s+',' ','g'),''),
                                    coalesce(extract(epoch from (xact_start))*1000,0)
                             from pg_stat_activity where pid = any(%s)""", (APP_PIDS,))
                samples.append((t, c.fetchall()))
            except Exception:
                pass
            time.sleep(SAMPLE_MS / 1000.0)

    th = threading.Thread(target=sampler, daemon=True)
    th.start()
    time.sleep(0.5)
    marker = time.time()          # межа «нового»
    samples.clear()
    t0 = time.perf_counter()
    ms = hit(path, **params)
    time.sleep(0.4)
    stop.set(); th.join(timeout=1)

    seen = {}
    timeline = []
    for t, rows in samples:
        for pid, state, sc, q, xs in rows:
            if sc and sc.timestamp() >= marker - 0.002:
                key = (pid, q, sc)
                if key not in seen:
                    seen[key] = t
                    timeline.append((t - t0, pid, state, q))
    timeline.sort()
    print(f"\n=== {label} → HTTP {ms:.0f} мс | нових SQL-подій: {len(timeline)} ===")
    prev = 0.0
    for dt, pid, state, q in timeline:
        gap = dt - prev
        flag = "  " if gap < 0.025 else "!!"
        print(f"  {dt*1000:7.1f} мс (Δ{gap*1000:6.1f}) {flag} pid={pid} {q}")
        prev = dt
    print(f"  межа: остання SQL-подія на {prev*1000:.0f} мс, HTTP завершився на {ms:.0f} мс")
    print(f"  → app-side (не мережа): {ms - prev*1000:.0f} мс")
    return ms


run("CATEGORIES", "/api/v1/categories", size="5")
run("PRODUCTS", "/api/v1/products", size="20")
run("PRODUCTS пошук", "/api/v1/products", size="20", query="молоко")
