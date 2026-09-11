# Дизайн схеми синхронізації — Offline-first (ЕТАП 1)

> **Статус:** ПРОЄКТ (узгоджується з ADR-015)
> **Топологія:** 4 каси (SQLite, основне сховище) + сервер-агрегатор (PostgreSQL)
> **Модель:** однонапрямлені потоки — pull майстер-даних (сервер→каса),
> push транзакцій (каса→сервер). Конфліктів немає за конструкцією.
> **Базові артефакти:** `offline/db.rs` (SQLite), multi-store-implementation-plan,
> ADR-015 (рішення + відхилені альтернативи).

---

## 1. Версіонування майстер-даних

### 1.1 Принцип

Сервер — єдине джерело істини для довідників. Кожна зміна довідника
інкрементує глобальний монотонний лічильник **`version`** у таблиці
`sync_meta`. Каса тримає **останню застосовану версію на кожну сутність**
і при pull запитує `since_version` → отримує дельту (rows з `version >
since_version`) → застосовує атомарно → зберігає нову версію.

### 1.2 Формат версій

- **Числова, монотонна, глобальна для сутності:** `BIGINT`, інкремент +1 на
  кожну зміну (upsert або soft-delete). Без timestamp-порівнянь (годинники
  кас/сервера можуть розходитись — це не джерело істини для версій).
- Ключ версії: `entity` (назва довідника) + `version` (BIGINT).
- `version = 0` — початковий стан (порожньо).
- Каса зберігає `since_version` локально в `sync_meta` (SQLite).

Приклад значень:

```
sync_meta(entity='categories',  version=142)
sync_meta(entity='products',    version=3017)
sync_meta(entity='stock_norms', version=88)
sync_meta(entity='suppliers',   version=56)
sync_meta(entity='employees',   version=24)
sync_meta(entity='settings',    version=19)
```

### 1.3 Інкремент версій на сервері

Інкремент відбувається **в тій самій транзакції**, що й зміна даних
(тригер або явний код в Rust-фасаді):

```sql
-- Приклад (Rust-фасад, upsert категорії):
BEGIN;
INSERT INTO categories (id, name, parent_id, is_deleted, updated_at)
VALUES ($1, $2, $3, false, now())
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, parent_id = EXCLUDED.parent_id;
UPDATE sync_meta SET version = version + 1 WHERE entity = 'categories';
COMMIT;
```

Правило: **жодна зміна довідника неможлива без інкременту версії** —
інакше каса ніколи не дізнається про неї.

### 1.4 Pull — протокол

```
GET /api/v1/sync/master?entity=products&since_version=3010&store_id=<uuid>
        (X-Store-Id: <uuid каси>)
```

Відповідь (дельти):

```json
{
  "entity": "products",
  "since": 3010,
  "to": 3017,
  "changes": [
    {
      "op": "upsert",
      "id": "3f2c9e2a-...",
      "version": 3011,
      "data": { "name": "Молоко 2,6%", "barcode": "4820000000000", "category_id": "..." }
    },
    {
      "op": "delete",
      "id": "7b1d4f10-...",
      "version": 3012,
      "data": null
    }
  ]
}
```

Правила:
- `op: "upsert"` — вставка/оновлення рядка (каса робить UPSERT по `id`).
- `op: "delete"` — soft-delete (`is_deleted=true` на сервері): каса позначає
  рядок `is_deleted=1` локально (товар зникає з продажу, історія зберігається).
  Фізичне видалення рядків не використовується (каса могла ще не побачити зміну).
- **Атомарність застосування:** каса застосовує всю дельту в ОДНІЙ SQLite
  транзакції; при помилці — ROLLBACK, версія не просувається, pull повторюється.
- **Стабільність дельти:** версії не перепризначаються; якщо каса вже
  застосувала версію `X`, сервер ніколи не віддасть їй `X` знову з іншими
  даними (append-only семантика версій).

### 1.5 Обмеження розміру дельти

- Максимум `page_size` (за замовчуванням 500 rows) на один pull.
- Відповідь містить `to` (останню віддану версію); якщо змін більше —
  каса повторює pull з `since_version = to`.
- Початковий pull (порожня каса) може зайняти кілька сторінок — це очікувано
  (перший онбординг каси: екран «Очікування синхронізації»).

---

