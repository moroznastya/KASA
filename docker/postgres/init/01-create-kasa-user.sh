#!/bin/bash
# ============================================================================
# Torgashka — init-скрипт PostgreSQL (docker-entrypoint-initdb.d)
# ============================================================================
# Створює користувача `kasa` та БД `kasa` для backend-сервісу.
# Виконується автоматично при ПЕРШІЙ ініціалізації volume (порожній pgdata).
# Ідемпотентний: при повторному запуску нічого не ламає.
#
# ⚠️  Пароль `kasa` — для ЛОКАЛЬНОЇ розробки (docker-compose).
#     Для production замініть на надійний пароль!
# ============================================================================
set -e

echo "[init] Torgashka: створюю користувача та БД для backend..."

# Створюємо користувача kasa (якщо ще не існує)
if ! psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='kasa'" | grep -q 1; then
    psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
        CREATE USER kasa WITH PASSWORD 'kasa';
EOSQL
    echo "[init] ✅ Користувач kasa створений"
else
    echo "[init] ⏭️  Користувач kasa вже існує — пропускаю"
fi

# Створюємо БД kasa (якщо ще не існує) з власником kasa
if ! psql -tAc "SELECT 1 FROM pg_database WHERE datname='kasa'" | grep -q 1; then
    psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
        CREATE DATABASE kasa OWNER kasa;
EOSQL
    echo "[init] ✅ БД kasa створена (owner: kasa)"
else
    echo "[init] ⏭️  БД kasa вже існує — пропускаю"
fi

# Надаємо всі права на БД kasa
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    GRANT ALL PRIVILEGES ON DATABASE kasa TO kasa;
EOSQL

echo "[init] ✅ Torgashka init завершено"
