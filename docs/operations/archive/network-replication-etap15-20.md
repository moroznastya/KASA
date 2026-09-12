> [!WARNING]
> **⛔ ЗАСТАРІЛО — АРХІВ (2026-09-12).** Документ описує модель **read-only вузла /
> фізичної реплікації PostgreSQL** (`primary` → hot-standby, `pg_basebackup`,
> WAL-стрімінг, promote). Цю модель **демонтовано** рішенням Творця,
> зафіксованим у [ADR-0008 «Рівноправні read-write вузли + центральний хаб»](../../adr/ADR-0008-peer-nodes-sync-hub.md)
> і виконаним кроком **E7** плану [`plan-adr0008-peer-nodes.md`](../../architecture/plan-adr0008-peer-nodes.md)
> (коміт `50eb8ec`).
>
> **Актуальна модель: [docs/operations/hub-and-nodes.md](../hub-and-nodes.md).**
>
> Збережено як **історичний запис** (рішення, процедури, факти реальних прогонів) — не видаляти.
> **НЕ керуватися цим документом.** Згадані тут шляхи та роути у коді БІЛЬШЕ НЕ ІСНУЮТЬ:
> `/api/v1/network-nodes/join`, `/api/v1/network-nodes/:id/heartbeat`,
> `/api/v1/admin/network-nodes/:id/force-resync`, `/api/v1/local/promote`,
> `/api/v1/local/repoint-primary`, локальна embedded-PG репліка на порту 5433,
> `pg_basebackup`-провіжн, ґейт запису `write_gate`/`readonly_net`.

---
> [!NOTE]
> Це був **план реалізації ЕТАПів 15–20** (фізична реплікація + реєстр вузлів).
> Частина механізмів пережила E7 і працює в новій моделі **за іншим призначенням**:
> `network_nodes` лишився як реєстр вузлів/подій (`/api/v1/admin/network-nodes`,
> `list_network_events`), а `debtors`/`work_sessions`/`prro_shifts` — як push-kinds
> етапу E1 замість реплікації. Перевіряти в коді, не за цим текстом.

---
# План реалізації: мережа магазинів з повною копією БД на кожному вузлі

**Статус:** ДО ЗАТВЕРДЖЕННЯ (версія для аудиту, передана Творцем 2026-09-09).
> ⚠️ Виправлення 2026-09-09 (NIKO): попередня позначка «ЗАТВЕРДЖЕНО до реалізації (рішення
> Творця від 2026-09-08)» була ПОМИЛКОВОЮ — проставлена NIKO без рішення Творця.
> Творець підтвердив: план «до затвердження». Рев'ю-секції §17 і лог §18 лишаються
> як довідковий матеріал NIKO, але НЕ є затвердженням плану.
**Продовжує нумерацію:** `database-architecture-implementation-plan.md` (ЕТАП 1–14) → цей документ вводить **ЕТАП 15–20**.
**Не скасовує й не замінює:** ЕТАП 7 (реальний RLS) і ЕТАП 8 (ідемпотентність чеків) із зазначеного документа — вони лишаються незалежним пріоритетом.

---

## 0. Контекст і мета

Власник хоче, щоб мережу магазинів (спільна БД) було легко й зручно налаштовувати, і щоб **копія бази даних зберігалась на кожному вузлі мережі** — вузлами є комп'ютери в магазинах і один сервер. Мета — максимально ефективний, зручний і надійний спосіб реалізації.

Перевірка коду (`network.rs`, `provision.rs`, `stores.rs`, `StoresPage.tsx`, `StoreDetailPage.tsx`) підтвердила: наразі такого функціоналу немає **ні в робочому, ні в заглушковому вигляді** — існує лише (а) активація кас на точку (`network.rs`, повністю робоче) і (б) перемикання на **незалежну** нову БД («Джерело даних» + `provision.rs`, теж робоче, але не реплікація). Тобто це нова розробка з нуля, а не доробка існуючого.

## 1. Архітектурне рішення (TL;DR)

