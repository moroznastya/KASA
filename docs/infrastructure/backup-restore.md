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
| Ротація не видаляє старі файли | Перевірте `KEEP_DAYS`; видаляються лише файли за масками `*_*.dump` і `offline_*.db` у `BACKUP_DIR` |
| Копії черги немає в `backups/` | `offline.db` відсутній на цій машині (норма), або задано `--meta-only`/`--no-queue`/`BACKUP_QUEUE=0` — див. `grep offline.db logs/backup.log` |
| `sqlite3 не знайдено, а offline.db ІСНУЄ` | На POS-вузлі немає `sqlite3`; `cp` заборонено свідомо → бекап черги не робиться, exit 1 | Встановіть `sqlite3` на касі |
| `Error: database is locked` при бекапі черги | Каса активно пише в SQLite | Штатно: `.timeout 10000` + 3 спроби (`scripts/backup.sh:211-226`); при систематичному повторі — перевірте диск/лочки |
| `--queue`: «Схоже, каса ще працює» | Поруч непорожні `offline.db-wal`/`-shm` | Зупиніть застосунок і повторіть; обхід — `--force-live` |

---

## 8. Безпека

- Паролі — тільки в `/etc/torgashka/backup.env` (`chmod 600`), **не** в unit-файлах і не в git.
- `.env`, `backups/`, `logs/` — у `.gitignore` (бекапи містять фіскальні дані).
- Бекапи на LAN-сервері: рекомендується окремий диск/розділ або мережевий mount
  (для фіскального POS — зовнішній носій із періодичним копіюванням офлайн).
- Відновлення — лише під адміністративним користувачем (`postgres`), з явним `--yes`.

---

## 9. Локальна SQLite-черга вузла (каса/standby) — ADR-0007

> Фаза 3.6. На вузлі-standby POS-документи каси (чеки, повернення, накладні, списання,
> переміщення, касові операції, оплати боргів, повернення постачальнику) пишуться НЕ в
> PostgreSQL, а в **локальну SQLite-чергу вузла**, звідки доїжджають на primary приймачами
> (`torgashka-api/src/sync.rs`, `torgashka-api/src/sync_receivers.rs`). Адмін/мережеві операції
> йдуть HTTP pass-through на primary (ADR-0007 §11.2); фіскальна черга ПРРО — свідома межа
> (`ProxyToPrimary`, ADR-0007 §11.7.9.7, `docs/adr/ADR-0007-standby-write-routing.md:1291`).
>
> **Критично:** несинхронізована черга містить РЕАЛЬНІ бізнес-дані (чеки, борги, залишки).
> Її втрата = втрата продажів. PG-бекапи (розділи 1–5) цю чергу **не покривають**.

### 9.1 Що саме бекапити (шлях узятий з коду)

- **Файл:** `offline.db` **разом із** `offline.db-wal` і `offline.db-shm`.
  - Дефініція шляху: `OfflineDatabase::default_db_path()` →
    `frontend/src-tauri/crates/torgashka-infrastructure/src/offline/db.rs:78-88`:
    `dirs_next::data_dir()/torgashka/offline.db` (`dirs-next = "2"` —
    `crates/torgashka-infrastructure/Cargo.toml:20`).
    На Linux `dirs_next::data_dir()` = `$XDG_DATA_HOME` або `~/.local/share`, тобто типово
    **`~/.local/share/torgashka/offline.db`**.
  - Точний шлях у рантаймі повертає `get_offline_stats()` → поле `db_path`
    (`offline/commands.rs:559-578`), внутрішній геттер — `offline/db.rs:67`.
  - **WAL увімкнено при кожному відкритті:** `offline/sync_push.rs:731-735`
    (`PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;`). Тому копія лише `offline.db`
    без `-wal` **небезпечна**: закомічені, але ще не перенесені з WAL транзакції = втрачений
    хвіст черги.
- **Що всередині (бізнес-цінність):** таблиця `outbox` (черга агрегатів) + локальні агрегати
  й залишок `stock`. Міграції: `offline/migrations/offline/0002_sync_meta.sql` (outbox),
  `0005_local_stock.sql` (stock), `0006_transaction_tables.sql`, `0010_local_invoices.sql`,
  `0011_local_cash.sql`, `0012_local_return_debtor_ledger.sql`
  (включення — `offline/migrations.rs:62-63`).

### 9.2 Чому це критично (ADR-0007 §9, §10)

- Агрегат доставлено **лише після ack** сервера. `pending`/`failed` в `outbox` = невивантажені
  продажі: `outbox.status` — `0002_sync_meta.sql:16-22`; `in_flight` у CHECK **резервований і
  кодом не використовується** (`0002_sync_meta.sql:19-22`).
