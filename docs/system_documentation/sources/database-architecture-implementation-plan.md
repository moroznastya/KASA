# План впровадження рекомендацій: архітектура БД Torgashka POS

> Статус: ЗАТВЕРДЖЕНО ДО ВИКОНАННЯ
> Дата: 2026-09-01
> Джерело: `Projects/database-architecture-decision.md` (аналіз + рекомендація)
> Репозиторій: `Projects/kasa`, гілка `feat/rust-migration`
> Стек: Rust-фасад (torgashka-api/application/domain/infrastructure) + React (Tauri) + PostgreSQL 15-17 + SQLite (офлайн) + Alembic

---

## 0. Контекст і обмеження (від Творця)

1. **БД — окрема для кожного власника** (database-per-tenant, вісь B3). Вже обрано й частково реалізовано: `owners_db`, `torgashka_template`, `torgashka_owner_<id>`. Треба **добудувати маршрутизацію**.
2. **Глобальної інтернет-версії НЕ буде.** Програма розгортається як **локальна мережа**: у кожного власника — своя локальна мережа магазинів (одне приміщення або LAN-зв'язок між точками власника).
3. **Наслідки для архітектури:**
   - Сценарій 3 (хмарний VPS + TLS + PgBouncer + read-репліка) з документа-джерела — **виключений з MVP**. Замість нього — «LAN-розгортання» (сценарій 2) як цільовий і єдиний.
   - TLS-шифрування каналу: не критичне всередині довіреної LAN (сценарій 2), обов'язкове лише якщо власник об'єднує точки через інтернет (VPN/WireGuard) — опційно, поза скоупом MVP.
   - PgBouncer: не потрібен для 1-10 кас на один LAN-сервер (ліміт `max_connections=100` Postgres покриває). Потрібен лише пул пулів для N власників на одному сервері — і то з LRU-кешем.
4. **Пріоритети (з документа):** 🔴 ЕТАП 7 (RLS) і ЕТАП 8 (ідемпотентність) — критичні, робляться першими. 🟠 ЕТАП 9 (маршрутизація), ЕТАП 10 (надійність синку). 🟡 ЕТАП 11 (бекапи), ЕТАП 13 (SQLCipher).

---

## 1. Цільова архітектура (після впровадження)

```
┌─ МЕТА-РІВЕНЬ (одна БД pos_system на сервері власника) ─────────────┐
│  users (логін), owners_db (owner_id → db_name), auth/setup         │
│  НЕ містить бізнес-даних                                           │
└─────────────────────────────────────────────────────────────────────┘
        │ маршрутизація за owner_id (JWT → owners_db → пул БД)
        ▼
┌─ БД ВЛАСНИКА (torgashka_owner_<id>) ───────────────────────────────┐
│  stores, user_stores, stock, products, receipts, ...               │
│  + RLS по store_id, FORCE RLS, роль torgashka_app                  │
└─────────────────────────────────────────────────────────────────────┘
        ▲ синхронізація (TLS опційно, LAN)
        │
┌─ КАСИ (Tauri + Rust-фасад + SQLite offline.db) ────────────────────┐
│  receipts → черга з client_receipt_uuid (ідемпотентність)          │
│  stock → дельти (атомарні UPDATE), каталог — read-only кеш         │
│  тригер синку: health-check 15-30с + backoff, не подія 'online'    │
└─────────────────────────────────────────────────────────────────────┘
```

**Ключові рішення (підтверджені/змінені):**
- **Вісь A = A3** (offline-first + синхронізація на центральний PostgreSQL) — залишається.
- **Вісь B = B3** (окрема БД на власника) — залишається, доводиться до кінця.
- **Сценарій розгортання = 2** (виділений PostgreSQL на LAN-сервері власника) + сценарій 1 (embedded PG на одному ПК). Сценарій 3 (хмара) — НЕ входить.

---

## 2. Карта робіт: ЕТАПИ, власники, залежності

| ЕТАП | Назва | Пріоритет | Головний виконавець | Залежить від |
|---|---|---|---|---|
| 7 | Реальне забезпечення RLS | 🔴 | DB_Admin_Agent + Rust_Agent + QA_Agent | — |
| 8 | Ідемпотентність синхронізації чеків | 🔴 | Rust_Agent + DB_Admin_Agent + React_UI_UX_Agent + QA_Agent | — |
| 9 | Маршрутизація до БД власника | 🟠 | Rust_Agent + DB_Admin_Agent + QA_Agent | 7 (роль/пул) |
| 10 | Надійність офлайн-синхронізації | 🟠 | React_UI_UX_Agent + Rust_Agent + QA_Agent | 8 |
| 11 | Резервне копіювання (LAN) | 🟡 | Infrastructure_Master_Agent + File Wizard Agent | 9 |
| 13 | Шифрування offline.db (SQLCipher) | 🟡 | Rust_Agent + QA_Agent | 10 |
| 14 | Ланцюг бекапів мета-БД | 🟡 | Infrastructure_Master_Agent | 11 |

ЕТАП 12 (хмарний сценарій) — **скасовано** згідно з обмеженням Творця (немає глобальної версії).

---

## 3. ЕТАП 7 — 🔴 Реальне забезпечення RLS (критичний)

### Проблема (підтверджено кодом)
`setup.rs:23-27`: фасад підключається під `postgres` (власник таблиць) → власник таблиці **обходить RLS** за замовчуванням. Захист store_id зараз — це «списки в WHERE» у коді, а не гарантія БД.

### 3.1 Створення ролі `torgashka_app` — DB_Admin_Agent
**ВХІД:** `frontend/src-tauri/crates/torgashka-infrastructure/src/schema.sql`, `backend/alembic/versions/0004_rls.py`, `setup.rs`.
**ЗАДАЧА:**
- SQL-скрипт `scripts/create_app_role.sql` (ідемпотентний):
  ```sql
  DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'torgashka_app') THEN
      CREATE ROLE torgashka_app LOGIN PASSWORD '<генерується при інсталяції>';
    END IF;
  END $$;
  -- права: CONNECT на всі БД кластера; НЕ власник таблиць; БЕЗ SUPERUSER/BYPASSRLS/CREATEDB
  ```
- Гранти: `GRANT USAGE ON SCHEMA public`, `GRANT SELECT/INSERT/UPDATE/DELETE ON ALL TABLES`, `USAGE ON ALL SEQUENCES` — на БД `pos_system`, `torgashka_template` і кожну `torgashka_owner_*` (через раннер по `owners_db` — див. ЕТАП 9.3).
- Пароль: генерується при інсталяції, зберігається в `config.toml` (chmod 0600), не хардкодиться.
**ВИХІД:** `scripts/create_app_role.sql` + оновлений `config.toml` шаблон (`.env.example`/docs).
**КРИТЕРІЙ:** `psql -U torgashka_app -d pos_system -c "SELECT current_user"` → `torgashka_app`; роль не має `rolbypassrls`/`rolsuper`.

### 3.2 FORCE ROW LEVEL SECURITY — DB_Admin_Agent
**ЗАДАЧА:** міграція `0005_force_rls.py` (Alembic) або доповнення до `schema.sql`:
- `ALTER TABLE <кожна з STORE_TABLES> FORCE ROW LEVEL SECURITY;`
- Те саме для `stores`, `user_stores`.
**ВИХІД:** міграція `backend/alembic/versions/0005_force_rls.py` + синхронне оновлення `frontend/src-tauri/crates/torgashka-infrastructure/src/schema.sql`.
**КРИТЕРІЙ:** `SELECT relforcerowsecurity FROM pg_class WHERE relname='receipts'` → `t`; `alembic upgrade head` на чистій БД і на БД з даними.

### 3.3 Переведення підключень фасаду на `torgashka_app` — Rust_Agent
**ВХІД:** `frontend/src-tauri/crates/torgashka-infrastructure/src/db.rs`, `repositories/setup.rs`, `store_ctx.rs`.
**ЗАДАЧА:**
- У `resolve_database_url`/`PgPoolOptions` — підставляти роль `torgashka_app` з конфігу для всіх підключень (embedded PG, LAN-сервер).
- Для embedded PG (сценарій 1): `initdb` створює роль `torgashka_app` і призначає права.
- **Виняток:** операції, які потребують суперкористувача (CREATE DATABASE для нового власника, CREATE ROLE, pg_dump) — окреме адмін-з'єднання під `postgres`, тільки в setup-сервісі, ніколи в бізнес-шляху.
**ВИХІД:** зміни в `db.rs`/`setup.rs`/`store_ctx.rs` + `cargo build`/`cargo test` зелений.
**КРИТЕРІЙ:** бізнес-запит під `torgashka_app` без `set_config('app.store_id')` → 0 рядків (RLS блокує), а не дані.

### 3.4 Regression-тести — QA_Agent
**ЗАДАЧА:** розширити `store_settings_isolation.rs`/`store_ctx_reset.rs`:
- Негативний тест: `SELECT * FROM receipts` під `torgashka_app` без контексту → 0 рядків **через RLS** (перевірити, що RLS-enabled, а не порожня таблиця).
- Позитивний: з `app.store_id` → тільки свої рядки; owner бачить всі свої точки через `user_stores`.
**ВИХІД:** нові тести + звіт про проходження.
**КРИТЕРІЙ:** `cargo test --workspace` зелений, негативний тест падає ДО фіксу і проходить ПІСЛЯ.

---

## 4. ЕТАП 8 — 🔴 Ідемпотентність синхронізації чеків (критичний)

### Проблема (підтверджено кодом)
`offline/commands.rs` + `useOfflineSync.tsx`: немає ідемпотентного ключа. Обрив зв'язку після успішного запису → повторна відправка → **дублікат чека + подвійне списання залишку**.

### 8.1 `client_receipt_uuid` на клієнті — Rust_Agent
**ВХІД:** `frontend/src-tauri/crates/torgashka-infrastructure/src/offline/commands.rs`, `offline/db.rs`, API-слой `torgashka-api`.
**ЗАДАЧА:**
- `save_receipt_offline`: генерувати `client_receipt_uuid` (UUID v4) одразу при збереженні, до будь-якої мережевої взаємодії.
- Додати колонку в SQLite-схему (міграція в `initialize_tables`/`migrate` — ідемпотентно).
- Ендпоінт прийому чека: `POST /receipts` приймає `client_receipt_uuid`; при `UNIQUE`-конфлікті повертає `200/409` з результатом першого запису (не помилку) — клієнт безпечно робить `mark_receipt_synced`.
**ВИХІД:** змінений Rust-код + `cargo test`.
**КРИТЕРІЙ:** двічі відправлений чек з тим самим UUID → один запис у БД, залишок списаний один раз.

### 8.2 UNIQUE-обмеження на сервері — DB_Admin_Agent
**ЗАДАЧА:** міграція `0006_receipt_idempotency.py`:
- `ALTER TABLE receipts ADD COLUMN client_receipt_uuid UUID;`
- `CREATE UNIQUE INDEX uq_receipts_client_uuid ON receipts(client_receipt_uuid) WHERE client_receipt_uuid IS NOT NULL;`
- Backfill для існуючих чеків (генерація UUID), щоб NOT NULL не ламав старих даних — або partial index, як вище.
**ВИХІД:** міграція + синхронне оновлення `schema.sql`.
**КРИТЕРІЙ:** `alembic upgrade head` без втрати даних; дублікат UUID → `23505 unique_violation`.

### 8.3 Frontend-обробка відповіді — React_UI_UX_Agent
**ВХІД:** `frontend/src/hooks/useOfflineSync.tsx`, `frontend/src/services/tauri/offlineReceiptService.ts`.
**ЗАДАЧА:**
- При `200/201/409` — `mark_receipt_synced`; помилка лише при мережевих збоях/5xx (не при 409).
- Передавати `client_receipt_uuid` у запиті.
**ВИХІД:** змінений TSX/TS + `eslint`/`tsc` чисто.
**КРИТЕРІЙ:** симуляція обриву після запису → повторна спроба не створює дублікат (тест 8.4).

### 8.4 Тест дубліката — QA_Agent
**ЗАДАЧА:** integration-тест: «відправити чек → імітувати обрив (не отримати відповідь) → відправити ще раз» → COUNT(receipts) = 1, stock списано один раз.
**ВИХІД:** тест + звіт.
**КРИТЕРІЙ:** тест зелений.

---

## 5. ЕТАП 9 — 🟠 Маршрутизація до БД власника

### Проблема (підтверджено кодом)
`setup.rs` TODO: БД власника створюється й реєструється в `owners_db`, але бізнес-запити йдуть у мета-БД. Ізоляція власників — заготовка, не гарантія.

### 9.1 Пул пулів з LRU-кешем — Rust_Agent
**ВХІД:** `store_ctx.rs` (StorePool), `db.rs`, JWT-авторизація в `torgashka-api`.
**ЗАДАЧА:**
- `StorePool` → `PoolOfPools`: `HashMap<db_name, PgPool>` + LRU-витіснення, ліміт одночасно відкритих пулів (конфіг: `pool_cache_size`, за замовчуванням 16).
- Маршрутизація: `user_id` з JWT → (кешований) запит до мета-БД `SELECT db_name FROM owners_db WHERE owner_id = (SELECT owner_id FROM users WHERE id=$1)` → пул до `torgashka_owner_<id>`.
- Бізнес-ендпоінти (`/products`, `/receipts`, `/stock`, `/stores`, ...) — через пул БД власника. Мета-БД — тільки `/auth`, `/setup`, `/owners_db`.
- `acquire_timeout` параметризувати (LAN: 5с — ок).
**ВИХІД:** змінений Rust-код + `cargo test`.
**КРИТЕРІЙ:** два власники фізично не мають спільних таблиць; запит власника А не може повернути дані власника Б навіть теоретично.

### 9.2 Міграція `users.owner_id` — DB_Admin_Agent
**ЗАДАЧА:** перевірити/додати `owner_id` у `users` (або зв'язок `users.owner_id`), якщо його немає — міграція `0007_owner_id_in_users.py` + індекс.
**ВИХІД:** міграція.
**КРИТЕРІЙ:** `owner_id` NOT NULL для ролі owner; JOIN users→owners_db працює.

### 9.3 Раннер міграцій по всіх `torgashka_owner_*` — DB_Admin_Agent + Rust_Agent
**ЗАДАЧА:**
- Скрипт `scripts/migrate_all_owners.sh` (або Rust-команда): для кожної БД з `owners_db` → `alembic upgrade head` / `sqlx migrate run` (ідемпотентно).
- Порядок: `torgashka_template` → усі `torgashka_owner_*`.
- Додати в CI/startup: при старті фасаду перевіряти, чи схема актуальна.
**ВИХІД:** скрипт + інтеграція в старт.
**КРИТЕРІЙ:** нова міграція застосовується до всіх БД власників одним запуском.

### 9.4 Тест ізоляції — QA_Agent
**ЗАДАЧА:** integration-тест: власник А створює товар/чек → власник Б не бачить (404/0 рядків); прямий SQL у БД Б не містить рядків А.
**ВИХІД:** тест + звіт.
**КРИТЕРІЙ:** тест зелений; `\dt` у `torgashka_owner_A` ≠ `torgashka_owner_B` за даними.

---

## 6. ЕТАП 10 — 🟠 Надійність офлайн-синхронізації

### Проблема (підтверджено кодом)
`useOfflineSync.tsx:115`: єдиний тригер — браузерна подія `online`. Це false-positive сигнал (Wi-Fi є — сервера нема). Немає backoff при частковій невдачі.

### 10.1 Health-check + backoff — React_UI_UX_Agent
**ВХІД:** `frontend/src/hooks/useOfflineSync.tsx`, `SyncStatus` компонент.
**ЗАДАЧА:**
- Періодичний `GET /api/v1/health` кожні 15-30 с (незалежно від події `online`).
- Плановий повтор несинхронізованих: експоненційний backoff 5с → 15с → 60с → 5хв (з межею), поки `pendingCount > 0`.
- UI: окремий стан «сервер недоступний» від «є несинхронізовані чеки».
**ВИХІД:** змінений useOfflineSync.tsx + SyncStatus.tsx; eslint/tsc чисто.
**КРИТЕРІЙ:** вимкнення Wi-Fi на 5 хв → повторне ввімкнення → чеки синхронізовані автоматично без події `online` і без ручного кліку.

### 10.2 Health-ендпоінт у Rust-фасаді — Rust_Agent
**ЗАДАЧА:** переконатись, що `GET /api/v1/health` доступний з Tauri-клієнта (перевірити CORS/маршрут у `torgashka-api`); додати перевірку досяжності БД власника (SELECT 1) у відповідь.
**ВИХІД:** змінений Rust-код.
**КРИТЕРІЙ:** `curl /api/v1/health` → `{"status":"ok","db":"ok"}`.

### 10.3 `transfers.from/to_store_id` FK — Rust_Agent + DB_Admin_Agent
**ЗАДАЧА:** міграція `0008_transfers_store_fk.py`: `transfers.from_location/to_location` → `from_store_id`/`to_store_id` FK (TODO з власного плану), оновлення Rust-репозиторію transfers.
**ВИХІД:** міграція + Rust-зміни.
**КРИТЕРІЙ:** `alembic upgrade head`; transfer створюється з валідними store_id; каскадне видалення точок заблоковано FK.

### 10.4 Тест відновлення мережі — QA_Agent
**ЗАДАЧА:** тест сценарію «5 хвилин офлайн → онлайн» (mock мережі) — чеки синхронізуються без ручного втручання.
**ВИХІД:** тест + звіт.
**КРИТЕРІЙ:** тест зелений.

---

## 7. ЕТАП 11 — 🟡 Резервне копіювання (LAN-сценарій)

### Проблема
У репозиторії немає жодної backup-стратегії. Для фіскального POS — критичний ризик.

### 11.1 Скрипт бекапу + systemd timer — Infrastructure_Master_Agent
**ВХІД:** docker-compose.yml, скрипти `scripts/`, конфіг БД.
**ЗАДАЧА:**
- `scripts/backup.sh`: `pg_dump` (custom format) для `pos_system` (мета-БД) + кожної `torgashka_owner_*` (список з `owners_db`), ротація (зберігати N днів, конфіг).
- `scripts/backup-restore.sh`: документована й перевірена процедура відновлення (DROP/CREATE DB → pg_restore).
- systemd timer `torgashka-backup.timer` (щоденно, 02:00) + unit; для Docker — cron у контейнері або host-timer.
- Бекап мета-БД — окремо і найчастіше (втрата = фасад не знає, де дані власників).
**ВИХІД:** `scripts/backup.sh`, `scripts/backup-restore.sh`, systemd units, README-інструкція.
**КРИТЕРІЙ:** `backup.sh` створює файли; `backup-restore.sh` на чистому Postgres відновлює дані (перевірено ручним прогоном на тестовій БД).

### 11.2 Тест відновлення — Infrastructure_Master_Agent + File Wizard Agent
**ЗАДАЧА:** задокументувати й виконати тест: бекап → DROP DATABASE → restore → COUNT збігається.
**ВИХІД:** звіт про тест відновлення.
**КРИТЕРІЙ:** відновлена БД ідентична (COUNT по ключових таблицях).

---

## 8. ЕТАП 13 — 🟡 Шифрування offline.db (SQLCipher)

### Проблема
`offline.db` на диску не зашифрована; містить чеки з сумами продажів.

### 13.1 SQLCipher — Rust_Agent
**ЗАДАЧА:**
- Оцінити інтеграцію SQLCipher у Rust (крейт `rusqlite` з `bundled-sqlcipher-vendored-openssl` або `sqlx` з SQLCipher-фічею) — **спочатку spike/прототип**, бо це зміна низькорівневої залежності.
- Ключ шифрування: генерується при першому запуску, зберігається в `config.toml` (0600) або системному keyring; міграція існуючих `offline.db` (spike: чи можна відкрити незашифровану і переписати в SQLCipher).
**ВИХІД:** звіт spike + (якщо життєздатно) реалізація.
**КРИТЕРІЙ:** `offline.db` не читається без ключа (`sqlite3 offline.db` → garbage/error); `cargo test` зелений.
**ПРИМІТКА:** якщо spike покаже несумісність з поточною SQLite-схемою (WAL, PRAGMA) — зафіксувати рішення «відкладено до v-next» з обґрунтуванням, не ламаючи робочу систему (принцип stability_first).

---

## 9. ЕТАП 14 — 🟡 Ланцюг бекапів мета-БД (посилення 11.1)

### 14.1 — Infrastructure_Master_Agent
**ЗАДАЧА:** виділити бекап `pos_system` в окремий таймер з вищою частотою (щоденно мінімум; опційно — двічі на день), бо втрата `owners_db` = втрата маршрутизації до всіх власників.
**ВИХІД:** окремий unit/timer + документація.
**КРИТЕРІЙ:** бекапи мета-БД існують незалежно від бекапів власників.

---

## 10. Порядок виконання та контроль

### Хвилі (короткі цикли)
1. **Хвиля 1 (паралельно):** ЕТАП 7 (RLS) + ЕТАП 8 (ідемпотентність) — незалежні, критичні.
2. **Хвиля 2 (після 7):** ЕТАП 9 (маршрутизація — потребує ролі/пулу з 7) + ЕТАП 10 (після 8, потребує 409-семантики).
3. **Хвиля 3:** ЕТАП 11 + ЕТАП 14 (бекапи) + ЕТАП 13 (spike SQLCipher).

### Контроль (NIKO)
- Кожен ЕТАП — контракт: ЗАДАЧА / ВХІД / ВИХІД / КРИТЕРІЙ (вище).
- Після кожної хвилі: `cargo test --workspace`, `alembic upgrade head` на чистій БД, `eslint`/`tsc`.
- Аномалії — вгору по ієрархії (канал зворотного зв'язку).
- Статус фіксується в цьому документі (чекбокси).

### Ризики
| Ризик | Мітигація |
|---|---|
| FORCE RLS зламає бізнес-шлях (забутий set_config) | Хвиля 1: QA-тести до/після; rollback-план (зняти FORCE) |
| SQLCipher несумісний зі схемою | Spike до реалізації; рішення «відкласти» — допустиме |
| Міграція по N БД власників повільна | Раннер з idempotent + паралельність (xargs -P) |
| Втрата даних при міграції 0006 (backfill UUID) | Partial unique index (NULL дозволений для старих рядків) |

---

## 11. Що НЕ входить (свідомо виключено Творцем)
- ❌ ЕТАП 12: хмарний VPS, PgBouncer, read-репліка, WireGuard — немає глобальної версії.
- ❌ TLS `verify-full` до публічного Postgres — немає публічного Postgres.
- ❌ Мультимайстер/CRDT/P2P — відкинуто документом-джерелом (фіскальна послідовність).
