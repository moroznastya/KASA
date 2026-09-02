#!/usr/bin/env bash
# ============================================================================
# backup.sh — ЕТАП 11.1 + ЕТАП 14: резервне копіювання PostgreSQL (LAN)
# ============================================================================
# Що робить:
#   1) Мета-БД pos_system (owners_db, users, auth) — ЗАВЖДИ ПЕРШОЮ і завжди.
#      Втрата мета-БД = втрата маршрутизації до всіх власників (ЕТАП 14) —
#      тому вона має ОКРЕМИЙ таймер з вищою частотою (див. systemd units).
#   2) Кожна БД власника torgashka_owner_* — список з owners_db мета-БД
#      (фолбек: psql -l | grep torgashka_owner_).
#   3) Ротація: видаляє бекапи старші KEEP_DAYS днів (дефолт 14).
#
# Формат: pg_dump custom (pg_restore сумісний, --format=custom).
# Файли:  pos_system_YYYYMMDD_HHMM.dump, torgashka_owner_X_YYYYMMDD_HHMM.dump
# Лог:    logs/backup.log (успіх/невдача кожної БД).
#
# Використання:
#   scripts/backup.sh                     # повний бекап (мета + всі власники)
#   scripts/backup.sh --meta-only         # ТІЛЬКИ мета-БД (ЕТАП 14, таймер 2х/день)
#   META_ONLY=1 scripts/backup.sh         # те саме через env
#
# Необхідні env (PGHOST/PGPORT/PGUSER/PGPASSWORD або DATABASE_URL):
#   PGHOST, PGPORT, PGUSER, PGPASSWORD — стандартні змінні PostgreSQL;
#   або DATABASE_URL=postgresql://user:pass@host:port/dbname (перекриває PGHOST*);
#   META_DB      (дефолт pos_system)  — ім'я мета-БД;
#   BACKUP_DIR   (дефолт ./backups)   — куди писати бекапи;
#   KEEP_DAYS    (дефолт 14)          — скільки днів зберігати бекапи;
#   LOG_DIR      (дефолт ./logs)      — куди писати backup.log;
#   JOBS         (дефолт 4)           — паралельність pg_dump для власників;
#   BACKUP_INCLUDE_TEMPLATE (дефолт 0)— додатково бекапити torgashka_template.
#
# Поведінка: psql/pg_dump запускаються з -w (ніколи не питають пароль
# інтерактивно) — при відсутності PGPASSWORD/DATABASE_URL скрипт швидко
# падає з логом, а не висне на запиті.
#
# Cron-приклад для Docker-розгортання (host-контейнер, без systemd):
#   # Щодня о 02:00 — повний бекап; о 14:00 — тільки мета-БД (ЕТАП 14):
#   0 2 * * *  /opt/torgashka/scripts/backup.sh >> /opt/torgashka/logs/cron-backup.log 2>&1
#   0 14 * * * /opt/torgashka/scripts/backup.sh --meta-only >> /opt/torgashka/logs/cron-backup.log 2>&1
#   # (у контейнері: docker compose exec -T db ... або виконувати на host-машині
#   #  з встановленим postgresql-client; див. docs/infrastructure/backup-restore.md)
# ============================================================================
set -euo pipefail

