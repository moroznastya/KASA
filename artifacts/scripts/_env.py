"""Спільний env-шар для діагностичних скриптів Torgashka.

Реальні значення — у artifacts/.env.local (у .gitignore).
Скопіюйте artifacts/.env.local.example → artifacts/.env.local і заповніть.
"""
import os
import pathlib

_ENV_FILE = pathlib.Path(__file__).resolve().parent.parent / ".env.local"
if _ENV_FILE.exists():
    for _line in _ENV_FILE.read_text().splitlines():
        _line = _line.strip()
        if _line and not _line.startswith("#") and "=" in _line:
            _k, _v = _line.split("=", 1)
            os.environ.setdefault(_k.strip(), _v.strip())

HUB_HOST = os.environ.get("HUB_HOST", "127.0.0.1")
HUB_PG_USER = os.environ.get("HUB_PG_USER", "postgres")
HUB_PG_DB = os.environ.get("HUB_PG_DB", "pos_system")
HUB_PG_PASSWORD = os.environ.get("HUB_PG_PASSWORD", "")

API_BASE = os.environ.get("TORGASHKA_API_BASE", "http://127.0.0.1:8000")
OFFLINE_DB = os.environ.get(
    "TORGASHKA_OFFLINE_DB",
    os.path.expanduser("~/.local/share/torgashka/offline.db"),
)


def hub_connect(**overrides):
    """psycopg2-з'єднання до хаба з параметрами з env."""
    import psycopg2

    kw = dict(host=HUB_HOST, user=HUB_PG_USER, password=HUB_PG_PASSWORD, dbname=HUB_PG_DB)
    kw.update(overrides)
    return psycopg2.connect(**kw)


def api_token():
    """Токен каси з локальної offline.db (не хардкодимо в репо)."""
    import sqlite3

    conn = sqlite3.connect(f"file:{OFFLINE_DB}?mode=ro", uri=True)
    try:
        return conn.execute(
            "select value from settings where key='api_token'"
        ).fetchone()[0]
    finally:
        conn.close()
