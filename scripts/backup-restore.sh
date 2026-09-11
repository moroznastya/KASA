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
#   scripts/backup-restore.sh --queue <OFFLINE_DB_BACKUP> [--yes] [--force-live]
#
#   <DB_NAME>      — ім'я БД, яку відновлюємо (напр. torgashka_owner_abc12345)
#   <BACKUP_FILE>  — .dump файл (custom format), напр. backups/torgashka_owner_abc12345_20260901_0200.dump
#   --yes          — підтвердження деструктивної операції (без інтерактивного питання)
#
# Відновлення локальної SQLite-черги каси (--queue, Фаза 3.9; симетрично до
# `backup.sh` §3b):
#   scripts/backup-restore.sh --queue backups/offline_20260901_0200.db --yes
#     • джерело перевіряється (quick_check + наявність таблиці outbox) ДО змін;
#     • поточна черга зберігається у <offline.db>.pre-restore_YYYYMMDD_HHMM
#       (консистентний `sqlite3 .backup`, не cp);
#     • якщо поруч є непорожні offline.db-wal/-shm — скрипт відмовляє:
#       застосунок, схоже, ще працює (обхід — --force-live, на свій ризик);
#     • ціль — OFFLINE_DB (дефолт ${XDG_DATA_HOME:-$HOME/.local/share}/torgashka/offline.db).
#
# ⚠️  Не відновлюйте чергу «поверх» уже синхронізованих агрегатів не перевіривши
#     стан: ідемпотентність за client_uuid лишається (повторний push → already_exists),
#     але `receipts.synced=1` НЕ означає «доставлено» — істина в outbox.status
#     (docs/infrastructure/backup-restore.md §9.6-§9.7).
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

# --- Режим відновлення SQLite-черги каси (--queue) ---------------------------
# Симетрично до backup.sh §3b. PG-гілка нижче в цьому режимі не виконується:
# відновлення черги не потребує ні psql, ні pg_restore.
# Шлях продубльовано з коду: `OfflineDatabase::default_db_path()`
# (crates/torgashka-infrastructure/src/offline/db.rs:78-89).
OFFLINE_DB="${OFFLINE_DB:-${XDG_DATA_HOME:-$HOME/.local/share}/torgashka/offline.db}"