## 2. Формат дельт (payload)

Єдиний формат для pull (сервер→каса) і push (каса→сервер) — **JSON Lines
масив об'єктів** (як у відповіді pull вище). Push використовує той самий
конверт, але `op` фіксований `"insert"` для всіх транзакцій (транзакції не
оновлюються — вони незмінні після створення).

### 2.1 Pull-дельти (майстер-дані)

```json
{
  "entity": "products",
  "since": 3010,
  "to": 3017,
  "changes": [ { "op": "upsert|delete", "id": "uuid", "version": 3011, "data": { ... } } ]
}
```

### 2.2 Push-дельти (транзакції каси)

Кожен елемент outbox — **один агрегат** (транзакція + її ефекти), пакет
відправляється як масив:

```json
[
  {
    "type": "receipt",
    "client_uuid": "9f1c2d3e-4b5a-4c6d-8e7f-1a2b3c4d5e6f",
    "store_id": "a1b2c3d4-...",
    "created_at": "2026-09-10T09:31:05+03:00",
    "payload": {
      "receipt": { "id": null, "number": 1042, "cashier_id": "...", "total": 256.50 },
      "items": [
        { "product_id": "...", "qty": 2, "price": 89.50, "price_snapshot": 89.50,
          "sum": 179.00, "barcode": "4820000000000", "name_snapshot": "Молоко 2,6%" }
      ],
      "effects": {
        "stock_delta": [ { "product_id": "...", "delta": -2 } ],
        "debtor_delta": { "debtor_id": "...", "delta": 0 },
        "fiscal": { "status": "fiscalized", "fiscal_no": "F20260910-000042",
                    "fiscal_dt": "2026-09-10T09:31:07+03:00", "qr": "..." }
      }
    }
  }
]
```

Правила payload:
- `client_uuid` — **обов'язковий**, генерується касою (UUIDv4), унікальний
  назавжди. Служить ідемпотентним ключем push.
- `store_id` — store_id каси (з `settings`), валідується сервером через
  `X-Store-Id` (існуючий StoreCtx).
- `payload.items[].price_snapshot` та `name_snapshot` — знімок на момент
  продажу; сервер НЕ перераховує ціну.
- `payload.effects.stock_delta` — зміна локального stock каси (для агрегації
  на сервері). `delta` від'ємний для продажу/списання, додатний для
  надходження/повернення.
- `payload.effects.fiscal` — результат фіскалізації (ПРРО). `status`:
  `fiscalized` | `pending_fiscal`. `pending_fiscal` = ПРРО офлайн-режим ДПС
  (до 72 год): чек зберігається, фіскалізується пізніше ПЕРЕД повторним push.

Типи транзакцій (значення `type`):

| type | Сутність | Ефекти |
|---|---|---|
| `receipt` | Чек продажу | stock_delta (−), debtor_delta (якщо борг), fiscal |
| `return_receipt` | Повернення | stock_delta (+), debtor_delta, fiscal |
| `purchase` | Закупка/надходження | stock_delta (+) |
| `inventory` | Інвентаризація | stock_delta (коригування до факту) |
| `transfer_out` / `transfer_in` | Переміщення між точками | transfer_out: stock_delta (−) в точці А; transfer_in: + в точці Б (окремі агрегати, різні каси) |
| `write_off` | Списання | stock_delta (−) |
| `cash_operation` | Каса/інкасація | cash_ledger (без stock) |
| `work_session` | Відкриття/закриття зміни | метадані, без stock |

---

## 3. UUID транзакцій каси (ідемпотентність push)

### 3.1 Генерація

- UUIDv4, генерується **на касі** у момент створення транзакції.
- Зберігається в локальній таблиці транзакції (`client_uuid` колонка) та в
  `outbox.client_uuid` — один і той самий.
- Повторне створення (натискання «Пробити» двічі) не дублює UUID:
  генерація відбувається один раз, далі — перевикористання наявного
  ідентифікатора агрегата.

### 3.2 Ідемпотентний прийом на сервері

- Сервер має UNIQUE-обмеження на `client_uuid` у кожній таблиці-приймачі
  (див. розділ 8.2: `sync_meta.client_uuid` + UNIQUE в документах).
- Обробка push:

