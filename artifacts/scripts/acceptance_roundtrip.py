#!/usr/bin/env python3
"""Приймальний замір round-trips після фіксу (з URL-кодуванням параметрів)."""
import sqlite3, time, urllib.parse, urllib.request, statistics

SID = "65d5db51-672f-4a38-9c1e-f36c5feb5374"
token = sqlite3.connect(
    "file:/home/anastasia/.local/share/torgashka/offline.db?mode=ro", uri=True
).execute("select value from settings where key='api_token'").fetchone()[0]


def hit(path, n=7, **params):
    q = urllib.parse.urlencode({k: v for k, v in params.items()})
    url = f"http://127.0.0.1:8000{path}" + (f"?{q}" if q else "")
    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {token}", "X-Store-Id": SID})
    out = []
    for _ in range(n):
        t = time.perf_counter()
        with urllib.request.urlopen(req, timeout=60) as r:
            r.read()
        out.append((time.perf_counter() - t) * 1000)
    return statistics.median(out), min(out)


CASES = [
    ("GET /api/v1/categories?size=5",       "/api/v1/categories", {"size": "5"},   100),
    ("GET /api/v1/products?size=20",        "/api/v1/products",   {"size": "20"},  150),
    ("GET /api/v1/products?query=молоко",   "/api/v1/products",   {"size": "20", "query": "молоко"}, 150),
    ("GET /api/v1/suppliers?size=5",        "/api/v1/suppliers",  {"size": "5"},   100),
    ("GET /api/v1/users",                   "/api/v1/users",      {},              150),
    ("GET /api/v1/stores",                  "/api/v1/stores",     {},              100),
]

BEFORE = {
    "GET /api/v1/categories?size=5": 249,
    "GET /api/v1/products?size=20": 552,
    "GET /api/v1/products?query=молоко": 534,
    "GET /api/v1/suppliers?size=5": 257,
    "GET /api/v1/users": 284,
    "GET /api/v1/stores": 117,
}

print("=== ПРИЙМАЛЬНИЙ ТЕСТ (медіана з 7) ===")
print("  %-36s %8s %8s %8s  %s" % ("ендпоінт", "було", "стало", "ціль", "вердикт"))
fails = 0
for label, path, params, target in CASES:
    med, mn = hit(path, **params)
    ok = "PASS" if med <= target else "FAIL"
    if med > target:
        fails += 1
    print("  %-36s %6d мс %6.0f мс %6d мс  %s" % (label, BEFORE[label], med, target, ok))

print(f"\n  результат: {'УСІ PASS' if fails == 0 else str(fails) + ' FAIL'}")