`sources/database-architecture-decision.md` вже свідомо відхилив мультимайстер/CRDT (варіанти A5/A6) — конфлікти на рівні СУБД не знають бізнес-логіки (фіскальна послідовність, залишки). Це рішення **не переглядається**.

Розв'язання без порушення цього рішення: розділити вісь **«хто пише»** (лишається один автор — сервер/primary, як і зараз) від осі **«у кого лежить копія»** (це і є новий шар). Технічно — нативна **фізична стрімінг-реплікація PostgreSQL** (`pg_basebackup` + WAL streaming, hot standby), вбудована в PostgreSQL 15–17, без розширень (pglogical/Citus не потрібні). Standby — точна копія кластера, read-only, оновлюється в реальному часі по LAN. Запис і далі йде єдиним шляхом через primary; конфліктів класу A5 немає структурно (stock має PK `(store_id, product_id)` — кожен магазин пише лише у свій рядок).

## 2. Топологія й ролі вузлів

| Роль | Де | Що робить | Може писати? |
|---|---|---|---|
| **primary** | Сервер (як і зараз) | Приймає всі запити на запис, джерело істини | Так, єдиний |
| **standby** | Комп'ютер магазину | Повна копія через WAL-streaming, обслуговує локальні читання | Ні (тільки читання; нові документи буферизуються в наявній SQLite-черзі до відправки на primary) |

Магазин не отримує нового "виду" вузла для кожної каси — **один комп'ютер магазину = один вузол-standby**; інші каси в тому ж магазині лишаються тонкими клієнтами й ходять до **локального** фасаду цього вузла по LAN замість походу через WAN/VPN до сервера (сама ідея вже існує у "Варіанті B — Серверний" `multi-store-and-cash-operations.md`, тут вона просто рекурсивно застосована на рівень нижче).

## 3. Модель даних

### 3.1 Нова таблиця `network_nodes`

```sql
CREATE TYPE public.node_role AS ENUM ('primary', 'standby');
CREATE TYPE public.node_status AS ENUM (
    'provisioning',  -- створено запис, токен видано, вузол ще не озвався
    'syncing',       -- pg_basebackup/WAL-catchup у процесі
    'active',        -- в синку, лаг у межах норми
    'lagging',       -- в синку, але лаг > порогу (попередження, не помилка)
    'offline',       -- heartbeat не приходив > 5 хв
    'archived'       -- вузол виведено з мережі, слот реплікації видалено
);

CREATE TABLE public.network_nodes (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    store_id                UUID REFERENCES public.stores(id),  -- NULL для самого сервера (primary)
    name                    TEXT NOT NULL,
    role                    public.node_role   NOT NULL DEFAULT 'standby',
    status                  public.node_status NOT NULL DEFAULT 'provisioning',

    -- Провіжинінг (одноразовий код, аналог store_activation_codes)
    join_token_hash         TEXT,               -- SHA-256, як device_token_hash
    join_token_expires_at   TIMESTAMP,          -- TTL 30 хв

    -- Довготривалий токен вузла для heartbeat (окремо від JWT користувача)
    node_token_hash         TEXT,

    -- Реплікація
    replication_role_name   TEXT,               -- напр. replicator_a1b2c3
    replication_slot_name   TEXT,

    -- Телеметрія (оновлюється heartbeat-ом)
    host                    TEXT,
    app_version             TEXT,
    last_seen_at            TIMESTAMP,
    replication_lag_bytes   BIGINT,
    db_size_bytes           BIGINT,

    created_at              TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at              TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE UNIQUE INDEX network_nodes_replication_slot_uq
    ON public.network_nodes (replication_slot_name) WHERE replication_slot_name IS NOT NULL;

CREATE INDEX network_nodes_store_id_idx ON public.network_nodes (store_id);
```

Аудит окремої таблиці не потребує — використовуємо наявний `audit_log` (entity_type='network_node'), як уже робить `network.rs` для пристроїв.

### 3.2 Зміни в наявних таблицях

Жодних. `stores`, `devices`, `store_activation_codes` лишаються без змін.