```
1. BEGIN;
2. SELECT 1 FROM sync_meta WHERE entity='receipt' AND client_uuid=$uuid FOR UPDATE;
   -- якщо рядок є → COMMIT, відповідь 200 {status:"already_exists", id:<id>}
3. INSERT документ (receipts + items) + INSERT sync_meta(client_uuid);
4. Застосувати effects (stock_delta → stock, debtor_delta → ledger);
5. COMMIT;
```

- Відповідь сервера: `200 { "client_uuid": "...", "status": "created" |
  "already_exists", "server_id": "uuid" }`.
- Каса при `already_exists` — просто позначає outbox як `done` (без помилки).

### 3.3 Чому це безпечно

- Push однонапрямлений: сервер ніколи не створює транзакцій сам (крім
  адмін-режиму власника, який пише лише довідники).
- UNIQUE(client_uuid) — захист від дублікатів при retry (мережеві таймаути,
  повторні спроби, збій після COMMIT до отримання відповіді).
- `server_id` повертається касі для локального маппінгу (якщо потрібен).

---

## 4. Черга push (каса → сервер)

### 4.1 Локальна таблиця outbox (SQLite)

```sql
CREATE TABLE outbox (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    type          TEXT    NOT NULL,          -- receipt, return_receipt, ...
    client_uuid   TEXT    NOT NULL UNIQUE,   -- UUIDv4 каси
    payload       TEXT    NOT NULL,          -- JSON агрегата (розділ 2.2)
    status        TEXT    NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending','in_flight','failed','done')),
    attempts      INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT   NOT NULL DEFAULT (datetime('now')),
    last_error    TEXT,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    pushed_at     TEXT
);
CREATE INDEX idx_outbox_status ON outbox(status, next_attempt_at);
CREATE INDEX idx_outbox_created ON outbox(created_at);
```

### 4.2 Порядок відправки

- **Строго FIFO за `created_at`** (годинник каси). Сервер приймає в порядку
  надходження; ідемпотентність робить повторну відправку безпечною.