- Лічильники: `sync_status.pending_count` (`offline/commands.rs:441-455`),
  `get_unsynced_count()` (тільки `pending`, `offline/commands.rs:211-216`),
  `outbox_stats` (`offline/sync_push.rs:311-350`).
- Переходи статусів: `created`/`already_exists` → `done` (`sync_push.rs:560-567`),
  бізнес-помилка 400/422 → `failed` без retry (`:532-539`), 5xx/429 → backoff на весь пакет
  (`:524-530`), після `MAX_ATTEMPTS = 10` → `failed` (`:38`, `:630-660`).
- Алерт: `sync_health.degraded = failed > 0 АБО stale pending > BACKOFF_CAP_SECS (3600 с)`
  (`sync_push.rs:375-425`).

### 9.3 Коли бекапити

- **автоматично** — повним циклом `scripts/backup.sh` (без прапорців), разом із PG-бекапами
  (розділ 4); черга бекапиться тим самим таймером, окремого завдання не потрібно (Фаза 3.9);
- **перед оновленням** застосунку (новий бінарник застосовує міграції черги —
  `offline/migrations.rs`, `sync_push.rs:736`);
- **після оновлення** — до повернення каси в роботу;
- **окремо** — на `--meta-only`-таймері черга НЕ бекапиться свідомо (там лише мета-БД);
  потрібна черга саме в цей момент → запустіть повний цикл вручну;
- **обов'язково перед DR-операціями** (`promote`, `repoint-primary`, ручний `pg_basebackup`) —
  доки черга непорожня (чому — див. `docs/operations/disaster-recovery-network.md` §6).

### 9.4 Команди бекапу — РЕАЛІЗОВАНО (Фаза 3.9)

**Код-підтверджено:** чергу бекапить штатний скрипт, консистентно і без `cp`.

- `scripts/backup.sh:186-247` — `backup_queue()`; ядро — `sqlite3 "$OFFLINE_DB" ".timeout 10000"
  ".backup '$out'"` (`scripts/backup.sh:215`). Busy-timeout 10 с + до 3 спроб
  (`scripts/backup.sh:208-226`): під навантаженням живої каси `.backup` може віддати
  `database is locked` (перевірено смоук-тестом Фази 3.9).
- Виклик у повному циклі — `scripts/backup.sh:304`; `--meta-only` чергу не чіпає
  (`scripts/backup.sh:298-303`); опт-аут — `--no-queue` / `BACKUP_QUEUE=0`
  (`scripts/backup.sh:98`, `:80-81`).
- Джерело — `$OFFLINE_DB`; дефолт `${XDG_DATA_HOME:-$HOME/.local/share}/torgashka/offline.db`
  (`scripts/backup.sh:80`, продубльовано з `offline/db.rs:78-89`).
- Файл — `offline_YYYYMMDD_HHMM.db` у `$BACKUP_DIR`; ротація — та сама, за `KEEP_DAYS`
  (`scripts/backup.sh:310`).
- Перевірка копії перед визнанням успіху: `PRAGMA quick_check` = `ok` **і** наявність таблиці
  `outbox` (`scripts/backup.sh:229-236`); інакше файл видаляється і пишеться НЕВДАЧА.
- Лог — той самий `logs/backup.log`; у виводі явне попередження оператору, що копія містить
  НЕСИНХРОНІЗОВАНІ ПРОДАЖІ: `pending+failed = N` (`scripts/backup.sh:244-245`).

```bash
# Повний бекап: PG + черга (те саме, що викликає systemd/cron)
scripts/backup.sh

# Повний бекап без черги (напр. на центральному сервері, де offline.db немає)
scripts/backup.sh --no-queue

# Лише черга, вручну — тим самим способом, що і скрипт:
DB="$HOME/.local/share/torgashka/offline.db"; OUT="./backups/offline_$(date +%Y%m%d_%H%M).db"
sqlite3 "$DB" ".timeout 10000" ".backup '$OUT'"   # без .timeout можливий 'database is locked'
sqlite3 "$OUT" "PRAGMA quick_check;"              # очікувано: ok
```

Якщо `sqlite3` немає, а `offline.db` існує — скрипт **не** робить `cp`: він завершується
з помилкою й явним повідомленням у лог (`scripts/backup.sh:199-202`).

### 9.5 Що лишається НЕ покритим

- Бекап черги є, але окремого timer'а/юніта для неї немає: вона їде в тому ж повному циклі,
  що й PG (`scripts/systemd/torgashka-backup.timer`). Наслідок: `--meta-only`-запуск
  (`torgashka-meta-backup.timer`, 2х/день) чергу не бекапить — так задумано
  (`scripts/backup.sh:298-303`).