## 4. Стан-машина вузла

```
provisioning ──join OK──▶ syncing ──basebackup done + lag<поріг──▶ active
                                                                      │  лаг > поріг
                                                                      ▼
                                                                  lagging ──лаг знову < поріг──▶ active
active/lagging ──5 хв без heartbeat──▶ offline ──heartbeat повернувся──▶ active/lagging
будь-який стан ──owner: "Архівувати"──▶ archived (слот реплікації DROP)
```

Перехід `offline → active` відбувається автоматично, як тільки WAL-стрімінг наздогнав (реплікація сама продовжує з місця розриву — це і є головна перевага фізичної реплікації над самописною чергою). Перехід `→ archived` — лише вручну, ніколи автоматично.

## 5. API-шар (сервер / фасад primary)

Новий модуль `network_nodes.rs`, точно за стилем `network.rs`: той самий `NodeErr` (копія `NetworkErr`), той самий `audit()`, той самий rate-limit на публічному ендпоінті.

| Метод | Шлях | Хто | Призначення |
|---|---|---|---|
| `POST` | `/api/v1/admin/network-nodes` | owner | Створити запис вузла + видати join-код |
| `POST` | `/api/v1/network-nodes/join` | публічний | Новий комп'ютер представляється кодом |
| `PUT` | `/api/v1/network-nodes/:id/heartbeat` | node_token | Періодичний стан вузла |
| `GET` | `/api/v1/admin/network-nodes` | admin/owner | Список вузлів зі статусами |
| `POST` | `/api/v1/admin/network-nodes/:id/archive` | owner | Вивести вузол з мережі |
| `POST` | `/api/v1/admin/network-nodes/:id/force-resync` | owner | Позначити на повторний basebackup |

### 5.1 `POST /api/v1/admin/network-nodes`

Запит: `{ "name": "...", "store_id": "‹uuid або null›" }`
Відповідь `201`: `{ "id", "join_code", "join_code_expires_at", "primary_host_hint" }`

Код — 8 символів з того самого алфавіту `CODE_ALPHABET` (без 0/O/1/I), TTL 30 хв, одноразовий (`join_token_hash` обнуляється після успішного `join`). Помилки: `403` не-owner, `404` якщо вказаний `store_id` не існує.

### 5.2 `POST /api/v1/network-nodes/join` (публічний, як `/devices/activate`)

Той самий rate-limit (5 невдалих/60с на IP), той самий підхід "секрет повертається один раз".

Запит: `{ "join_code", "node_fingerprint", "requested_name" }`
Відповідь `200`: `{ "node_id", "node_token", "replication": { "role", "password", "primary_host", "primary_port", "primary_database", "slot_name" } }`

Помилки: `404` невірний/протермінований код, `409` код вже використано, `429` rate-limit.

Після цього виклику локальний застосунок **сам** запускає провіжинінг (розділ 7) — сервер лише видав креденшли, фізичну реплікацію ініціює клієнт.

### 5.3 `PUT /api/v1/network-nodes/:id/heartbeat`

Авторизація — `Bearer <node_token>` (перевірка через `node_token_hash`, окремо від JWT користувача — вузол не є користувачем). Оновлює `last_seen_at=now()`, перераховує `status` за `lag_bytes` (> порогу, дефолт 50 МБ → `lagging`).

### 5.4 Фоновий job (tokio interval, кожні 60 с)

```sql
UPDATE network_nodes
SET status = 'offline'
WHERE status IN ('active','lagging')
  AND last_seen_at < now() - interval '5 minutes';
```

Поріг 5 хв — свідомо та сама, що вже використовується для `isDeviceOnline` кас.

## 6. Локальний адмін-шар (фасад на самому вузлі)

| Метод | Шлях | Де виконується | Призначення |
|---|---|---|---|
| `POST` | `/api/v1/local/promote` | на вузлі-standby, локально | `pg_promote()` + перехід role: standby→primary |
| `POST` | `/api/v1/local/repoint-primary` | на вузлі-standby, локально | Переналаштувати `primary_conninfo` на новий primary |