- Обґрунтування FIFO: інвентаризація, зроблена після продажів, має прийти
  після них (інакше серверна проекція stock тимчасово роз'їдеться). FIFO +
  серверна атомарність прийому гарантують, що фінальна проекція коректна.
- Пакетування: до 50 агрегатів на один HTTP-запит (POST /api/v1/sync/push,
  масив). У межах пакета зберігається порядок created_at.

### 4.3 Retry / backoff

- Немає мережі (connect error): статус лишається `pending`, `next_attempt_at`
  не змінюється (немає сенсу в backoff — просто чекаємо зв'язок).
  Повторна спроба — за подією відновлення з'єднання (ping) або інтервал 30 с.
- Серверна помилка (5xx, 429): `attempts += 1`; exponential backoff:
  `next_attempt_at = now + min(2^attempts, 3600) секунд`
  (1 с → 2 с → 4 с → … → 1 год, далі фіксація на 1 год).
- Валідаційна помилка (400, 422): статус `failed`, `last_error` заповнено.
  **Не retry** — потрібне втручання (аномалія вгору). Каса продовжує
  працювати; блокована транзакція видима в UI як «Потребує уваги».
- Після 10 невдалих спроб (5xx) — статус `failed` + алерт оператору
  (каса працює, черга росте).

### 4.4 Атомарність: транзакція + її ефекти

Запис транзакції каси — **ОДНА SQLite транзакція** (все або нічого):

```
BEGIN IMMEDIATE;
INSERT INTO receipts (client_uuid, store_id, data, ...);        -- агрегат
INSERT INTO receipt_items (...);                                -- рядки
UPDATE stock SET quantity = quantity - 2 WHERE ...;             -- ефект 1
UPDATE debtors_ledger ...;                                      -- ефект 2 (якщо борг)
INSERT INTO outbox (type, client_uuid, payload, status='pending')
       VALUES ('receipt', $uuid, $payload, 'pending');          -- черга
COMMIT;
```

- ROLLBACK при будь-якій помилці — не буває «чека без зміни stock» або
  «зміни stock без чека».
- **ПРРО-фіскалізація — ПОЗА SQLite-транзакцією** (зовнішній виклик ДПС):
  1. Каса зберігає чек з `fiscal.status = 'pending_fiscal'` (SQLite-транзакція).
  2. Фоновий процес фіскалізує (ПРРО онлайн або офлайн-режим ДПС до 72 год).
  3. Після успіху — оновлює `payload.effects.fiscal` локально
     (`fiscalized`, fiscal_no, qr) і відправляє outbox.
  4. Якщо ПРРО недоступний понад 72 год (границя офлайн-режиму ДПС) —
     продаж блокується (політика каси), чек `pending_fiscal` залишається в
     черзі, алерт оператору.
- **Продаж у борг** — debtor_delta в тій самій SQLite-транзакції.
- **Після успішної відповіді сервера** (`created`/`already_exists`) —
  outbox.status = 'done', pushed_at = now.

---

## 5. Порядок pull (каса оновлює довідники)

Каса тягне довідники **у визначеному порядку** (залежності FK + пріоритет
доступності). Один цикл pull = послідовність запитів; кожен запит незалежний
(своя сутність, свій since_version), але застосування — в межах циклу
послідовне.

| Крок | Сутність | Чому саме тут |
|---|---|---|
| 1 | `settings` | Локальні налаштування каси (store_id, назва точки, ПРРО-налаштування, інтервали синку) |
| 2 | `employees` | Працівники + ролі: касир має залогінитись (PIN) до продажів |
| 3 | `categories` | Дерево категорій — FK для products |
| 4 | `products` | Каталог (barcode, назва, одиниця) — основний довідник продажу |
| 5 | `stock_norms` | Норми залишків (min/max) — для сигналів «мало товару»; НЕ кількість |
| 6 | `suppliers` | Постачальники — для закупок/повернень постачальнику |

Правила:
- **Порядок у межах циклу:** settings → employees → categories → products →
  stock_norms → suppliers. Пропуск кроку при помилці не блокує наступні
  (кожен запит самостійний), але застосування products без categories
  тимчасово дає `category_id = NULL` — прийнятно, виправляється наступним pull.
- **Інтервал:** pull кожні 30 с у LAN (налаштовується в settings, мін. 10 с).
- **Тригер негайного pull:** подія адміністратора на сервері (зміна
  довідника) — сервер може надіслати notification (WebSocket/SSE) або каса
  просто частіше опитує; для LAN 30 с достатньо.
- **Каса не редагує довідники** (крім локальних `settings` з простором ключів
  `local.*`). Всі записи в довідникові таблиці SQLite — копії з сервера.

---

## 6. Обробка конфліктів

### 6.1 Чому конфліктів немає

- **Кожна сутність має рівно одного власника запису:**
  - довідники → сервер (каса лише читає копію);
  - транзакції каси → каса (сервер лише приймає).
- Жодна сутність не редагується двома сторонами → немає двох «істин» для
  одного рядка → немає merge, ЛВВ, CRDT.
- Версії монотонні, дельти append-only → немає «перезапису історії».

### 6.2 Edge-cases та їх обробка

| Кейс | Поведінка |
|---|---|
| **Зміна ціни на сервері під час продажу** (каса продала за стару ціну) | Не конфлікт: чек фіксує `price_snapshot` + `name_snapshot` у items. Сервер приймає чек як є, виручка за снапшотом. Нова ціна застосується касам наступним pull. Ніякого ретро-перерахунку |
| **Видалення товару на сервері** | Soft-delete → каса отримує `op:"delete"` → товар зникає з продажу. Відкриті чеки не ламаються (sнапшот у items). Товар, який ще є в локальному stock (кількість > 0), лишається видимим у звітах, але не продається |
| **Інвентаризація під час відкладених чеків** | FIFO outbox гарантує: чеки (з їх stock_delta) прийдуть раніше інвентаризації → серверна проекція stock коректна на момент інвентаризації |
| **Переміщення між точками, коли точка Б ще не існує на сервері** | Сервер відхиляє `transfer_in` (404/422 store_id) → каса Б отримує `failed` + аномалія вгору. Виправлення: створити магазин на сервері, повторний push після виправлення (ручний reset статусу) |
| **Каса відключена тижнями, потім підключається** | Pull: дельта може бути великою → пагінація (500 rows/сторінка). Push: outbox відправляється FIFO пакетами; сервер приймає ідемпотентно; фіскалізація давніх чеків — через ПРРО офлайн-режим (границя 72 год — чек `pending_fiscal` блокував продажі лише якщо був у межах політики; давні чеки фіскалізуються при відновленні, або позначаються як потребуючі ручного втручання ДПС) |
| **Дублікат UUID (каса перезапущена, транзакція повторена)** | UNIQUE(client_uuid) на сервері → `already_exists` → каса позначає done. Жодного дублювання |
| **Годинник каси зсунутий** | `created_at` — лише метадата; версії pull числові (не timestamp), порядок outbox — локальний порядок вставки (rowid), не годинник. Зсув годинника не ламає консистентність |
| **Продаж без зв'язку, а товар видалений на сервері** | Каса продає за локальною копією (можливо, вже позначеною is_deleted після останнього pull). Чек приймається сервером (sнапшот). Після наступного pull товар зникає з продажу |

---

## 7. Міграції SQLite (user_version)

### 7.1 Принцип

- `PRAGMA user_version` — номер схеми каси (INTEGER, старт 0).
- Міграції — файли `frontend/src-tauri/crates/torgashka-infrastructure/
  src/offline/migrations/offline/NNNN_назва.sql`, нумерація з 0001.
- При старті каси: `user_version` → застосувати всі міграції > поточної
  послідовно, КОЖНА в окремій транзакції (BEGIN/COMMIT), після успіху —
  `PRAGMA user_version = N`.
- Існуючі БД (створені `db.rs` без user_version, user_version = 0) проходять
  ті самі міграції з 0001 — це **єдиний шлях** розвитку схеми.
- `ensure_column`-хак (PRAGMA table_info → ALTER) з `db.rs` **залишається
  лише як emergency-запас** для старих версій бінарника; новий код не
  використовує його для нових колонок.

### 7.2 Список міграцій (ЕТАП 1 — цільова схема)

| N | Назва | Зміст |
|---|---|---|
| 0001 | `baseline_legacy` | Базові таблиці `products`, `receipts`, `settings` (як у db.rs) + індекси — для нових інсталяцій; для старих — no-op (CREATE IF NOT EXISTS) |
| 0002 | `sync_meta` | `sync_meta(entity, version)` + `outbox` (розділ 4.1) + `PRAGMA user_version=2` |
| 0003 | `master_tables` | Нормалізовані копії довідників: `categories`, `suppliers`, `employees`, `stock_norms`, `products_v2` (замість JSON-кешу) + `is_deleted`, `server_version` колонки |
| 0004 | `transaction_tables` | Повні локальні таблиці транзакцій: `receipt_items`, `return_receipts`, `purchase_orders`, `inventories`, `transfers`, `write_offs`, `debtors_ledger`, `cash_ledger` + `client_uuid` колонки |
| 0005 | `local_stock` | Локальний `stock(store_id, product_id, quantity, price_snapshot)` — основне робоче сховище кількості каси |
| 0006 | `local_settings_namespace` | Розширення `settings`: простір ключів `local.*` (store_id, sync-інтервали, стан ПРРО) |

### 7.3 Шаблон міграції

```sql
-- 0002_sync_meta.sql
PRAGMA user_version = 1;  -- попередня (щоб міграція була застосована один раз
                          -- у межах транзакції; фінальне значення виставляється кодом)
BEGIN;
CREATE TABLE IF NOT EXISTS sync_meta (
    entity  TEXT PRIMARY KEY,
    version INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS outbox ( ... );  -- див. 4.1
COMMIT;
PRAGMA user_version = 2;  -- виставляється кодом після COMMIT
```

> Точний механізм: Rust-код читає `PRAGMA user_version`, для кожної міграції
> N > current: `BEGIN` → execute SQL файлу N → `COMMIT` → `PRAGMA user_version = N`.

### 7.4 Розширення схеми каси до повної

Ціль: SQLite каси містить **всі** дані, потрібні для роботи точки без сервера:
- довідники (копії з сервера, read-only локально);
- транзакції (повні агрегати, write локально);
- stock (локальний залишок);
- outbox + sync_meta (синхронізація);
- settings (local.* — локальні, інші — копії з сервера).

Схема таблиць — розділ 8.1.

---

## 8. Схема таблиць

### 8.1 SQLite — каса (цільова, після міграцій 0001–0006)

```sql
-- ── Довідники (копії, read-only; server_version = версія, з якою прийшов рядок) ──
CREATE TABLE products (
    id            TEXT PRIMARY KEY,          -- uuid (серверний id)
    barcode       TEXT,
    name          TEXT NOT NULL,
    unit          TEXT,
    category_id   TEXT,
    is_deleted    INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL,          -- версія дельти
    data          TEXT                        -- повний JSON (для зворотної сумісності)
);
CREATE INDEX idx_products_barcode ON products(barcode);
CREATE INDEX idx_products_category ON products(category_id);

CREATE TABLE categories (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT,
    is_deleted INTEGER NOT NULL DEFAULT 0, server_version INTEGER NOT NULL
);
CREATE TABLE suppliers (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, phone TEXT, is_deleted INTEGER NOT NULL DEFAULT 0,
    server_version INTEGER NOT NULL
);
CREATE TABLE employees (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, pin_hash TEXT, role TEXT,
    is_deleted INTEGER NOT NULL DEFAULT 0, server_version INTEGER NOT NULL
);
CREATE TABLE stock_norms (
    product_id TEXT PRIMARY KEY, min_qty NUMERIC, max_qty NUMERIC,
    server_version INTEGER NOT NULL
);

-- ── Локальний stock (робоче сховище кількості каси) ──
CREATE TABLE stock (
    store_id   TEXT NOT NULL,
    product_id TEXT NOT NULL,
    quantity   NUMERIC NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (store_id, product_id)
);

-- ── Транзакції (повні агрегати; client_uuid — ключ ідемпотентності) ──
CREATE TABLE receipts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,   -- локальний rowid
    client_uuid TEXT NOT NULL UNIQUE,
    store_id    TEXT NOT NULL,
    number      INTEGER NOT NULL,
    cashier_id  TEXT,
    total       NUMERIC NOT NULL,
    fiscal_status TEXT NOT NULL DEFAULT 'pending_fiscal',  -- pending_fiscal|fiscalized
    fiscal_no   TEXT,
    fiscal_dt   TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE receipt_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    receipt_client_uuid TEXT NOT NULL REFERENCES receipts(client_uuid),
    product_id TEXT, barcode TEXT, name_snapshot TEXT NOT NULL,
    qty NUMERIC NOT NULL, price NUMERIC NOT NULL, price_snapshot NUMERIC NOT NULL,
    sum NUMERIC NOT NULL
);
CREATE TABLE return_receipts   ( ... );   -- аналогічно receipts
CREATE TABLE purchase_orders   ( ... );
CREATE TABLE inventories       ( ... );
CREATE TABLE transfers         ( ... );
CREATE TABLE write_offs        ( ... );
CREATE TABLE debtors_ledger    ( store_id TEXT, debtor_id TEXT, delta NUMERIC, client_uuid TEXT UNIQUE, ... );
CREATE TABLE cash_ledger       ( store_id TEXT, op_type TEXT, amount NUMERIC, client_uuid TEXT UNIQUE, ... );

-- ── Синхронізація ──
CREATE TABLE sync_meta (
    entity  TEXT PRIMARY KEY,      -- categories|products|stock_norms|suppliers|employees|settings
    version INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE outbox ( ... );      -- див. 4.1
CREATE TABLE settings (
    key TEXT PRIMARY KEY,          -- 'local.store_id', 'local.sync_interval', 'server.shop_name', ...
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### 8.2 PostgreSQL — сервер (агрегатор): таблиці синку

```sql
-- Версії майстер-даних (інкремент у тій самій транзакції, що й зміна)
CREATE TABLE sync_meta (
    entity    text PRIMARY KEY,        -- categories|products|stock_norms|suppliers|employees|settings
    version   bigint NOT NULL DEFAULT 0
);
-- Вставка початкових рядків:
-- INSERT INTO sync_meta(entity) VALUES ('categories'),('products'),('stock_norms'),
--        ('suppliers'),('employees'),('settings');

-- Журнал синхронізації (аудит + SLA зведених звітів)
CREATE TABLE sync_log (
    id          bigserial PRIMARY KEY,
    store_id    uuid NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    direction   varchar(8) NOT NULL CHECK (direction IN ('pull','push')),
    entity      varchar(32) NOT NULL,
    client_uuid uuid,
    status      varchar(16) NOT NULL CHECK (status IN ('ok','error','already_exists')),
    payload_hash text,                    -- sha256 payload (для дедуплікації аудиту)
    error       text,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_sync_log_store ON sync_log(store_id, created_at DESC);
CREATE INDEX ix_sync_log_status ON sync_log(status, created_at);

-- Ідемпотентний прийом транзакцій: у кожній таблиці-приймачі UNIQUE(client_uuid)
-- Приклад для receipts (інші — аналогічно):
ALTER TABLE receipts ADD COLUMN IF NOT EXISTS client_uuid uuid;
CREATE UNIQUE INDEX IF NOT EXISTS uq_receipts_client_uuid ON receipts(client_uuid)
    WHERE client_uuid IS NOT NULL;
-- return_invoices, purchase_orders, inventories, transfers, write_offs,
-- debtor_payments, cash_operations — той самий патерн (client_uuid uuid + UNIQUE)

-- Soft-delete довідників (каса отримує op:"delete")
ALTER TABLE products     ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
ALTER TABLE products     ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE categories   ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
ALTER TABLE suppliers    ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;
ALTER TABLE users        ADD COLUMN IF NOT EXISTS is_deleted boolean NOT NULL DEFAULT false;

-- Тригер інкременту версій (приклад для products):
CREATE OR REPLACE FUNCTION bump_sync_version() RETURNS trigger AS $$
BEGIN
    UPDATE sync_meta SET version = version + 1 WHERE entity = TG_ARGV[0];
    RETURN NEW;
END; $$ LANGUAGE plpgsql;

CREATE TRIGGER trg_products_bump
AFTER INSERT OR UPDATE OR DELETE ON products
FOR EACH ROW EXECUTE FUNCTION bump_sync_version('products');
-- Аналогічні тригери: categories, suppliers, users, stock_norms, system_settings.
-- ВАЖЛИВО: тригер для users має НЕ спрацьовувати на зміну пароля без зміни
-- довідникових полів — або прийняти інкремент версії на будь-яку зміну
-- (безпечніше: каса просто перетягне працівника; обсяг малий).

-- Проєкція stock (агрегатор): перераховується з прийнятих транзакцій.
-- НЕ редагується вручну; каса — джерело істини для кількості своєї точки.
CREATE TABLE IF NOT EXISTS stock_projection (
    store_id   uuid NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    product_id uuid NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    quantity   numeric(10,3) NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (store_id, product_id)
);
```

> **Узгодження з multi-store:** `stores`, `user_stores`, `stock` (поточна
> таблиця), RLS залишаються як є. `stock` у multi-store плані — наявність/
> ціна per store для UI; у offline-first серверна кількість ведеться через
> `stock_projection` (перераховується з push), а `stock.quantity` для кас
> більше не є операційним джерелом (каса пише локально). Це НЕ ламає RLS:
> RLS діє на сервері для адмін-доступу; `stock_projection` покривається
> тими самими політиками (store_id = current_setting('app.store_id')).

---

## 9. Послідовність впровадження (ЕТАПИ 2–7) — оновлено під цей дизайн

Оцінки — людино-дні (Rust_Agent; фронт — React_UI_UX_Agent). Кожен етап має
контракт (ЗАДАЧА/ВХІД/ВИХІД/КРИТЕРІЙ) і приймальний тест.

### ЕТАП 2 — Міграційна база SQLite (user_version) — 1.5 дн
- **Задача:** впровадити `PRAGMA user_version` + движок міграцій
  (`migrations/offline/NNNN_*.sql`), міграції 0001–0002.
- **Вхід:** `offline/db.rs` (поточний), дизайн розділ 7.
- **Вихід:** міграції 0001–0002 + runner; `sync_meta`, `outbox` створюються.
- **Критерій:** стара БД (user_version=0) відкривається і мігрується до 0002
  без втрати даних; повторний запуск ідемпотентний; тест на нову БД.

### ЕТАП 3 — Версіонування майстер-даних на сервері + pull API — 3 дн
- **Задача:** `sync_meta` (PG), тригери bump, soft-delete колонки,
  `GET /api/v1/sync/master?entity&since_version` (пагінація 500), pull-клієнт
  на касі (порядок: settings → employees → categories → products →
  stock_norms → suppliers, інтервал 30 с, атомарне застосування).
- **Вхід:** дизайн розділи 1–2, 5, 8.2; Rust-фасад (axum), RLS.
- **Вихід:** pull працює; каса наповнюється довідниками з сервера.
- **Критерій:** зміна категорії/товару на сервері → каса отримує дельту ≤ 30 с;
  `op:"delete"` прибирає товар з продажу; пагінація > 500 rows працює;
  RLS не пропускає чужу точку.

### ЕТАП 4 — UUID транзакцій + outbox + push API — 4 дн
- **Задача:** `client_uuid` (UUIDv4) на всіх локальних транзакціях;
  outbox-запис в одній SQLite-транзакції з агрегатом та ефектами (stock,
  борг); `POST /api/v1/sync/push` (ідемпотентний прийом, UNIQUE client_uuid,
  sync_log); retry/backoff (розділ 4.3); статусна модель фіскалізації
  (pending_fiscal → fiscalized).
- **Вхід:** дизайн розділи 2–4, 8; існуючий StoreCtx/X-Store-Id.
- **Вихід:** каса пушить чеки/повернення/закупки; сервер приймає без дублікатів.
- **Критерій:** 2× push одного client_uuid → `already_exists`, один запис;
  вимкнений сервер → черга росте, при включенні — вивантажується FIFO;
  ROLLBACK при збої mid-транзакції (немає чека без stock-ефекту).

### ЕТАП 5 — Edge-cases та звірка — 2 дн
- **Задача:** обробка кейсів розділу 6.2 (зміна ціни під час продажу —
  price_snapshot; видалення товару; інвентаризація після чеків; зсув
  годинника); UI-статус синхронізації каси («Остання синхронізація», outbox
  count, «Потребує уваги»).
- **Вхід:** дизайн розділ 6; фронт React.
- **Вихід:** edge-cases покриті тестами, UI статусу синку готовий.
- **Критерій:** тест «продаж під час зміни ціни» — виручка за снапшотом;
  тест «видалення товару» — товар зникає з продажу, чеки не ламаються.

### ЕТАП 6 — Розширення схеми каси до повної (0003–0006) — 3 дн
- **Задача:** нормалізовані довідники (products_v2 замість JSON-кешу),
  повні таблиці транзакцій (return_receipts, purchase_orders, inventories,
  transfers, write_offs, debtors_ledger, cash_ledger), локальний `stock`,
  локальний простір `settings.local.*`; міграція даних з legacy-таблиць
  (products JSON → products_v2).
- **Вхід:** дизайн розділи 7.2, 8.1; поточні дані offline.db.
- **Вихід:** каса повністю самодостатня (усі операції — проти SQLite).
- **Критерій:** продаж/повернення/закупка/інвентаризація/переміщення
  працюють з вимкненим сервером; legacy-дані мігрували без втрат
  (COUNT до/після збігається).

### ЕТАП 7 — Тести, приймання, моніторинг — 2.5 дн
- **Задача:** інтеграційні тести синку (pull дельти, push ідемпотентність,
  FIFO, backoff, edge-cases); QA-сценарії (вимкнення сервера на годину,
  відновлення, зведений звіт власника по 4 точках); моніторинг `sync_log`
  (алерт на failed/стагнацію).
- **Вхід:** дизайн; Rust-тести; QA_Agent.
- **Вихід:** зелені тести, приймальні сценарії пройдені, моніторинг працює.
- **Критерій:** всі тести зелені; симуляція 4 кас × відмова мережі → після
  відновлення всі дані на сервері, дублікатів 0, зведений звіт точний.

---

## 10. Відкриті питання (не блокують ЕТАП 1)

- **Міграція `stock` PG:** чи перейменовувати існуючу `stock` у `stock_projection`
  чи додавати нову таблицю (рішення — на ЕТАПІ 3 разом з DB_Admin_Agent).
- **Сповіщення про зміну довідників** (SSE/WebSocket) — опційно, для LAN 30 с
  інтервал достатній; рішення після ЕТАПУ 3 (виміряти фактичну затримку).
- **Резервне копіювання offline.db** (ключ SQLCipher) — окремий процес,
  поза межами цього дизайну.

> **Документ створено:** System_Architect_Agent (NIKO) — ЕТАП 1 offline-first,
> 2026-09-10. Пов'язаний: ADR-015.
