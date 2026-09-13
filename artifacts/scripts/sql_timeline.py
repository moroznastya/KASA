#!/usr/bin/env python3
"""Точний таймлайн SQL одного HTTP-запиту каси.

Семплимо pg_stat_activity кожні 8 мс ОДНИМ постійним з'єднанням
(psycopg2), тому вимір не створює артефактів. Показує: які statement-и
реально виконуються, у якому порядку, і де витрачається час.
"""
from _env import HUB_HOST, HUB_PG_PASSWORD
import os, sqlite3, threading, time, urllib.request

import psycopg2

OUR_IP = "10.179.55.165"
SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
SAMPLE_MS = 8

token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]

mon = psycopg2.connect(host=HUB_HOST, user="postgres",
                       password=HUB_PG_PASSWORD, dbname="pos_system")
mon.autocommit = True
my_pid = mon.get_backend_pid()
cur = mon.cursor()
cur.execute("select pid from pg_stat_activity where client_addr=%s", (OUR_IP,))
APP_PIDS = [r[0] for r in cur.fetchall() if r[0] != my_pid]
print(f"з'єднань застосунку: {len(APP_PIDS)} {APP_PIDS}")

events = []
stop = threading.Event()


def sampler():
    c = mon.cursor()
    while not stop.is_set():
        t = time.perf_counter()
        try:
            c.execute("""select pid, coalesce(state,'?'),
                                coalesce(regexp_replace(query,'\s+',' ','g'),''),
                                extract(epoch from (now()-query_start))*1000
                         from pg_stat_activity
                         where pid = any(%s)""", (APP_PIDS,))
            for pid, state, q, dur in c.fetchall():
                events.append((t, pid, state, q[:110], dur))
        except Exception:
            pass
        time.sleep(SAMPLE_MS / 1000.0)


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
    events.clear()
    stop.clear()
    th = threading.Thread(target=sampler, daemon=True)
    th.start()
    time.sleep(0.4)
    events.clear()
    t0 = time.perf_counter()
    ms = hit(path, **params)
    t1 = time.perf_counter()
    time.sleep(0.25)
    stop.set(); th.join(timeout=1)

    print(f"\n=== {label} → HTTP {ms:.0f} мс ===")
    # стискаємо в послідовність унікальних statement-ів (по pid+query)
    seq, prev = [], None
    for t, pid, state, q, dur in events:
        key = (pid, q)
        if key != prev:
            seq.append((t - t0, pid, state, q))
            prev = key
    t_prev = 0.0
    for dt, pid, state, q in seq:
        gap = dt - t_prev
        mark = "  " if gap < 0.03 else "!!"
        print(f"  {dt*1000:7.1f} мс (Δ{gap*1000:6.1f}) {mark} pid={pid} {state:8s} {q}")
        t_prev = dt
    total_sql = sum(1 for _ in seq)
    print(f"  → зафіксовано переходів стану: {total_sql}")
    return ms


run("CATEGORIES (size=5)", "/api/v1/categories", size="5")
run("PRODUCTS (size=20)", "/api/v1/products", size="20")
run("PRODUCTS пошук 'молоко'", "/api/v1/products", size="20", query="молоко")
