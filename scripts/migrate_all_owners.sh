#!/usr/bin/env bash
# ============================================================================
# migrate_all_owners.sh — Етап 9.3: застосування міграцій до всіх БД власників
# ============================================================================
# Для кожної БД з owners_db (мета-БД pos_system) + torgashka_template:
#   alembic upgrade head (ідемпотентно, через DATABASE_URL підстановку).
#
# Порядок:
#   1) torgashka_template — завжди ПЕРШОЮ (серійно) — база-еталон для нових БД.
#   2) torgashka_owner_* з owners_db — паралельно (xargs -P ${JOBS}, за змовч. 4).
#
# Стратегія для БЕЗВЕРСІЙНИХ БД (створені через schema.sql, без alembic_version):
#   • schema.sql=head (є users.owner_id)        → stamp head (схема вже актуальна);
#   • стара schema.sql (0001..0005, без owner_id)→ stamp ${STAMP_BASE} + upgrade head
#     (прогін повного ланцюга з 0001 падає: доіснуюче дублювання 0001/62c0fd0b93a4);
#   • порожня БД-оболонка (<5 табл.)            → stamp head (наповнить setup.rs з template).
# Для версійних БД — звичайний upgrade head (ідемпотентний).
# ============================================================================
set -euo pipefail

# --- Конфіг (env з дефолтами для локальної розробки) ------------------------
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PGPASSWORD="${PGPASSWORD:-}"
META_DB="${META_DB:-pos_system}"
JOBS="${JOBS:-4}"
STAMP_BASE="${STAMP_BASE:-0005_onboarding_completed}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KASA_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BACKEND_DIR="${BACKEND_DIR:-$KASA_ROOT/backend}"
ALEMBIC_BIN="${ALEMBIC_BIN:-alembic}"

export PGPASSWORD PGHOST PGPORT PGUSER

# Перевірка залежностей
command -v psql >/dev/null 2>&1 || { echo "[migrate] ERROR: psql не знайдено" >&2; exit 1; }
[ -d "$BACKEND_DIR/alembic" ] || { echo "[migrate] ERROR: $BACKEND_DIR/alembic не знайдено" >&2; exit 1; }

PSQL=(psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc)