Авторизація — той самий JWT, `role=owner`. Працює навіть офлайн: JWT stateless (перевірка підписом без БД), `users`/`user_stores` репліковані на standby.

## 7. Провіжинінг standby: покроковий алгоритм (клієнт, після `join`)

1. Перевірити наявність бінарників PostgreSQL локально (перевикористати `embedded_pg.rs`).
2. Позначити вузол `syncing` (heartbeat).
3. Зупинити (якщо запущено) локальний Postgres, очистити/створити порожню директорію даних.
4. `pg_basebackup -h <primary> -p <port> -U replicator_xxx -D <dir> -X stream -C -S standby_xxx -R`
   (`-C -S` — створює replication slot на primary; `-R` — пише `standby.signal` + `primary_conninfo`).
5. Записати пароль реплікації **зашифрованим на диску** (AES-256-GCM/`.dbkey`-підхід).
6. Запустити локальний Postgres → `hot_standby`.
7. Дочекатися `pg_is_in_recovery()=true` і catchup → heartbeat `active`.
8. Локальний фасад стартує проти локального standby як джерела читання (розділ 9).

Крок 4 — **інший механізм**, ніж `provision_database()` у `provision.rs`. Для standby схема НЕ реплеїться — копіюється весь фізичний каталог. Писати окремий `standby_provision.rs`.

## 8. Безпека і креденшли

- Роль реплікації — **окрема на кожен вузол** (`replicator_<short_id>`), член групової ролі `torgashka_replicators` (`NOLOGIN`).
- `pg_hba.conf` на primary: `host replication +torgashka_replicators  <LAN-підмережа> scram-sha-256`.
- При архівації — `DROP ROLE replicator_xxx;` + `pg_drop_replication_slot(...)` (звільняє WAL).
- `join_token_hash`/`node_token_hash` — SHA-256, оригінали повертаються один раз.
- TLS/VPN: не обов'язково в довіреній LAN; обов'язково поза нею (Tailscale/ZeroTier з `deploy/network/vpn-setup.sh`).

## 9. Локальний роутинг читання/запису на вузлі

```toml
[node]
mode = "standby"           # "primary" | "standby"
local_read_pool = "..."    # локальний standby, для GET-запитів
upstream_write_url = "..." # primary, для POST/PUT/DELETE
```

`GET` → `local_read_pool`; `POST/PUT/DELETE` → `upstream_write_url`; недоступний upstream → вже наявна SQLite-черга офлайн-режиму.

## 10. Взаємодія з офлайн-чергою (SQLite)

Не чіпаємо. Два незалежні шари: SQLite-черга ("як не втратити новий чек") і standby-реплікація ("повна актуальна копія для читання").

## 11. Frontend

### 11.1 `network/NetworkTopologyPage.tsx` (новий)
Список вузлів, картка: назва, роль, статус (кольоровий бейдж — палітра `online/offline` з `StoreDetailPage`), лаг, розмір БД, останній heartbeat. Кнопки: «Додати вузол» (модалка → код + `primary_host_hint`), «Архівувати», «Примусовий ресинк», на standby — «Зробити головним» (`ConfirmDialog`, danger).

### 11.2 Join-екран першого запуску
Два шляхи: «Це новий магазин» (наявний флоу) або «Приєднати як вузол мережі» — код + адреса сервера, прогрес-бар `syncing`.

### 11.3 `networkNodeService.ts` + типи
`list/create/archive/forceResync` через `/admin/network-nodes`.

## 12. Disaster recovery runbook (сервер недоступний)

1. Власник фізично йде до магазину → локальний застосунок.
2. `Налаштування → Мережа → «Зробити цей комп'ютер головним»` → підтвердження з явним попередженням.
3. Локально: `pg_promote()`, role → primary.
4. На кожному ІНШОМУ вузлі вручну «Вказати новий головний вузол» → новий `pg_basebackup` (стара позиція невалідна).
5. Старий сервер при поверненні — приєднується як **новий** standby через join-код (ніколи автоматично — захист від split-brain).