# --- Конфіг (env з дефолтами) ------------------------------------------------
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PGPASSWORD="${PGPASSWORD:-}"
DATABASE_URL="${DATABASE_URL:-}"
META_DB="${META_DB:-pos_system}"
BACKUP_DIR="${BACKUP_DIR:-./backups}"
KEEP_DAYS="${KEEP_DAYS:-14}"
LOG_DIR="${LOG_DIR:-./logs}"
JOBS="${JOBS:-4}"
BACKUP_INCLUDE_TEMPLATE="${BACKUP_INCLUDE_TEMPLATE:-0}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# BACKUP_DIR/LOG_DIR резолвимо відносно кореня репозиторію, якщо шлях відносний
case "$BACKUP_DIR" in
    /*) : ;;
    *)  BACKUP_DIR="$SCRIPT_DIR/../$BACKUP_DIR" ;;
esac
case "$LOG_DIR" in
    /*) : ;;
    *)  LOG_DIR="$SCRIPT_DIR/../$LOG_DIR" ;;
esac

META_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --meta-only) META_ONLY=1 ;;
        *) echo "[backup] ERROR: невідомий аргумент: $arg" >&2; exit 2 ;;
    esac
done
[ -n "${META_ONLY_ENV:-}" ] && META_ONLY=1   # META_ONLY=1 env-сумісність
META_ONLY="${META_ONLY:-0}"

export PGPASSWORD PGHOST PGPORT PGUSER

LOG_FILE="$LOG_DIR/backup.log"
mkdir -p "$BACKUP_DIR" "$LOG_DIR"

# --- Логування ---------------------------------------------------------------
log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"; }
fail() { log "❌ $*"; }
# Експорт функцій/змінних для паралельних worker'ів (xargs -P → bash -c),
# той самий патерн, що в scripts/migrate_all_owners.sh
export -f log fail
export LOG_FILE BACKUP_DIR PGHOST PGPORT PGUSER PGPASSWORD

# --- Перевірка залежностей ---------------------------------------------------
for bin in psql pg_dump; do
    command -v "$bin" >/dev/null 2>&1 || { fail "ERROR: $bin не знайдено в PATH"; exit 1; }
done

# --- Парсинг DATABASE_URL (перекриває PGHOST/PGPORT/PGUSER/PGPASSWORD) -------
if [ -n "$DATABASE_URL" ]; then
    # підтримка postgresql:// і postgres:// (без IPv6-адрес — документоване обмеження)
    rest="${DATABASE_URL#*://}"
    creds=""; hostport="$rest"
    if [[ "$rest" == *@* ]]; then
        creds="${rest%%@*}"
        hostport="${rest#*@}"
    fi
    hostport="${hostport%%/*}"                 # host[:port]
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
    log "ℹ️  DATABASE_URL задано: host=$PGHOST port=$PGPORT user=$PGUSER (пароль приховано)"
fi

# --- Перевірка доступності PostgreSQL ----------------------------------------
# -w: без інтерактивного запиту пароля — швидкий провал при невірних облікових
if ! psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$META_DB" -tAc "SELECT 1" >/dev/null 2>&1; then
    # остання спроба через postgres-БД (мета-БД може ще не існувати)
    if ! psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT 1" >/dev/null 2>&1; then
        fail "PostgreSQL недоступний ($PGHOST:$PGPORT, user=$PGUSER, db=$META_DB) — бекап НЕ виконано"
        exit 1
    fi
fi
log "✅ PostgreSQL доступний: $PGHOST:$PGPORT (user=$PGUSER, meta=$META_DB)"

# --- Бекап однієї БД ---------------------------------------------------------
backup_db() {
    local db="$1"
    local ts out
    ts="$(date '+%Y%m%d_%H%M')"
    out="$BACKUP_DIR/${db}_${ts}.dump"
    log "▶ pg_dump: $db → $out"
    if pg_dump -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" \
            --format=custom --compress=9 --no-owner --no-privileges \
            -f "$out" "$db" 2>>"$LOG_FILE"; then
        log "✅ OK: $db ($(du -h "$out" | cut -f1))"
        return 0
    else
        rm -f "$out"
        fail "НЕВДАЧА: $db (файл видалено)"
        return 1
    fi
}
export -f backup_db

# --- Список БД власників ------------------------------------------------------
get_owner_dbs() {
    # 1) основний шлях: owners_db у мета-БД
    local list
    list="$(psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$META_DB" -tAc \
        "SELECT db_name FROM owners_db WHERE db_name LIKE 'torgashka_owner_%' ORDER BY db_name" 2>/dev/null || true)"
    if [ -n "$list" ]; then
        echo "$list" | sed '/^[[:space:]]*$/d'
        return 0
    fi
    # 2) фолбек: psql -l | grep
    psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -l 2>/dev/null \
        | awk '{print $1}' | grep -E '^torgashka_owner_' | sort -u
}

# ============================================================================
# ВИКОНАННЯ
# ============================================================================
log "══════════════════════════════════════════════════════════"
log "▶ backup.sh старт (mode=$([ "$META_ONLY" = 1 ] && echo 'meta-only' || echo 'full'))"
log "  dir=$BACKUP_DIR keep_days=$KEEP_DAYS meta=$META_DB"

fail_count=0

# ── 1. Мета-БД — ЗАВЖДИ ПЕРШОЮ (ЕТАП 14: незалежна, найчастіша) ────────────
backup_db "$META_DB" || fail_count=$((fail_count + 1))

# ── 2. torgashka_template (опційно) ─────────────────────────────────────────
if [ "$BACKUP_INCLUDE_TEMPLATE" = "1" ]; then
    if psql -w -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc \
        "SELECT 1 FROM pg_database WHERE datname='torgashka_template'" 2>/dev/null | grep -q 1; then
        backup_db "torgashka_template" || fail_count=$((fail_count + 1))
    fi
fi

# ── 3. БД власників (пропускаємо в режимі meta-only) ────────────────────────
if [ "$META_ONLY" = "1" ]; then
    log "ℹ️  meta-only: БД власників не бекапимо (окремий повний цикл)."
else
    mapfile -t OWNER_DBS < <(get_owner_dbs)
    if [ "${#OWNER_DBS[@]}" -eq 0 ]; then
        log "ℹ️  Список власників порожній — бекапимо тільки мета-БД (це нормально)."
    else
        log "ℹ️  Знайдено БД власників (${#OWNER_DBS[@]}): ${OWNER_DBS[*]}"
        # паралельно, як у migrate_all_owners.sh
        printf '%s\n' "${OWNER_DBS[@]}" | xargs -P "$JOBS" -I{} bash -c 'backup_db "$@"' _ {} \
            || fail_count=$((fail_count + 1))
    fi
fi

# ── 4. Ротація: видалити бекапи старші KEEP_DAYS ────────────────────────────
log "▶ Ротація: видаляю бекапи старші ${KEEP_DAYS} дн. у $BACKUP_DIR"
deleted="$(find "$BACKUP_DIR" -maxdepth 1 -type f \
    \( -name 'pos_system_*.dump' -o -name 'torgashka_owner_*.dump' -o -name 'torgashka_template_*.dump' \) \
    -mtime "+${KEEP_DAYS}" -print -delete 2>/dev/null | wc -l)"
[ "$deleted" -gt 0 ] && log "🗑  Видалено старих бекапів: $deleted"
log "📦 Всього бекапів у $BACKUP_DIR: $(find "$BACKUP_DIR" -maxdepth 1 -name '*.dump' | wc -l)"

# ── 5. Підсумок ─────────────────────────────────────────────────────────────
if [ "$fail_count" -gt 0 ]; then
    fail "Завершено з помилками: $fail_count БД не забекaплено"
    exit 1
fi
log "✅ backup.sh завершено успішно"
