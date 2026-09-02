#!/usr/bin/env bash
# ============================================================================
# backup-restore.sh — ЕТАП 11.1: відновлення однієї БД з бекапу (pg_restore)
# ============================================================================
# Процедура (деструктивна!):
#   1) terminate всіх активних підключень до цільової БД;
#   2) DROP DATABASE IF EXISTS (з terminate connections);
#   3) CREATE DATABASE (чиста БД);
#   4) pg_restore --format=custom (бекап, створений scripts/backup.sh).
#
# Використання:
#   scripts/backup-restore.sh <DB_NAME> <BACKUP_FILE> [--yes]
#
#   <DB_NAME>      — ім'я БД, яку відновлюємо (напр. torgashka_owner_abc12345)
#   <BACKUP_FILE>  — .dump файл (custom format), напр. backups/torgashka_owner_abc12345_20260901_0200.dump
#   --yes          — підтвердження деструктивної операції (без інтерактивного питання)
#
# Приклад:
#   scripts/backup-restore.sh torgashka_owner_abc12345 \
#       backups/torgashka_owner_abc12345_20260901_0200.dump --yes
#
# Безпека:
#   - відновлення мета-БД pos_system БЕЗ --yes заборонено (втрата маршрутизації);
#   - перед DROP перевіряється, що файл є валідним pg_dump-архівом;
#   - підключення до сервера: PGHOST/PGPORT/PGUSER/PGPASSWORD або DATABASE_URL
#     (той самий формат, що в scripts/backup.sh);
#   - всі виклики з -w: ніколи не питають пароль інтерактивно.
# ============================================================================
set -euo pipefail

# --- Конфіг (env з дефолтами) ------------------------------------------------
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PGPASSWORD="${PGPASSWORD:-}"
DATABASE_URL="${DATABASE_URL:-}"
LOG_DIR="${LOG_DIR:-./logs}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$LOG_DIR" in
    /*) : ;;
    *)  LOG_DIR="$SCRIPT_DIR/../$LOG_DIR" ;;
esac

export PGPASSWORD PGHOST PGPORT PGUSER

LOG_FILE="$LOG_DIR/backup.log"
mkdir -p "$LOG_DIR"
log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"; }
fail() { log "❌ $*"; }

# --- Парсинг DATABASE_URL (той самий, що в backup.sh) ------------------------
if [ -n "$DATABASE_URL" ]; then
    rest="${DATABASE_URL#*://}"
    creds=""; hostport="$rest"
    if [[ "$rest" == *@* ]]; then
        creds="${rest%%@*}"
        hostport="${rest#*@}"
    fi
    hostport="${hostport%%/*}"
    if [ -n "$creds" ]; then
        PGUSER="${creds%%:*}"
        PGUSER="${PGUSER:-postgres}"
        PGPASSWORD="${creds#*:}"
    fi
    if [[ "$hostport" == *:* ]]; then
        PGHOST="${hostport%:*}"
        PGPORT="${hostport##*:}"
    else
        PGHOST="$hostport"
        PGPORT="${PGPORT:-5432}"
    fi
    export PGHOST PGPORT PGUSER PGPASSWORD
fi

# --- Аргументи ----------------------------------------------------------------
if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "Використання: $0 <DB_NAME> <BACKUP_FILE> [--yes]" >&2
    echo "  приклад: $0 torgashka_owner_abc12345 backups/torgashka_owner_abc12345_20260901_0200.dump --yes" >&2
    exit 2
fi
DB_NAME="$1"
BACKUP_FILE="$2"
CONFIRM=0
[ "$#" -eq 3 ] && [ "$3" = "--yes" ] && CONFIRM=1

# --- Валідація ----------------------------------------------------------------
for bin in psql pg_restore; do
    command -v "$bin" >/dev/null 2>&1 || { fail "ERROR: $bin не знайдено в PATH"; exit 1; }
done
[ -f "$BACKUP_FILE" ] || { fail "ERROR: файл бекапу не знайдено: $BACKUP_FILE"; exit 1; }
case "$DB_NAME" in
    *[!a-zA-Z0-9_]*)
        fail "ERROR: недопустиме ім'я БД: $DB_NAME (лише [a-zA-Z0-9_])"; exit 2 ;;
esac

log "══════════════════════════════════════════════════════════"
log "▶ restore: db=$DB_NAME file=$BACKUP_FILE"

# Валідність архіву ДО деструктивних дій
if ! pg_restore -w --list "$BACKUP_FILE" >/dev/null 2>&1; then
    fail "Файл не є валідним pg_dump-архівом (custom format): $BACKUP_FILE"
    exit 1
fi
log "✅ Архів валідний (pg_restore --list OK)"

# Доступність сервера
if ! psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT 1" >/dev/null 2>&1; then
    fail "PostgreSQL недоступний ($PGHOST:$PGPORT, user=$PGUSER) — відновлення НЕ виконано"
    exit 1
fi

# Підтвердження (мета-БД — обов'язково явне)
if [ "$DB_NAME" = "pos_system" ] && [ "$CONFIRM" -ne 1 ]; then
    fail "Відновлення мета-БД pos_system вимагає явного --yes (втрата owners_db = втрата маршрутизації)"
    exit 1
fi
if [ "$CONFIRM" -ne 1 ]; then
    echo "⚠️  БД '$DB_NAME' буде ЗНИЩЕНО та відновлено з $BACKUP_FILE"
    read -r -p "Продовжити? [y/N]: " ans
    case "$ans" in
        y|Y|yes|YES) : ;;
        *) echo "Скасовано."; exit 0 ;;
    esac
fi

# --- 1. Terminate активних підключень ----------------------------------------
log "▶ Terminate підключень до $DB_NAME ..."
psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -v ON_ERROR_STOP=1 -c \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '$DB_NAME' AND pid <> pg_backend_pid();" \
    >>"$LOG_FILE" 2>&1 || { fail "Не вдалося terminate підключення"; exit 1; }

# --- 2. DROP DATABASE IF EXISTS ------------------------------------------------
log "▶ DROP DATABASE IF EXISTS $DB_NAME ..."
psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -v ON_ERROR_STOP=1 -c \
    "DROP DATABASE IF EXISTS \"$DB_NAME\";" >>"$LOG_FILE" 2>&1 \
    || { fail "DROP DATABASE не вдалося"; exit 1; }

# --- 3. CREATE DATABASE ----------------------------------------------------------
log "▶ CREATE DATABASE $DB_NAME ..."
psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -v ON_ERROR_STOP=1 -c \
    "CREATE DATABASE \"$DB_NAME\";" >>"$LOG_FILE" 2>&1 \
    || { fail "CREATE DATABASE не вдалося"; exit 1; }

# --- 4. pg_restore -----------------------------------------------------------------
log "▶ pg_restore → $DB_NAME (може тривати хвилини) ..."
if pg_restore -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$DB_NAME" \
        --no-owner --no-privileges --exit-on-error --verbose "$BACKUP_FILE" >>"$LOG_FILE" 2>&1; then
    log "✅ Відновлення $DB_NAME завершено успішно"
else
    fail "pg_restore завершився з помилкою (див. $LOG_FILE)"
    exit 1
fi

# --- 5. Перевірка --------------------------------------------------------------------
log "▶ Перевірка: SELECT count(*) ..."
count="$(psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$DB_NAME" -tAc \
    "SELECT count(*) FROM pg_tables WHERE schemaname='public'" 2>/dev/null || echo "?")"
log "ℹ️  Таблиць у відновленій БД: $count (очікувано ≈34 для owner-БД, ≈мета-таблиці для pos_system)"
log "✅ restore.sh завершено"