## 13. WAL retention і примусовий ресинк

- `max_slot_wal_keep_size = 10GB` на primary.
- Вузол `offline` > 7 днів → `force-resync required`; heartbeat зі старою позицією відхиляється.
- Кнопка «Примусовий ресинк» = розділ 7 з кроку 3.

## 14. Поетапний план (ЕТАП 15–20)

| ЕТАП | Зміст | Артефакти | Критерії приймання |
|---|---|---|---|
| **15** | Реєстр вузлів + join/heartbeat API (без реальної реплікації) | DDL `network_nodes`; `network_nodes.rs`; фоновий offline-job | Новий вузол реєструється кодом, з'являється в списку, статус переходить `provisioning→syncing` вручну (мок) |
| **16** | `standby_provision.rs`: реальний `pg_basebackup` + запуск standby | Новий модуль; інтеграція з `embedded_pg.rs` | Вимкнути мережу на 30 с під час синку → продовжує з місця розриву; `active` після наздоганяння |
| **17** | UI: `NetworkTopologyPage.tsx` + join-екран першого запуску | Нова сторінка, сервіс, типи | Власник додає магазин-вузол за 3 кліки, без ручного редагування конфіг-файлів |
| **18** | Локальний роутинг читання/запису у фасаді | Блок `[node]` у конфізі, routing-шар | Вимкнути primary → вузол показує каталог/залишки, нові чеки — у SQLite-чергу, не втрачаються |
| **19** | Promote + repoint-primary + runbook | Локальні ендпоінти §6; документація | Ручний promote спрацьовує; старий primary при поверненні не підключається сам |
| **20** | WAL-політика + примусовий ресинк | `max_slot_wal_keep_size`; кнопка ресинку; job 7-денного офлайну | Вузол офлайн 2 тижні не забиває диск сервера; ресинк повертає його в мережу |

## 15. Ризики і мітигації

| Ризик | Мітигація |
|---|---|
| Реплікація ≠ бекап: помилкове видалення розповсюдиться | ЕТАП 11/14 (`pg_dump`/`pg_basebackup` за розкладом) — обов'язкові окремо |
| Split-brain після невдалого promote | Ре-приєднання старого primary — лише вручну через join-код |
| Велика БД → довгий basebackup | Прогрес-бар; USB-носій для первинного basebackup (не MVP) |
| Слабка LAN/Wi-Fi | Поріг `lagging` конфігурований; UI показує лаг |

## 16. Що свідомо НЕ входить у цей план

- Автоматичний failover (тільки ручний, "стабільність понад магію").
- Мультимайстер/незалежний запис з кількох вузлів — відхилено раніше.
- WAN/хмарний сценарій поза LAN/VPN — поза MVP.
- Автоматичне re-pointing після promote — у MVP вручну.

---

## 17. Рев'ю коду — відповіді на відкриті питання (2026-09-08, NIKO)

Звірено з фактичним кодом репозиторію. Усі три питання закрито — план здійсненний без змін архітектури.

### 17.1 `embedded_pg.rs` — логіка запуску/бінарників для standby: **ТАК, перевикористати**
`torgashka-infrastructure/src/embedded_pg.rs` має `PostgresManager`:
- `locate()` — пошук бінарників PG у 5 місцях: env `TORGASHKA_PG_DIR` → Tauri-ресурси (`resources/postgres/bin`, з fallback `../resources`) → `.cache/pg` (dev) → `pg_config --bindir` → `/usr/lib/postgresql/{17..13}/bin`.
- `ensure_initialized()` — `initdb -D <dir> -U <user> -A trust --locale=C --encoding=UTF8`, ідемпотентний (PG_VERSION).
- `start()` — `pg_ctl`, фіксований порт **5433**, poll-готовність 30 с, crash-recovery (чистка postmaster.pid, до 2 спроб).
- `database_url()`, `stop()` (припускаємо — далі у файлі).