- Перевірки цілісності **всередині застосунку** немає: grep `integrity_check|VACUUM` по
  `frontend/src-tauri/**/*.rs` → 0 збігів. Цілісність перевіряє лише скрипт — `quick_check`
  на копії (`scripts/backup.sh:229`).
- Шифрування черги (SQLCipher) і робота з ключем — поза цим контуром
  (`docs/design/sync-schema-design.md:660`); якщо черга стане шифрованою, `.backup` вимагатиме ключа.
- `get_db_size()` (`offline/db.rs:487-493`) дає лише розмір файлу — діагностика, не цілісність.

### 9.6 Перевірка цілісності та `pending` після відновлення

1. Перевірка знімка (рекомендація, не в коді):

```bash
sqlite3 "$OUT" "PRAGMA integrity_check;"                                  # очікувано: ok
sqlite3 "$OUT" "SELECT status, COUNT(*) FROM outbox GROUP BY status;"      # pending/done/failed
sqlite3 "$OUT" "SELECT COUNT(*) FROM outbox WHERE status IN ('pending','failed');"
```

2. Відновлення — штатним скриптом (Фаза 3.9):
   `scripts/backup-restore.sh --queue <бекап> --yes` (`scripts/backup-restore.sh:95-186`):
   - перевіряє джерело ДО змін: `quick_check` + наявність таблиці `outbox` (`:111-121`);
   - зберігає ПОТОЧНУ чергу в `<offline.db>.pre-restore_YYYYMMDD_HHMMSS` консистентним
     `.backup` (секунди в імені — повторний restore тієї ж хвилини не затирає попередню копію);
   - **відмовляє**, якщо поруч непорожні `offline.db-wal`/`-shm` — каса, схоже, ще працює
     (`:125-129`); свідомий обхід — `--force-live`;
   - підміняє файл атомарно і перевіряє `quick_check` (`:158-180`);
   - без `--yes` питає інтерактивно.
   Після відновлення застосунок відкриє чергу через `sync_push::open_connection`
   (`sync_push.rs:728-737`) і застосує міграції — вручну нічого робити не треба.
3. `pending` після відновлення: фоновий push (`src-tauri/src/lib.rs:347`,
   `offline/commands.rs:123-140`) або ручний `sync_now` (`offline/commands.rs:489-556`) вибере
   чергу FIFO (`sync_push.rs:252-280`) і надішле батчами ≤ 50 (`sync_push.rs:252`,
   `torgashka-api/src/sync.rs:441`).
4. **Повторний push ідемпотентний за `client_uuid`** (`outbox.client_uuid UNIQUE` —
   `0002_sync_meta.sql:14`): сервер відповідає `already_exists`
   (`sync.rs:428, :613, :725, :845, :971, :1104`; partial UNIQUE на primary: `receipts` —
   Alembic 0013, `invoices` — 0016, ADR-0007 §9), клієнт переводить агрегат у `done`
   **без другого stock-ефекту** (`sync_push.rs:564-567`).
5. Легасі-рядки `synced=0` (створені старішою версією без outbox) після відновлення
   підмітаються в чергу при першому ж циклі: `transactions.rs:467` `sweep_legacy_unsynced`,
   виклик — `sync_push.rs:483`; ідемпотентно (`INSERT OR IGNORE` за `client_uuid`).

### 9.7 Чого НЕ робити

- **Не відновлювати чергу «поверх» уже синхронізованих агрегатів без перевірки `synced`-стану.**
  - `synced=1` на локальному агрегаті **НЕ означає «доставлено на primary»**: черговий чек
    пишеться як `INSERT INTO receipts (data, store_id, synced, client_uuid) VALUES (…, 1, …)`
    **в одній транзакції** з `INSERT INTO outbox (…, 'pending')` (`sync_push.rs:110-130`) —
    тобто `synced=1` = «вже в черзі», а стан доставки живе в `outbox.status`.
  - Отже знімок, зроблений «після ack», але відновлений «пізніше», може повернути в роботу
    документи, чиї `outbox`-записи вже `done`, і затерти новіші локальні дані каси.
  - Правильний порядок: спершу — скільки `pending`/`failed` у **поточному** файлі
    (`sqlite3 … GROUP BY status`), потім рішення; за сумніву — не підміняти файл цілком.
  - Повторний push такого агрегата **не** створить дубля залишку/боргу (ідемпотентність
    `client_uuid`, 9.6.4) — але локальний `stock`/агрегати будуть зі старого знімка.
- Не робити `DROP`/перезапис `offline.db` при живому застосунку.
- Не виконувати DR-операції (`promote`) з непорожньою чергою без бекапу (див. 9.3).