restore_queue() {
    local src="$1" confirm="$2" force_live="$3"
    local chk ts dest_bak cur

    if ! command -v sqlite3 >/dev/null 2>&1; then
        fail "ERROR: sqlite3 не знайдено в PATH — відновлення черги неможливе"
        return 1
    fi
    if [ ! -f "$src" ]; then
        fail "ERROR: файл бекапу черги не знайдено: $src"
        return 1
    fi

    log "══════════════════════════════════════════════════════════"
    log "▶ restore(queue): src=$src → dest=$OFFLINE_DB"

    # 1. Валідність джерела ДО будь-яких змін
    chk="$(sqlite3 "$src" 'PRAGMA quick_check;' 2>/dev/null || echo 'quick_check error')"
    if [ "$chk" != "ok" ]; then
        fail "Файл не є цілим SQLite-файлом (quick_check: $chk)"
        return 1
    fi
    if ! sqlite3 "$src" "SELECT 1 FROM sqlite_master WHERE type='table' AND name='outbox';" 2>/dev/null | grep -q 1; then
        fail "У файлі немає таблиці outbox — це не бекап черги каси"
        return 1
    fi
    log "✅ Джерело валідне: outbox записів — $(sqlite3 "$src" 'SELECT count(*) FROM outbox;' 2>/dev/null || echo '?')"

    # 2. Живий застосунок: непорожні -wal/-shm означають відкриту SQLite —
    #    підміна файлу під живим процесом знищить його транзакції.
    if { [ -s "$OFFLINE_DB-wal" ] || [ -s "$OFFLINE_DB-shm" ]; } && [ "$force_live" != "1" ]; then
        fail "Схоже, каса ще працює (є $OFFLINE_DB-wal/-shm). ЗУПИНІТЬ застосунок і повторіть."
        fail "Свідомий обхід (на свій ризик): --force-live — файли -wal/-shm буде видалено."
        return 1
    fi

    # 3. Страхувальна копія поточної черги (що втрачаємо)
    if [ -f "$OFFLINE_DB" ]; then
        cur="$(sqlite3 "$OFFLINE_DB" "SELECT count(*) FROM outbox WHERE status IN ('pending','failed');" 2>/dev/null || echo '?')"
        log "ℹ️  Поточна черга: pending+failed = $cur (буде перезаписано)"
        # Секунди в імені: повторний restore у ту саму хвилину не має затирати
        # попередню страхувальну копію (виявлено смоук-тестом Фази 3.9).
        ts="$(date '+%Y%m%d_%H%M%S')"
        dest_bak="$OFFLINE_DB.pre-restore_${ts}"
        if sqlite3 "$OFFLINE_DB" ".backup '$dest_bak'" 2>>"$LOG_FILE"; then
            log "🛟 Страхувальна копія поточної черги: $dest_bak"
        else
            fail "Не вдалося зробити страхувальну копію поточної черги ($dest_bak)"
        fi
    else
        log "ℹ️  Робочого offline.db немає — файл буде створено з бекапу."
    fi

    # 4. Підтвердження (деструктивно)
    if [ "$confirm" -ne 1 ]; then
        echo "⚠️  Поточний $OFFLINE_DB буде ПЕРЕЗАПИСАНО бекапом $src"
        read -r -p "Продовжити? [y/N]: " ans
        case "$ans" in
            y|Y|yes|YES) : ;;
            *) echo "Скасовано."; return 0 ;;
        esac
    fi

    # 5. Атомарна підміна: копія у тому ж каталозі → перейменування
    mkdir -p "$(dirname "$OFFLINE_DB")"
    if ! cp "$src" "$OFFLINE_DB.restore_tmp"; then
        fail "Не вдалося скопіювати бекап у $OFFLINE_DB.restore_tmp"
        return 1
    fi
    if ! mv -f "$OFFLINE_DB.restore_tmp" "$OFFLINE_DB"; then
        fail "Не вдалося замінити $OFFLINE_DB"
        return 1
    fi
    # Залишкові WAL-файли попереднього життя (застосунок зупинено/--force-live)
    if [ -f "$OFFLINE_DB-wal" ]; then rm -f "$OFFLINE_DB-wal"; fi
    if [ -f "$OFFLINE_DB-shm" ]; then rm -f "$OFFLINE_DB-shm"; fi

    # 6. Перевірка результату
    chk="$(sqlite3 "$OFFLINE_DB" 'PRAGMA quick_check;' 2>/dev/null || echo 'quick_check error')"
    if [ "$chk" != "ok" ]; then
        fail "Після відновлення quick_check: $chk (використайте страхувальну копію .pre-restore_*)"
        return 1
    fi
    log "✅ Черга відновлена: pending+failed = $(sqlite3 "$OFFLINE_DB" "SELECT count(*) FROM outbox WHERE status IN ('pending','failed');" 2>/dev/null || echo '?')"
    log "⚠️  ПЕРЕД синком перевірте, чи знімок не «повертає» вже доставлені агрегати:"
    log "    • повторний push ідемпотентний за client_uuid (сервер → already_exists) — ADR-0007 §9;"
    log "    • але receipts.synced=1 НЕ означає «доставлено» — істина в outbox.status;"
    log "    • знімок, знятий «після ack», може повернути агрегати з outbox.status='done'."
    log "    Деталі: docs/infrastructure/backup-restore.md §9.6-§9.7"
    log "✅ restore(queue) завершено"
    return 0
}

if [ "${1:-}" = "--queue" ]; then
    shift
    if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
        echo "Використання: $0 --queue <OFFLINE_DB_BACKUP> [--yes] [--force-live]" >&2
        echo "  приклад: $0 --queue backups/offline_20260901_0200.db --yes" >&2
        exit 2
    fi
    QUEUE_SRC="$1"; shift
    QUEUE_CONFIRM=0
    QUEUE_FORCE_LIVE=0
    for a in "$@"; do
        case "$a" in
            --yes) QUEUE_CONFIRM=1 ;;
            --force-live) QUEUE_FORCE_LIVE=1 ;;
            *) echo "ERROR: невідомий аргумент: $a" >&2; exit 2 ;;
        esac
    done
    if restore_queue "$QUEUE_SRC" "$QUEUE_CONFIRM" "$QUEUE_FORCE_LIVE"; then
        exit 0
    else
        exit 1
    fi
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