**Висновок для ЕТАП 16:** механізм знаходження бінарників + запуску процесу перевикористовується 1:1. Нового потребує лише: (а) виклик `pg_basebackup` (його немає — писати), (б) підтримка конфігурації `hot_standby`/`primary_conninfo` (standby.signal). Застереження: порт 5433 фіксований — вузол НЕ може одночасно бути незалежною БД (Варіант A, `provision.rs`) і standby; режими взаємовиключні на рівні конфігурації `[node] mode`.

### 17.2 Шифрування `.dbkey` для пароля реплікації: **ТАК, той самий підхід**
`torgashka-infrastructure/src/db_sources.rs`:
- AES-256-GCM; формат збереження `base64(nonce(12) || tag(16) || ciphertext)` у полі `password_encrypted`.
- Ключ 32 байти: env `TORGASHKA_DBKEY` → інакше файл `.dbkey` поруч із `db_sources.toml` (права 0600).
- Публічні функції: `resolve_key(cfg_path)`, генерація `.dbkey` при записі.

**Висновок:** ЕТАП 16 використовує `resolve_key` + ті самі encrypt/decrypt для `primary_conninfo`-пароля. Новий механізм не потрібен.

### 17.3 Офлайн-перевірка JWT для локального promote (§6): **ТАК, працює повністю локально**
`torgashka-api/src/auth.rs`:
- JWT = HS256, `Claims { sub, role, permissions, token_type, iat, exp }`.
- `decode_token`/`validate_jwt` — локальна перевірка підпису секретом, **без жодного запиту в БД**.
- `require_admin`/`require_owner` беруть роль з JWT-claims, БЕЗ БД (підтверджено коментарем у `network.rs`: "роль береться з JWT, БЕЗ запиту в БД").
- `PUBLIC_PATHS` — статичний список; новий публічний `/api/v1/network-nodes/join` додається туди (як `/api/v1/devices/activate`).

**Висновок:** §6 (локальний `promote` з JWT owner) життєздатний навіть офлайн. Єдина умова — локальний вузол має мати той самий `jwt_secret`, що й primary (перевірити, як секрет поширюється на вузли — відкрите питання на ЕТАП 19, за замовчуванням: секрет у локальному конфізі вузла, що створюється при join).

### 17.4 Додатковий факт — механізм DDL (уточнення розділу 3.1)

У репозиторії **немає** нумерованих SQL-міграцій (дир. `frontend/src-tauri/migrations/` порожня; вона не використовується). Схема влаштована так:
- `torgashka-infrastructure/src/schema.sql` (`SCHEMA_SQL`) — повна схема для СВІЖИХ БД (`provision.rs`/`setup.rs`).
- `torgashka-infrastructure/src/db.rs` — ідемпотентні DDL-константи (`OWNERS_DB_DDL`, `CASH_OPS_DDL`, `NETWORK_DDL`: `CREATE TABLE IF NOT EXISTS` + `ADD COLUMN IF NOT EXISTS`), які **виконуються ЗАВЖДИ при старті** — це і є "міграційний шлях" для вже наявних БД.

**Висновок для ЕТАП 15:** `network_nodes` додається ДВОМА місцями: (а) у `schema.sql` (свіжі БД) і (б) новою ідемпотентною константою `NETWORK_NODES_DDL` у `db.rs` + її виклик у стартовій послідовності (поряд із `NETWORK_DDL`) — інакше наявні primary-БД не отримають таблицю.

---

## 18. Хід реалізації (лог)

