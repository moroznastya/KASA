# Резервне копіювання та відновлення PostgreSQL — Torgashka POS (LAN)

> ЕТАП 11.1 + ЕТАП 14 плану `Projects/database-architecture-implementation-plan.md`
> Цільовий сценарій: **LAN-розгортання** (виділений PostgreSQL на сервері власника або embedded PG).
> Гілка: `feat/rust-migration` · Репозиторій: `Projects/kasa`

---

## 1. Що і як бекапиться

| Об'єкт | Ім'я | Частота | Чому |
|---|---|---|---|
| Мета-БД | `pos_system` | **2× на добу** (02:00, 14:00) | Втрата = втрата `owners_db` = втрата маршрутизації до **всіх** власників (ЕТАП 14) |
| БД власника | `torgashka_owner_<id>` | 1× на добу (02:00) | Бізнес-дані (чеки, залишки, товари) |
| Шаблон (опція) | `torgashka_template` | 1× на добу (опц. `BACKUP_INCLUDE_TEMPLATE=1`) | Схема для створення нових власників |

- Формат: `pg_dump --format=custom` (стиснутий, сумісний з `pg_restore`).
- Імена файлів: `pos_system_YYYYMMDD_HHMM.dump`, `torgashka_owner_X_YYYYMMDD_HHMM.dump`.
- Ротація: `KEEP_DAYS` (дефолт **14**) — старіші бекапи видаляються автоматично.
- Лог: `logs/backup.log` (успіх/невдача кожної БД з часовою міткою).

```
backups/
├── pos_system_20260901_0200.dump
├── pos_system_20260901_1400.dump
├── torgashka_owner_abc12345_20260901_0200.dump
└── torgashka_owner_def67890_20260901_0200.dump
```

---

## 2. Вимоги та конфігурація

### Необхідні змінні оточення (спосіб 1 — компоненти)

| Змінна | Дефолт | Опис |
|---|---|---|
| `PGHOST` | `localhost` | Host PostgreSQL |
| `PGPORT` | `5432` | Порт (Docker: `5434` для `db`, `5433` для `db-test`) |
| `PGUSER` | `postgres` | Користувач (потрібні права на `pg_dump` усіх БД) |
| `PGPASSWORD` | — | 🔐 Пароль (той самий, що `DB_PASSWORD` у `.env`) |

### Або спосіб 2 — повний DSN (перекриває PGHOST*)

```
DATABASE_URL=postgresql://postgres:ПАРОЛЬ@localhost:5434/pos_system
```

### Опційні змінні

| Змінна | Дефолт | Опис |
|---|---|---|
| `META_DB` | `pos_system` | Ім'я мета-БД |
| `BACKUP_DIR` | `./backups` | Каталог бекапів (відносно кореня репозиторію або абсолютний) |
| `LOG_DIR` | `./logs` | Каталог логів |
| `KEEP_DAYS` | `14` | Днів зберігання бекапів |
| `JOBS` | `4` | Паралельність `pg_dump` для БД власників |
| `BACKUP_INCLUDE_TEMPLATE` | `0` | `1` — додатково бекапити `torgashka_template` |

---

## 3. Швидкий старт (вручну)

```bash
cd Projects/kasa

# Локальна розробка (компоненти)
PGPASSWORD='пароль' scripts/backup.sh

# Або через DSN
DATABASE_URL='postgresql://postgres:пароль@localhost:5434/pos_system' scripts/backup.sh

# Тільки мета-БД (ЕТАП 14)
scripts/backup.sh --meta-only

# Перевірка результату
ls -lh backups/
cat logs/backup.log
```

Режими:
- `backup.sh` — повний бекап: **мета-БД першою** → `torgashka_owner_*` (паралельно) → ротація.
- `backup.sh --meta-only` (`META_ONLY=1`) — **тільки мета-БД** (для окремого таймера 14:00).

---

## 4. Автоматизація

### 4.1 systemd (Linux host)

Файли: `scripts/systemd/` — 2 пари unit/timer + `backup.env.example`.

```bash
# 1. Скрипт та конфіг
sudo mkdir -p /etc/torgashka
sudo cp scripts/systemd/backup.env.example /etc/torgashka/backup.env
sudo chmod 600 /etc/torgashka/backup.env
#    → відредагуйте /etc/torgashka/backup.env (PGHOST/PGPORT/PGUSER/PGPASSWORD)

# 2. Units (шлях до скрипта в .service — замініть /opt/torgashka на фактичний)
sudo cp scripts/systemd/torgashka-backup.{service,timer} /etc/systemd/system/
sudo cp scripts/systemd/torgashka-meta-backup.{service,timer} /etc/systemd/system/

# 3. Активація
sudo systemctl daemon-reload
sudo systemctl enable --now torgashka-backup.timer      # щодня 02:00
sudo systemctl enable --now torgashka-meta-backup.timer # 02:00 і 14:00

# 4. Перевірка
systemctl list-timers 'torgashka-*'
sudo journalctl -u torgashka-backup.service -e
```

Розклад:
- `torgashka-backup.timer` → `OnCalendar=*-*-* 02:00:00` (повний).
- `torgashka-meta-backup.timer` → `02:00:00` + `14:00:00` (тільки мета-БД).

### 4.2 Docker-розгортання (cron на host)

Якщо PostgreSQL працює в контейнері (`docker compose up -d db`, порт `5434`), на host-машині з встановленим `postgresql-client`:

```cron
# crontab -e
# Щодня 02:00 — повний бекап
0 2 * * *  /opt/torgashka/scripts/backup.sh >> /opt/torgashka/logs/cron-backup.log 2>&1
# Щодня 14:00 — тільки мета-БД (ЕТАП 14)
0 14 * * * /opt/torgashka/scripts/backup.sh --meta-only >> /opt/torgashka/logs/cron-backup.log 2>&1
```

Або cron **усередині** контейнера (потрібен `postgresql-client` в образі та змонтований каталог бекапів):

```cron
0 2 * * *  PGPASSWORD=$POSTGRES_PASSWORD pg_dump -U postgres -Fc -f /backups/pos_system_$(date +\%Y\%m\%d_\%H\%M).dump pos_system
```

> Рекомендація: бекапи писати на **окремий носій/змонтований volume**, а не в volume самої БД.

---

## 5. Відновлення (backup-restore.sh)

Документована, деструктивна процедура для **однієї БД**:

```bash
scripts/backup-restore.sh <DB_NAME> <BACKUP_FILE> [--yes]
```

Алгоритм скрипта:
1. Валідація файлу (`pg_restore --list`) — до будь-яких руйнівних дій;
2. Перевірка доступності PostgreSQL;
3. Підтвердження (інтерактивне; для `pos_system` — **обов'язково** `--yes`);
4. `pg_terminate_backend` усіх активних підключень до цільової БД;
5. `DROP DATABASE IF EXISTS` → `CREATE DATABASE`;
6. `pg_restore --no-owner --no-privileges --exit-on-error`;
7. Перевірка: кількість таблиць у відновленій БД.

Приклади:

```bash
# Відновлення БД власника (інтерактивне підтвердження)
PGPASSWORD='пароль' scripts/backup-restore.sh torgashka_owner_abc12345 \
    backups/torgashka_owner_abc12345_20260901_0200.dump

# Автоматичне відновлення (--yes)
DATABASE_URL='postgresql://postgres:пароль@localhost:5434/pos_system' \
    scripts/backup-restore.sh torgashka_owner_abc12345 \
    backups/torgashka_owner_abc12345_20260901_0200.dump --yes

# Відновлення мета-БД — ТІЛЬКИ з --yes (критично!)
scripts/backup-restore.sh pos_system backups/pos_system_20260901_1400.dump --yes
```

> ⚠️ Відновлення мета-БД `pos_system` без `--yes` **заборонено** скриптом: втрата
> `owners_db` означає, що фасад не знатиме, де дані власників.

---

## 6. Перевірка бекапів (рекомендована практика)

1. **Щоденна (автоматична):** перевірте, що `logs/backup.log` містить `✅ OK` для
   `pos_system` і кожної `torgashka_owner_*`, а в `backups/` з'явилися файли за поточну добу.

2. **Щотижнева (ручна):** валідність архіву без відновлення:
   ```bash
   pg_restore --list backups/pos_system_$(date +%Y%m%d_0200).dump | head -20
   ```

3. **Щомісячна — тест відновлення (ЕТАП 11.2):**
   ```bash
   # бекап → DROP → restore → COUNT збігається
   PGPASSWORD='пароль' scripts/backup.sh --meta-only
   F=backups/pos_system_$(date +%Y%m%d_%H%M).dump
   PGPASSWORD='пароль' psql -h localhost -p 5434 -U postgres -d pos_system -tAc \
       "SELECT count(*) FROM owners_db"        # запам'ятати N
   PGPASSWORD='пароль' scripts/backup-restore.sh pos_system "$F" --yes
   PGPASSWORD='пароль' psql -h localhost -p 5434 -U postgres -d pos_system -tAc \
       "SELECT count(*) FROM owners_db"        # має дорівнювати N
   ```

---

## 7. Пошук проблем

| Симптом | Причина / рішення |
|---|---|
| `PostgreSQL недоступний` | Невірний `PGHOST/PGPORT` або пароль; перевірте `docker compose ps`, `pg_isready -h localhost -p 5434` |
| `НЕВДАЧА: <db> (файл видалено)` | `pg_dump` не має прав на БД або БД не існує; перевірте лог-рядки `pg_dump` у `backup.log` |
| `Список власників порожній` | Нормально для нового сервера: бекапиться тільки `pos_system` (exit 0) |
| Бекап `torgashka_owner_*` не створюється | `owners_db` порожній або запит недоступний — скрипт робить фолбек на `psql -l \| grep` |
| `pg_dump: error: connection to server failed` | БД у Docker: перевірте проброс порту (`5434:5432`) і `PGPORT=5434` |
| Ротація не видаляє старі файли | Перевірте `KEEP_DAYS`; видаляються лише файли за масками `*_*.dump` у `BACKUP_DIR` |

---

## 8. Безпека

- Паролі — тільки в `/etc/torgashka/backup.env` (`chmod 600`), **не** в unit-файлах і не в git.
- `.env`, `backups/`, `logs/` — у `.gitignore` (бекапи містять фіскальні дані).
- Бекапи на LAN-сервері: рекомендується окремий диск/розділ або мережевий mount
  (для фіскального POS — зовнішній носій із періодичним копіюванням офлайн).
- Відновлення — лише під адміністративним користувачем (`postgres`), з явним `--yes`.
