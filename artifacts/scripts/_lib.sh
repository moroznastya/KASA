#!/usr/bin/env bash
# ── Спільні налаштування для діагностичних скриптів Torgashka ──
# Це бібліотека: підключається через `. "$(dirname "$0")/_lib.sh"`
# Реальні значення лежать у artifacts/.env.local (у .gitignore, у репо не потрапляє).
# Скопіюйте artifacts/.env.local.example → artifacts/.env.local і заповніть.
_lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_lib_env="$_lib_dir/../.env.local"
# shellcheck source=/dev/null
[ -f "$_lib_env" ] && . "$_lib_env"

export HUB_HOST="${HUB_HOST:-127.0.0.1}"
export HUB_PG_USER="${HUB_PG_USER:-postgres}"
export HUB_PG_DB="${HUB_PG_DB:-pos_system}"
export HUB_PG_PASSWORD="${HUB_PG_PASSWORD:?HUB_PG_PASSWORD не задано — див. artifacts/.env.local.example}"
export PGPASSWORD="$HUB_PG_PASSWORD"

# Зручний хелпер: psql до хаба
hub_psql() { psql -h "$HUB_HOST" -U "$HUB_PG_USER" -d "$HUB_PG_DB" -X -q "$@"; }