| Дата | ЕТАП | Статус | Ким | Результат |
|---|---|---|---|---|
| 2026-09-08 | Рев'ю §17 | ✅ | NIKO | Відповіді вище; план підтверджено |
| 2026-09-08 | 15 | ✅ | Rust_Agent + NIKO (рев'ю) DDL network_nodes (schema.sql + db.rs NETWORK_NODES_DDL в ensure_schema); network_nodes.rs (811 р.): create/join/heartbeat/list/archive/force-resync + фоновий offline-job 60с; роути router_v1; PUBLIC_PATHS. cargo check --workspace ✅, юніт-тести 5/5 ✅. target/ почищено (cargo clean, +22.4G) |
| 2026-09-09 | 7 | ✅ | NIKO (SQL) | FORCE ROW LEVEL SECURITY на 28 таблицях: schema.sql (fresh) + db.rs RLS_FORCE_DDL в ensure_schema (наявні БД). Реальний захист фасаду під torgashka_app (NOSUPERUSER NOBYPASSRLS). Суперюзер postgres обходить — адмін-операції безпечні |
| 2026-09-09 | 8 | ✅ | NIKO (SQL) | Ідемпотентність чеків: client_uuid (ADD COLUMN IF NOT EXISTS) + UNIQUE-індекс uq_receipts_client_uuid (WHERE NOT NULL): schema.sql + db.rs RECEIPTS_CLIENT_UUID_DDL в ensure_schema |
| 2026-09-09 | 16 | ✅ | Rust_Agent | crates/torgashka-infrastructure/src/standby_provision.rs (31KB, підключено в lib.rs): pg_basebackup -X stream -C -S -R через бінарники embedded_pg; AES-256-GCM пароль реплікації (db_sources-підхід); запуск hot_standby на 5433; перевірка pg_is_in_recovery(). cargo check --workspace ✅ |
| 2026-09-09 | 17 | ✅ | NIKO (React_UI_UX_Agent звіт «виконано» БЕЗ результату — аномалія, зроблено самотужки) | networkNodeService.ts (типи+CRUD+join+heartbeat); NetworkTopologyPage.tsx (список вузлів, статус-бейджі, модалка join-коду, архів/ресинк); NodeJoinPage.tsx (публічний /node-join, збереження node_* у SQLite settings/localStorage); маршрути + меню (Sidebar, AdminShell); formatBytes у utils. tsc --noEmit ✅ |
| 2026-09-09 | 18 | ✅ | Rust_Agent + NIKO (рев'ю) | node_config.rs (NodeMode Primary/Standby, [node] у db_sources.toml, primary_reachable TCP-чек, local_db_url); route_local.rs (/api/v1/local/*: читання каталогу/залишків/чеків з локальної репліки 5433, запис чека в SQLite-чергу, effective_status); router_v1 merge. Unit-тести node_config 8/8 ✅ |
| 2026-09-09 | 19 | ✅ | Rust_Agent + NIKO (рев'ю) | promote.rs (POST /api/v1/local/promote — pg_promote + перехід standby→primary + очистка replication creds, анти-split-brain; POST /api/v1/local/repoint-primary); монтування ОКРЕМО від store-middleware (працює при недоступному primary); docs/operations/disaster-recovery-network.md (runbook). Тести 5/5 ✅ |
| 2026-09-09 | 20 | ✅ | Rust_Agent + NIKO (рев'ю) | WAL_POLICY_DDL в ensure_schema (ALTER SYSTEM max_slot_wal_keep_size=10GB + pg_reload_conf, graceful fallback якщо не superuser); heartbeat_node відхиляє застарілі standby (offline>7днів → 410 force-resync required, крім status=syncing). Тести network_nodes 8/8 ✅ |
| 2026-09-09 | Дод. | ✅ | Rust_Agent + NIKO (рев'ю) | Детальне логування (рішення Творця): DDL network_events (db.rs + schema.sql); log_node_event (побічний, не валить основну операцію) у create/join/heartbeat(status_change, reject_stale)/archive/force-resync/promote/repoint/degraded_local; GET /api/v1/admin/network-events (node_id/event/limit). cargo test 65/65 ✅ |
| 2026-09-09 | — | 🔧 | NIKO | Виправлено статус: «ЗАТВЕРДЖЕНО» → «ДО ЗАТВЕРДЖЕННЯ» (позначка була помилковою). Рев'ю §17 звірено з кодом повторно — підтверджено (embedded_pg 5433/locate, AES-256-GCM .dbkey, локальний JWT) |