# --- Міграція однієї БД ------------------------------------------------------
migrate_db() {
    local db="$1"
    local log
    log="$(mktemp /tmp/migrate_${db}.XXXXXX.log 2>/dev/null || echo /tmp/migrate_${db}.log)"
    echo "[migrate] ▶ $db ..."

    local versioned table_count head_rev has_owner_id
    versioned="$(psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$db" -tAc \
        "SELECT to_regclass('public.alembic_version') IS NOT NULL" 2>/dev/null || echo f)"
    table_count="$(psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$db" -tAc \
        "SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE' AND table_name <> 'alembic_version'" 2>/dev/null || echo 0)"
    # Сентинел 0008: поточна schema.sql синхронізована до head (містить owner_id).
    has_owner_id="$(psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$db" -tAc \
        "SELECT count(*) FROM information_schema.columns WHERE table_name='users' AND column_name='owner_id'" 2>/dev/null || echo 0)"
    head_rev="$(cd "$BACKEND_DIR" && "$ALEMBIC_BIN" heads 2>/dev/null | awk '{print $1}' | head -1)"
    head_rev="${head_rev:-0008_owner_id_in_users}"

    local rc=0
    if [ "${table_count:-0}" -lt 5 ]; then
        # Порожня БД-оболонка (незавершений onboarding, навіть якщо є alembic_version
        # від невдалого прогону): schema відсутня → лише stamp head.
        # Наповнення зробить setup.rs з template (який вже на head).
        echo "[migrate]   $db: порожня (${table_count} табл.) → stamp ${head_rev}"
        ( cd "$BACKEND_DIR" && DATABASE_URL="postgresql+asyncpg://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${db}" \
            "$ALEMBIC_BIN" stamp "$head_rev" ) >"$log" 2>&1 || rc=1
    elif [ "$versioned" = "t" ]; then
        # Нормальний шлях: ланцюг вже є — просто upgrade head (ідемпотентно)
        ( cd "$BACKEND_DIR" && DATABASE_URL="postgresql+asyncpg://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${db}" \
            "$ALEMBIC_BIN" upgrade head ) >"$log" 2>&1 || rc=1
    elif [ "${has_owner_id:-0}" -ge 1 ]; then
        # Безверсійна БД з АКТУАЛЬНОЇ schema.sql (синхронізована до head):
        # схема вже містить FORCE RLS, client_receipt_uuid, owner_id → stamp head.
        echo "[migrate]   $db: без alembic_version, schema.sql=head → stamp ${head_rev}"
        ( cd "$BACKEND_DIR" && DATABASE_URL="postgresql+asyncpg://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${db}" \
            "$ALEMBIC_BIN" stamp "$head_rev" ) >"$log" 2>&1 || rc=1
    else
        # Безверсійна БД зі СТАРОЇ schema.sql (еквівалент 0001..0005, без owner_id):
        # stamp бази + upgrade застосовує дельту (0005_force_rls, 0006, 0008).
        # Повний ланцюг з 0001 на такій БД падає (доіснуюче дублювання 0001/62c0fd0b93a4).
        echo "[migrate]   $db: без alembic_version, стара schema.sql → stamp ${STAMP_BASE} + upgrade head"
        ( cd "$BACKEND_DIR" && DATABASE_URL="postgresql+asyncpg://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${db}" \
            "$ALEMBIC_BIN" stamp "$STAMP_BASE" >/dev/null 2>&1 && \
            DATABASE_URL="postgresql+asyncpg://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${db}" \
            "$ALEMBIC_BIN" upgrade head ) >"$log" 2>&1 || rc=1
    fi

    if [ "$rc" -ne 0 ]; then
        echo "[migrate] ✗ FAIL $db" >&2
        tail -5 "$log" >&2
        rm -f "$log"
        return 1
    fi
    echo "[migrate] ✓ OK  $db"
    rm -f "$log"
    return 0
}
export -f migrate_db
export BACKEND_DIR ALEMBIC_BIN STAMP_BASE PGUSER PGPASSWORD PGHOST PGPORT

# --- Збір списку БД ----------------------------------------------------------
tmp_list="$(mktemp)"
trap 'rm -f "$tmp_list"' EXIT

# 1) torgashka_template — завжди першою (якщо існує)
if [ "$("${PSQL[@]}" "SELECT 1 FROM pg_database WHERE datname='torgashka_template'" 2>/dev/null)" = "1" ]; then
    echo "torgashka_template" >"$tmp_list"
fi

# 2) власники з owners_db (мета-БД)
psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$META_DB" -tAc \
    "SELECT db_name FROM owners_db WHERE db_name LIKE 'torgashka_owner_%' ORDER BY db_name" \
    >>"$tmp_list" 2>/dev/null || true

# --- Порожній список: не падаємо --------------------------------------------
if [ ! -s "$tmp_list" ]; then
    echo "[migrate] Жодної БД для міграції (torgashka_template відсутня, owners_db порожній). Вихід 0."
    exit 0
fi

echo "[migrate] Список БД: $(tr '\n' ' ' <"$tmp_list")"
echo "[migrate] JOBS=${JOBS}, STAMP_BASE=${STAMP_BASE}"

# --- Виконання ---------------------------------------------------------------
# template — серійно
template_db="$(grep -x torgashka_template "$tmp_list" || true)"
owners_list="$(grep -vx torgashka_template "$tmp_list" || true)"

fail=0
if [ -n "$template_db" ]; then
    migrate_db "$template_db" || fail=1
fi

# власники — паралельно (xargs -P)
if [ -n "$owners_list" ]; then
    echo "$owners_list" | xargs -P "$JOBS" -I{} bash -c 'migrate_db "$@"' _ {} || fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo "[migrate] ❌ Є помилки (див. вище)."
    exit 1
fi
echo "[migrate] ✅ Всі БД актуальні (head)."
