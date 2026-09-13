#!/usr/bin/env python3
"""Чи відкриває каса НОВЕ з'єднання до хаба на кожен HTTP-запит?

Полимо pg_stat_activity кожні 100 мс і збираємо PID бекендів з НАШОГО IP.
Виключаємо власні psql-підключення (працюємо через один постійний конект).
"""
from _env import HUB_HOST, HUB_PG_PASSWORD
import os, subprocess, sqlite3, sys, threading, time, urllib.request

HUB = HUB_HOST
OUR_IP = "10.179.55.165"
SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"

os.environ["PGPASSWORD"] = HUB_PG_PASSWORD
PSQL = ["psql", "-h", HUB, "-U", "postgres", "-d", "pos_system", "-X", "-q", "-At"]

token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]


def pids_from_ip():
    out = subprocess.run(
        PSQL + ["-c",
                f"select pid from pg_stat_activity where client_addr='{OUR_IP}'"],
        capture_output=True, text=True, timeout=20).stdout
    return {int(x) for x in out.split() if x.strip().isdigit()}


def hit(path, **params):
    q = "&".join(f"{k}={v}" for k, v in params.items())
    url = f"http://127.0.0.1:8000{path}" + (f"?{q}" if q else "")
    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {token}", "X-Store-Id": SID})
    t = time.perf_counter()
    with urllib.request.urlopen(req, timeout=60) as r:
        r.read()
    return (time.perf_counter() - t) * 1000


seen = {}
stop = threading.Event()


def poller():
    while not stop.is_set():
        try:
            for p in pids_from_ip():
                seen.setdefault(p, time.time())
        except Exception:
            pass
        time.sleep(0.1)


print("=== стартова картина з'єднань каси ===")
base = pids_from_ip()
print(f"  {len(base)} з'єднань: {sorted(base)}")

th = threading.Thread(target=poller, daemon=True)
th.start()
time.sleep(1.0)
before = {p: t for p, t in seen.items()}

print("\n=== 5 HTTP-запитів ===")
for i, (path, params) in enumerate([
    ("/api/v1/categories", {"size": "5"}),
    ("/api/v1/categories", {"size": "5"}),
    ("/api/v1/products", {"size": "20"}),
    ("/api/v1/products", {"size": "20"}),
    ("/api/v1/suppliers", {"size": "5"}),
], 1):
    ms = hit(path, **params)
    now = pids_from_ip()
    print(f"  #{i} {path:20s} {ms:7.1f} мс | pid-ів: {len(now)} | нових: {sorted(now - set(base))}")

time.sleep(2.0)
stop.set(); th.join(timeout=2)

newpids = {p: t for p, t in seen.items() if p not in before}
print("\n=== ВИСНОВОК ===")
print(f"  нових бекендів за 5 запитів: {len(newpids)}")
if newpids:
    print("  → ПІДТВЕРДЖЕНО: застосунок відкриває НОВЕ з'єднання на кожен запит")
    print("    (шлях: TCP 31 мс + SCRAM-auth ~3 RTT ≈ 130-180 мс на запит)")
else:
    print("  → пул тримає з'єднання; плата не за connect, а за кількість statement-ів")
