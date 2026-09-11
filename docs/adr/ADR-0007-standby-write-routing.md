# ADR-0007: Маршрутизація запису на standby-вузлі («варіант B»)

## Статус
✅ Прийнято (2026-09-11) — рішення Творця, заморожений інтерфейс.

Рівень ADR: System / Infrastructure. Джерело фактів: логи продакшн-каси
(`postgres.log`, 127.0.0.1:5433) + код `frontend/src-tauri/crates/*`.

Пов'язані документи:
* `docs/system_documentation/sources/network-replication-etap15-20.md` §9, §10;
* `docs/adr/ADR-015-offline-first-architecture.md` (SQLite-черга, outbox).

---

## 1. Контекст (факти, доведені логами продакшн-каси)

1. **Каса = hot standby репліка.** `[node] mode="standby"`, `standby.signal=True`,
   локальний embedded PostgreSQL на `127.0.0.1:5433`; primary —
   `user=replicator_075454c3 host=192.168.0.160 port=5544`.
   Код: `crates/torgashka-infrastructure/src/node_config.rs:43-53` (`NodeMode`),
   `crates/torgashka-api/src/lib.rs:570-605, 985-1030` (`standby_url_for`,
   `init_local_standby`).
2. **`DATABASE_URL` фасаду на standby резолвиться у локальну репліку**
   (`standby_local_url_wide`), тому **усі** пули фасаду (`read_pool`,
   `write_pool`, `store_pool`) pointing на read-only репліку.
   Код: `crates/torgashka-api/src/lib.rs:748-800, 1140-1215`.
3. **Доказаний дефект (логін фізично неможливий).**
   `crates/torgashka-infrastructure/src/repositories/auth.rs::login_common`
   (рядок 140) безумовно пише `work_sessions`:
   `close_active_work_sessions` → `UPDATE work_sessions` (**auth.rs:111**),
   `create_work_session` → `INSERT INTO work_sessions` (**auth.rs:126**),
   плюс `logout` → `UPDATE work_sessions` (**auth.rs:274**).
   На репліці → `ERROR: cannot execute INSERT/UPDATE in a read-only transaction`.
4. **Фоновий job «network_nodes offline-job»** (`crates/torgashka-api/src/lib.rs:1251-1273`,
   SQL на **lib.rs:1257**) кожні 60 с виконує `UPDATE network_nodes` проти
   `state.write_pool` → та сама помилка, спам у `postgres.log`.
5. **Журнал мережевих подій падає.** `crates/torgashka-api/src/route_local.rs:99-131`
   (`note_connectivity_transition`) → `network::log_node_event`
   (`crates/torgashka-api/src/network.rs:297`, SQL на **network.rs:305**:
   `INSERT INTO network_events`) → read-only error.
   *Фактична корекція до контракту:* таблиця — `network_events`, не
   `network_node_events` (див. §7).
6. **§9 дизайн-документа не реалізовано.** Проєктна вимога
   «`GET` → `local_read_pool`; `POST/PUT/DELETE` → `upstream_write_url`»
   (`network-replication-etap15-20.md:180-189`) у коді відсутня:
   `grep -rn upstream_write_url` по репозиторію = **0 входжень**.
   ЕТАП 18 виконано наполовину: читання через `/api/v1/local/*`
   (`crates/torgashka-api/src/route_local.rs`), шляху запису до primary немає.
7. **Єдиний наявний канал запису з вузла** — SQLite-outbox:
   `crates/torgashka-infrastructure/src/offline/sync_push.rs` (вичитує outbox і
   HTTP-push'ить на `server_url`), `offline/commands.rs::local_enqueue_op`
   (запис операції в чергу), `offline/db.rs`, `offline/transactions.rs`,
   `offline/stock.rs`. Типи: `TYPE_RECEIPT` та не-чекові deltas.
8. **Масштаб проблеми.** У `crates/torgashka-api/src/` **35** DML-точок
   (`INSERT/UPDATE/DELETE`) + **2** DDL-точки (`CREATE ROLE` / `ALTER ROLE`)
   = **37** statement-рівневих write-точок у 11 файлах: `admin.rs`,
   `admin_migrate.rs`, `admin_prro.rs`, `lib.rs`, `network.rs`,
   `network_nodes.rs`, `promote.rs`, `store_context.rs`, `sync.rs`,
   `sync_receivers.rs`. Плюс **3** точки логіну/логауту в
   `torgashka-infrastructure/.../repositories/auth.rs`.
   *Розходження з контрактом (41) — див. §7, не аномалія класифікації.*

**Причина (перші принципи).** На вузлі змішано дві несумісні ролі: «репліка
для читання» і «джерело запису». Репліка фізично read-only, тому будь-який
безумовний write-шлях у коді фасаду є дефектом архітектури, а не конфігурації.

---

## 2. Рішення (варіант B): «запис — тільки туди, де він має сенс»

Для кожного write-шляху явно визначається **ціль запису**; локальна репліка
ціллю запису не є ніколи.

### 2.1 Заморожений інтерфейс (змінювати заборонено без узгодження з NIKO)

| # | Правило |
|---|---------|
| F1 | Нове поле `[node] upstream_write_url` у `db_sources.toml`, тип `Option<String>`, аналогічно `primary_db_url` у `NodeConfig` (`node_config.rs:76-79`). |
| F2 | `mode="primary"` → поле **ігнорується**, поведінка не змінюється взагалі. |
| F3 | `mode="standby"` + поле задане + primary досяжний → адмін-записи, які **мусять** дійти до primary, йдуть через пул на `upstream_write_url`. |
| F4 | `mode="standby"` + primary недосяжний → POS-записи в SQLite-outbox (наявний механізм); адмін-записи → **HTTP 503** з полем `detail` (не сирий 500 від PG). |
| F5 | Локальна репліка — **НІКОЛИ** не ціль запису. Жоден код-шлях не має права робити `INSERT/UPDATE/DELETE` у неї. |
| F6 | Операційні дані вузла (`work_sessions`) на standby **не пишуться в PG взагалі** — локально (SQLite) + outbox-черга, бо логін **мусить** працювати без primary. |

### 2.2 Класи write-точок

| Клас | Семантика | Ціль запису |
|------|-----------|-------------|
| `LOCAL_SQLITE` | Операційні дані вузла; коректні офлайн | Локальна SQLite (`offline/db.rs`) + outbox |
| `UPSTREAM_NOW` | Мусить дійти до primary зараз | Пул на `upstream_write_url`; немає primary → 503 |
| `QUEUE` | Некритично зараз, але не втрачаємо | SQLite-outbox → push на `server_url` |
| `DISABLED_ON_STANDBY` | На репліці не має сенсу | Вимкнено; **одноразовий** лог при старті |

Правило диспетчеризації (єдине місце прийняття рішення):

```
mode == primary            → write_pool (як сьогодні; F2 — незмінно)
mode == standby:
    LOCAL_SQLITE           → SQLite
    UPSTREAM_NOW           → upstream_write_pool (F3); None/недосяжний → 503 (F4)
    QUEUE                  → SQLite-outbox
    DISABLED_ON_STANDBY    → 503 на HTTP-поверхні; одноразовий лог у фоні
```

---

## 3. Таблиця класифікації write-точок

Метод: `grep -rnE '(INSERT INTO|DELETE FROM|UPDATE <table> (SET|AS))'` по
`crates/torgashka-api/src/**/*.rs` + рекурсивний Python-скан літералів;
рядок = перший рядок SQL-літерала. Усього класифіковано **40** точок
(37 у `torgashka-api/src/` + 3 логін/логаут у `torgashka-infrastructure`).

### 3.1 `crates/torgashka-api/src/` — адміністративні + мережеві (UPSTREAM_NOW)

| # | Файл:рядок | Операція / таблиця | Клас | Обґрунтування |
|---|-----------|--------------------|------|---------------|
| 1 | `admin.rs:313` | `INSERT INTO stores` (`create_store`) | `UPSTREAM_NOW` | Реєстр точок мережі — джерело істини на primary; створення точки з каси не має сенсу локально. У транзакції з #2 → вся tx іде на upstream-пул. |
| 2 | `admin.rs:331` | `INSERT INTO user_stores` (`create_store`, автоприв'язка owner) | `UPSTREAM_NOW` | Та сама транзакція, що #1; права доступу глобальні (RLS-контур primary). |
| 3 | `admin.rs:377` | `UPDATE stores` (`update_store`) | `UPSTREAM_NOW` | Реквізити точки — спільні для мережі. |
| 4 | `admin.rs:433` | `UPDATE stores` (`archive_store`, `is_active=false`) | `UPSTREAM_NOW` | Архівація точки керує мережею (каскад #5). |
| 5 | `admin.rs:448` | `UPDATE devices` (`archive_store`, каскад кас) | `UPSTREAM_NOW` | Статус кас бачать усі вузли; та сама tx, що #4. |
| 6 | `admin.rs:576` | `DELETE FROM stores` (`delete_empty_store`, owner-only) | `UPSTREAM_NOW` | Фізичне видалення рядка реєстру — лише на primary. |
| 7 | `admin.rs:703` | `INSERT INTO user_stores … ON CONFLICT DO UPDATE` (`create_worker`) | `UPSTREAM_NOW` | Створення працівника + прив'язка ролі до точки — глобальні (`users`/`user_stores`). |
| 8 | `admin_migrate.rs:157` | `INSERT INTO stores` (`migrate_legacy`) | `UPSTREAM_NOW` | Реєстрація legacy-точки в мережі — primary-only операція. |
| 9 | `admin_migrate.rs:231` | `INSERT INTO devices` (`migrate_legacy`) | `UPSTREAM_NOW` | Реєстрація каси мережі — primary-only. |
| 10 | `admin_prro.rs:348` | `INSERT INTO prro_settings … ON CONFLICT DO UPDATE` (`put_setting`) | `UPSTREAM_NOW` | Адмін-конфіг точки ПРРО — мусить дійти до primary (консистентність фіскалізації). |
| 11 | `network.rs:274` | `INSERT INTO audit_log` (`audit`) | `UPSTREAM_NOW` | Аудит admin-дій — єдиний слід; помилка глушиться (`eprintln!`), блокування запиту немає. |
| 12 | `network.rs:305` | `INSERT INTO network_events` (`log_node_event`) | `UPSTREAM_NOW` | Діагностика мережі (факт 5). Best-effort: помилка глушиться; при недосяжному primary — один рядок у stderr, не спам. |
| 13 | `network.rs:390` | `INSERT INTO devices` (`activate_device`) | `UPSTREAM_NOW` | Активація каси за кодом — реєстр мережі (primary). |
| 14 | `network.rs:459` | `INSERT INTO store_activation_codes … ON CONFLICT DO UPDATE` (`generate_activation_code`) | `UPSTREAM_NOW` | Коди активації дійсні для всієї мережі — primary. |
| 15 | `network.rs:594` | `UPDATE devices SET status` (`set_device_status`) | `UPSTREAM_NOW` | Статус каси — мережевий стан. |
| 16 | `network.rs:660` | `UPDATE devices SET status='deleted'` (`delete_device`) | `UPSTREAM_NOW` | Видалення каси — мережевий стан. |
| 17 | `network_nodes.rs:306` | `INSERT INTO network_nodes` (`create_node`) | `UPSTREAM_NOW` | Реєстр вузлів мережі — primary. |
| 18 | `network_nodes.rs:477` | `UPDATE network_nodes` (`join_node`) | `UPSTREAM_NOW` | Join вузла фіксується на primary (там же пишуться replication-creds). |
| 19 | `network_nodes.rs:702` | `UPDATE network_nodes` (`heartbeat_node`: `last_seen_at`, телеметрія, `lagging`) | `UPSTREAM_NOW` | Heartbeat має сенс лише на primary — там сторінка моніторингу мережі. |
| 20 | `network_nodes.rs:887` | `UPDATE network_nodes` (`owner_node_status`: archive / force-resync) | `UPSTREAM_NOW` | Owner-операції над реєстром вузлів — primary. |

### 3.2 `crates/torgashka-api/src/` — серверні приймачі та service-job (DISABLED_ON_STANDBY)

| # | Файл:рядок | Операція / таблиця | Клас | Обґрунтування |
|---|-----------|--------------------|------|---------------|
| 21 | `lib.rs:1257` | `UPDATE network_nodes` («offline-job», кожні 60 с) | `DISABLED_ON_STANDBY` | **Факт 4.** Це housekeeping primary: переводить чужі вузли в `offline`. На standby крутиться марно → спам у `postgres.log`. Вимкнути job поза primary (одноразовий лог «offline-job вимкнено (режим standby)»). |
| 22 | `sync.rs:179` | `INSERT INTO store_sync_state … ON CONFLICT DO UPDATE` (`upsert_store_sync_state`) | `DISABLED_ON_STANDBY` | Облік синку пристроїв — роль **агрегатора**. Standby-каса агрегатором не є; її власні deltas ідуть outbox'ом (#QUEUE). |
| 23 | `sync.rs:816` | `INSERT INTO sync_log` (`log_sync`, `direction='push'`) | `DISABLED_ON_STANDBY` | Журнал прийому push на агрегаторі; локально не має сенсу. |
| 24 | `sync_receivers.rs:192` | `INSERT INTO stock … DO UPDATE SET quantity = stock.quantity + $4` (`stock_add`, delta) | `DISABLED_ON_STANDBY` | Шлях `sync::push` (каси → агрегатор). Standby-каса не приймає чужі deltas. |
| 25 | `sync_receivers.rs:219` | `INSERT INTO stock … DO UPDATE SET quantity = EXCLUDED.quantity` (`stock_add`, абсолютний рівень) | `DISABLED_ON_STANDBY` | Те саме (inventory-факт). |
| 26 | `sync_receivers.rs:269` | `INSERT INTO purchase_orders` (`accept_purchase_order`) | `DISABLED_ON_STANDBY` | Агрегатор-only ingest документів кас. |
| 27 | `sync_receivers.rs:293` | `INSERT INTO purchase_order_items` (`accept_purchase_order`) | `DISABLED_ON_STANDBY` | Те саме (та сама tx). |
| 28 | `sync_receivers.rs:338` | `INSERT INTO inventories` (`accept_inventory`) | `DISABLED_ON_STANDBY` | Те саме. |
| 29 | `sync_receivers.rs:364` | `INSERT INTO inventory_items` (`accept_inventory`) | `DISABLED_ON_STANDBY` | Те саме. |
| 30 | `sync_receivers.rs:437` | `INSERT INTO transfers` (`accept_transfer`) | `DISABLED_ON_STANDBY` | Те саме. |
| 31 | `sync_receivers.rs:459` | `INSERT INTO transfer_items` (`accept_transfer`) | `DISABLED_ON_STANDBY` | Те саме. |
| 32 | `sync_receivers.rs:514` | `INSERT INTO write_offs` (`accept_write_off`) | `DISABLED_ON_STANDBY` | Те саме. |
| 33 | `sync_receivers.rs:537` | `INSERT INTO write_off_items` (`accept_write_off`) | `DISABLED_ON_STANDBY` | Те саме. |
| 34 | `network_nodes.rs:513` | `CREATE ROLE … LOGIN REPLICATION` (DDL, `join_node`) | `DISABLED_ON_STANDBY` | Провіжн реплікаційної ролі — primary-only (Факт: виконується суперкористувачем primary). |
| 35 | `network_nodes.rs:521` | `ALTER ROLE … WITH LOGIN REPLICATION PASSWORD` (DDL, `join_node`) | `DISABLED_ON_STANDBY` | Те саме (ідемпотентний повторний join). |
| 36 | `promote.rs:172` | `UPDATE public.network_nodes SET role='primary'` (`promote_handler`) | `DISABLED_ON_STANDBY` | Виконується **після** `pg_promote` на локальному пулі — вузол на той момент уже `primary` (F2: поведінка primary не змінюється). У стані репліки гілка недосяжна. |

### 3.3 `crates/torgashka-api/src/` — операційні дані вузла (QUEUE)

| # | Файл:рядок | Операція / таблиця | Клас | Обґрунтування |
|---|-----------|--------------------|------|---------------|
| 37 | `store_context.rs:108` | `UPDATE devices SET last_seen_at = now()` (`store_middleware`, heartbeat пристрою) | `QUEUE` | Некритичний операційний heartbeat, виконується на **кожен** запит пристрою. Не має права блокувати запит і зникати: доставляється outbox-каналом (Факт 7). Помилка вже глушиться (`eprintln!`). |

### 3.4 `crates/torgashka-infrastructure` — логін/логаут (LOCAL_SQLITE)

| # | Файл:рядок | Операція / таблиця | Клас | Обґрунтування |
|---|-----------|--------------------|------|---------------|
| 38 | `repositories/auth.rs:111` | `UPDATE work_sessions SET logout_time, duration_hours` (`close_active_work_sessions`) | `LOCAL_SQLITE` | **Факт 3 + F6.** Робоча сесія — дані вузла; логін мусить працювати без primary. Пишеться в SQLite вузла; реплікація — outbox'ом. **`UPSTREAM_NOW` заборонено.** |
| 39 | `repositories/auth.rs:126` | `INSERT INTO work_sessions` (`create_work_session`) | `LOCAL_SQLITE` | Те саме. Саме цей INSERT дає `read-only transaction` на репліці (Факт 3). |
| 40 | `repositories/auth.rs:274` | `UPDATE work_sessions SET logout_time` (logout) | `LOCAL_SQLITE` | Те саме (закриття зміни). |

### 3.5 Контрольні підсумки класифікації

| Клас | Кількість | Точки |
|------|-----------|-------|
| `UPSTREAM_NOW` (гейт: `ProxyToPrimary`) | 20 | #1–#20 |
| `DISABLED_ON_STANDBY` | 16 | #21–#36 |
| `QUEUE` (гейт: `LocalOutbox`) | 1 | #37 |
| `LOCAL_SQLITE` (гейт: `LocalOutbox`) | 3 | #38–#40 |
| `LocalOutbox` (§3.6, накладна) | 1 | #41 |
| **Усього** | **41** | 37 statement-точок `torgashka-api/src/` + 1 REST-поверхня накладної (`repositories/invoices.rs`) + 3 у `torgashka-infrastructure` |

Перевірки критерію прийняття:
* `UPSTREAM_NOW` **не містить** `work_sessions` → ✅ (#38–#40 = `LOCAL_SQLITE`).
* жоден клас не пише в локальну репліку → ✅ (усі цілі: `upstream_write_url`,
  SQLite, або «вимкнено»).
* 100 % точок мають клас і `файл:рядок` → ✅.
* класифікація накладних — **без «TBD»** (§3.6, #41 = `LocalOutbox`) → ✅.

### 3.6 Накладна `invoice` — клас `LocalOutbox` (остаточно, без «TBD»)

| # | Файл:рядок | Операція / таблиця | Клас | Обґрунтування |
|---|-----------|--------------------|------|---------------|
| 41 | REST `POST /api/v1/invoices` (`crates/torgashka-api/src/invoices.rs:195` → `router_v1.rs:446-451`) → `crates/torgashka-infrastructure/src/repositories/invoices.rs:263` (`update_stock`) | `INSERT invoices` + `INSERT invoice_items` + `stock.quantity + EXCLUDED.quantity` + `products.stock` + `supplier_ledger` (INVOICE, рядок 1112) | `LocalOutbox` | Прибуткова накладна (`STRUCTURE.md:202`, `ROADMAP.md:45`) — документ, який створює каса, а не транзакція push каси (Alembic 0013, рядок 48). Клас — **той самий, що `receipts`**: локальний агрегат + outbox-запис + локальний stock-ефект в ОДНІЙ SQLite-транзакції (§10). Термінологія гейта: `LocalOutbox` ≡ `LOCAL_SQLITE` + `QUEUE` (§11). |

**Обов'язкова умова вмикання (порядок — §12):** клас `LocalOutbox` для
`invoice` вмикається в гейті ТІЛЬКИ після того, як приймач накладних (§9)
реально приймає чергу та застосовує її на primary (AT-11…AT-13 зелені).
До цього моменту `invoice` на standby класифікується як `ProxyToPrimary`
(→ 503 при недосяжному primary), і **черга не накопичується**: інакше
порушується інваріант «жодного запису в нікуди» — саме від такого боргу
страхує `transactions.rs` (`sweep_legacy_unsynced`).

---

## 4. Контракт помилки (F4)

Коли `mode="standby"` і primary недосяжний (або `upstream_write_url` не задано),
будь-який `UPSTREAM_NOW`-запит **не** доходить до PG:

* **HTTP-статус:** `503 Service Unavailable`.
* **Тіло відповіді (JSON, строго ці поля):**
  ```json
  {"detail": "primary недоступний (standby-вузол): адміністративна операція не може бути виконана локально — повторіть, коли мережа відновиться"}
  ```
* **Заголовок-маркер режиму** — у **кожній** відповіді фасаду:
  * `X-Torgashka-Node-Mode: standby` (або `primary`);
  * додатково на 503: `X-Torgashka-Upstream: down` та `Retry-After: 30`.
* **Заборонено:** сирий `500` від `sqlx::Error` (`cannot execute INSERT in a
  read-only transaction`) і будь-який текст помилки PG у тілі відповіді.
* **Логування:** один рядок `[torgashka-api] upstream-write rejected (standby):
  <route>` — не частіше 1/сек на маршрут (анти-спам).
* **Позитивна гілка (F3):** primary досяжний → `2xx` звичайного контракту
  маршруту; тіло і статус не відрізняються від `mode="primary"`.

Для `DISABLED_ON_STANDBY`-фонових job'ів HTTP-контракту немає: одноразовий
лог `[...] <job> вимкнено (режим standby)` на старті, далі тиша.

---

## 5. Приймальні тести

| ID | Сценарій | Доказ коректності |
|----|----------|-------------------|
| AT-1 | standby + primary up: `POST /api/v1/admin/stores` (#1,#2) | `201/200`; рядок `stores` існує на primary `192.168.0.160:5544`; на `127.0.0.1:5433` `SELECT count(*) FROM stores` = без змін; `pg_is_in_recovery()` на 5433 = `true` упродовж тесту. |
| AT-2 | standby + primary down: той самий `POST` (#1,#2) | `503`; тіло містить рівно `detail` (json-схема `{"detail": string}`); заголовки `X-Torgashka-Node-Mode: standby`, `X-Torgashka-Upstream: down`; `postgres.log` репліки **не містить** `read-only transaction` (write не робився). |
| AT-3 | standby + primary down: `POST /api/v1/auth/login` (#38,#39) | `200` + валідний JWT; рядок `work_sessions` з'явився у SQLite вузла; у `postgres.log` репліки — **0** спроб `INSERT/UPDATE work_sessions`; кількість рядків `work_sessions` у репліці не змінилась. |
| AT-4 | standby + primary up: `login` (#38,#39) | `200`; `work_sessions` **не** з'явився в репліці (F6: на standby PG не пишеться взагалі) — перевірка `SELECT count(*)` до/після; рядок є у SQLite; після рестарту primary outbox доставив op у PG. |
| AT-5 | standby, прогін ≥ 3 інтервали job'а (180 с) (#21) | У `postgres.log` — рівно **1** рядок `offline-job вимкнено (режим standby)`; **0** рядків `network_nodes offline-job: ...read-only...`; `status` вузлів на репліці не змінюється. |
| AT-6 | standby: фронт каси опитує `/api/v1/local/status` N разів (#12) | Жодного `read-only transaction` у `postgres.log`; події `degraded_local`/`primary_restored` з'являються у `network_events` **primary**; анти-спам збережено (не більше 1 запису на перехід). |
| AT-7 | standby + primary up: приймачі синку (#22–#33) | `POST /api/v1/sync/push` → `503` + `X-Torgashka-Node-Mode: standby` (серверна роль недоступна на вузлі); жодного запису в `stock/purchase_orders/...` на репліці. |
| AT-8 | статичний guard (CI) | Тест сканує `crates/torgashka-api/src/**` і падає, якщо будь-який `INSERT/UPDATE/DELETE`-рядок виконується на пулі локального стану (`state.local.pool` / `store_pool` у standby-гілці); ціль запису ∈ {`upstream_write_pool`, SQLite}. |
| AT-9 | регресія `mode="primary"` (F2) | Секція `[node]` відсутня або `mode="primary"` → повний наявний набір інтеграційних тестів зелений; unit-тест `NodeConfig::load_from_str` підтверджує, що `upstream_write_url` не впливає на резолв пулів. |
| AT-10 | негативний: standby без `upstream_write_url` | Будь-який `UPSTREAM_NOW`-запит → `503` (той самий контракт §4), **не** тихий запис у репліку і **не** `500`. |

| AT-11 | primary: `POST /api/v1/sync/push` з типом `invoice` (приймач §9) | `200`; накладна видима в `GET /api/v1/invoices` на primary; `stock.quantity` точки виріс на кількість позиції; `products.stock` і `supplier_ledger` (INVOICE) оновлені |
| AT-12 | ідемпотентність: той самий `client_uuid` двічі (§9) | другий push → `already_exists`; `stock` більше **не** зростає; `sync_log` містить `already_exists` |
| AT-13 | негативний: `invoices_v1 = None` (фасад без `TORGASHKA_RUST_INVOICES=1`) | приймач → `PushItemResult::error`; `sync_log.status='error'`; outbox лишається `pending` (тихого ack немає) |
| AT-14 | standby + primary down: створення накладної касою | документ у локальній таблиці `invoices` (offline 0010); SQLite `stock +qty` в **одній** транзакції з outbox; підміна невалідного `product_id` → rollback: немає ні агрегата, ні outbox, ні stock; маркер «очікує синку» |
| AT-15 | звірка після відновлення репліки (§10) | локальний (SQLite) і авторитетний (репліка PG) залишки показані окремо + дельта; вирівнювання доступне інвентаризацією (`set_stock_level`) |

---

## 6. Наслідки

### Позитивні
* Логін і POS-робота на касі перестають залежати від primary (F6) — сенс
  офлайн-стійкості відновлено.
* Адмін/мережеві дані зберігають єдине джерело істини (primary), без
  розходження станів між вузлами.
* Клас-таблиця робить «правильну ціль запису» явною вимогою; нові write-точки
  класифікуються при review.

### Негативні / ціна
* Фасад мусить мати **два** пули: локальний (репліка, читання) і
  `upstream_write_pool` (primary, запис) + SQLite-канал; потрібна
  маршрутизація на рівні хендлерів/репозиторіїв (єдина точка рішення, §2.2).
* `auth.rs` мусить навчитися писати `work_sessions` у SQLite (нова
  outbox-операція типу `TYPE_WORK_SESSION`) — це не «підміна пулу», а
  окремий локальний store (F6).
* `503` замість `500` **змінює публічний контракт** admin-маршрутів на
  standby → фронт має вміти показувати «немає зв'язку з головним сервером» і
  ретраїти (читання заголовка `X-Torgashka-Upstream`).
* Точки #21–#36 на standby стають недоступними: потрібен явний лог і,
  для owner-операцій, підказка «виконуйте на головному сервері».

### Ризики та мінімізація
| Ризик | Мінімізація |
|-------|-------------|
| Частина write-точок залишиться на `write_pool` на standby | AT-8 (статичний guard) + AT-10 |
| Транзакції, що змішують класи (напр. `create_store`: #1+#2) | Клас визначається на рівні **запиту/транзакції**, не statement'а; tx цілком іде на upstream |
| Подвійний запис (outbox + upstream) для `QUEUE` | Outbox-push і upstream-шлях використовують той самий `client_uuid`-ідемпотентний приймач (Факт 7) |
| «Тиха» втрата аудиту (#11) при недосяжному primary | Best-effort зафіксовано явно; один рядок у stderr, без блокування запиту |
| Split-brain при частих перемиканнях | Поза межами ADR: `promote` — ручний, з підтвердженням (§12 дизайн-документа) |

### Що залишається незмінним
`mode="primary"` — повна зворотна сумісність (F2). SQLite-offline-шар
(`offline/*`) не чіпається (дизайн-док §10): він лишається каналом `QUEUE`.
Локальна репліка лишається **тільки** джерелом читання для `/api/v1/local/*`.

---

## 7. Розходження фактів (не аномалія класифікації)

1. **«41 write-точка» vs фактичні 35 DML (+2 DDL).** Рекурсивний скан
   `crates/torgashka-api/src/**/*.rs` дає: `INSERT INTO` — 23, `DELETE FROM` —
   1, `UPDATE <table>` — 11, `CREATE/ALTER ROLE` — 2. Разом **37**
   statement-точок; без DDL — **35**. Перелік у §3 верифікований поштучно.
   Розходження з числом контракту (41) = **−4 … −6** залежно від способу
   підрахунку (контракт не вказав методу; DDL та `ON CONFLICT … DO UPDATE`
   враховуються по-різному). **Класифіковано 100 % фактично наявних точок** —
   додаткових write-точок у вказаних файлах не знайдено.
2. **Таблиця подій.** У контракті — `network_node_events`; у коді —
   `network_events` (`network.rs:305`). Класифікація від цього не змінюється.
3. **`local_read_pool` (§9).** У дизайн-документі є поле `local_read_pool`; у
   `NodeConfig` його немає — локальний пул резолвиться через `local_port` +
   `standby_local_url_wide`. Заморожений інтерфейс вводить лише
   `upstream_write_url` — цьому ADR не суперечить.
4. **АНОМАЛІЯ (за визначенням контракту): не виявлено.** Жодна write-точка не
   залишилась некласифікованою; жодне формулювання не суперечить F1–F6.

5. **Alembic 0013 vs 0016.** `backend/alembic/versions/0013_sync_push_idempotency.py:48`
   фіксує рішення «`invoices` — документи закупівель, не транзакції каси push,
   не чіпати». §9 **свідомо переглядає** його: `invoices` стає приймачем
   (`0016_invoice_push_idempotency.py`). Це еволюція рішення, зафіксована явно
   (не суперечність коду).
6. **F3 звужено: DB-пул → HTTP pass-through.** §2.1 F3 і §3.1 описували
   `UPSTREAM_NOW` як запис через **пул** на `upstream_write_url`
   (реалізовано: `lib.rs:1040-1069`, `route_local.rs:66-88`). Рішення Творця
   вимагає `ProxyToPrimary` = HTTP pass-through із JWT користувача й **без
   нової БД-ролі** → для HTTP-поверхонь пул більше не ціль запису. Зафіксовано
   як **F3'** (§11); реалізований F3-код не видаляється (зворотна сумісність),
   але HTTP-хендлери на standby йдуть через гейт.
7. **Евристика 503 за текстом PG-помилки.** `auth_routes.rs:76-92` розпізнає
   деградацію рядком `m.contains("read-only transaction")` — залежить від
   локалі й версії PostgreSQL. Гейт (§11) замінює її явною політикою; після
   реалізації `write_gate.rs` евристику треба **видалити**, інакше лишається
   другий (неперевірюваний) шлях рішення «куди писати».

---

## 8. Додаток: як додати нову write-точку

1. Визнач, чи дані мають сенс **тільки** на primary (адмін/мережа/агрегатор) →
   `UPSTREAM_NOW` (гейт: `ProxyToPrimary`) або `DISABLED_ON_STANDBY`.
2. Якщо це операційні дані **вузла** (сесії, локальні налаштування) →
   `LOCAL_SQLITE` (гейт: `LocalOutbox`).
3. Якщо дані мусять зрештою дійти до primary, але не зараз → `QUEUE`
   (гейт: `LocalOutbox`).
4. **Ніколи** не писати в `local`/`store_pool` на standby (F5).
5. Додати рядок у §3 цього ADR (файл:рядок, клас, обґрунтування), рядок у
   таблицю політик гейта (§11) і, за потреби, тест у §5.
6. Якщо це POS-документ каси: пиши локальний агрегат + outbox + stock-ефект
   однією транзакцією (`transactions.rs::enqueue_transaction`) і додай тип у
   `is_supported_outbox_type`; приймач на primary мусить існувати **раніше**
   за вмикання класу (порядок §12).

---

## 9. Приймач накладних (`invoice`) на primary

Будується **перевикористанням** наявних серверних механізмів — другого підходу
не вводимо.

| Елемент | Що використовується (наявне) | Доказ |
|---------|------------------------------|-------|
| Транспорт push | `POST /api/v1/sync/push` — той самий, що для чеків | `offline/sync_push.rs:495` |
| Розбір конверта | `PushEnvelope` + `process_push_item` | `torgashka-api/src/sync.rs:546` |
| Реєстр типів | `receiver_table` += `"invoice" => Some("invoices")` | `sync.rs:618-627` |
| Гілка прийому | новий `match`-арм поряд з `"receipt" \| "return_receipt" => accept_receipt_kind` | `sync.rs:601-606` |
| Бізнес-операція | `InvoicesV1Facade` (порт `InvoicesV1Service`): `create` → `confirm` | `torgashka-application/src/services/invoices.rs:15`, `lib.rs:12` |
| Stock-ефект **на primary** | `update_stock`: `stock.quantity + EXCLUDED.quantity` — викликає сервіс, не приймач | `repositories/invoices.rs:263`, виклик `:1095` |
| Побічні ефекти | `products.stock` `:281`; `fiscal_stock` `:294-309`; `change_price` `:1101`; `supplier_ledger` INVOICE `:1112` | `repositories/invoices.rs` |
| Ідемпотентність | `client_uuid` + partial UNIQUE на `invoices` — нова Alembic **0016**, за зразком 0013 | `alembic/versions/0013_sync_push_idempotency.py:74-99` |
| Повторний push | `find_by_client_uuid_in` → `already_exists` | `sync.rs:591-594` |
| `created_at` каси | `parse_created_at_utc` (не `now()`) | `sync.rs:598`, `sync_receivers.rs:35` |
| Перевірка точки | `item.store_id == ctx_store` | `sync.rs:571-588` |

Чому сервіс, а не SQL-приймач рівня `sync_receivers`: накладна має **п'ять**
побічних ефектів (stock точки, `products.stock`, фіскальний залишок, ціна
товару, борг постачальника) — SQL-копія створила б друге джерело істини.
Чек іде саме так: `accept_receipt_kind` → `PosServiceFacade`
(`svc.create_sale_receipt` / `create_return_receipt`), `sync.rs:631-648`.

Обмеження (зафіксовано, не «TBD»):
* сервіс накладних монтується лише під `TORGASHKA_RUST_INVOICES=1`
  (`router_v1.rs:446-448`; `AppState.invoices_v1 = Option<...>`, `lib.rs:143`).
  Якщо `invoices_v1.is_none()` → приймач повертає `PushItemResult::error`
  («приймач накладних не змонтовано»), пише `sync_log` зі `status='error'`
  і **не** підтверджує чергу (outbox лишається `pending`) — тихого ack немає;
* `invoice_items` власного `client_uuid` не отримує: ідемпотентність агрегатна
  (як у `purchase_order_items`, §3.2 #27);
* Alembic 0013 (рядок 48) свідомо виключав `invoices` з приймачів; 0016 —
  явний перегляд цього рішення (§7.5).

---

## 10. Рішення: **(a)** оптимістичне локальне відображення

**Рішення: (a).** Накладна в локальній черзі на standby **негайно** змінює
локальний SQLite-залишок у тій самій транзакції, що й outbox-запис; остаточний
(авторитетний) перерахунок залишку виконує **лише primary** після синку.

**Обґрунтування — узгодженість з чеками, а не UX.** Офлайн-чек уже робить (a):
локальний stock-ефект виконується **всередині** тієї самої SQLite-транзакції,
що `INSERT receipts` + `INSERT outbox`:

| Крок атомарного контуру чека | Файл:рядок |
|------------------------------|-----------|
| Відкриття транзакції | `offline/sync_push.rs:112` (`BEGIN IMMEDIATE`) |
| INSERT агрегата чека | `offline/sync_push.rs:117-121` |
| INSERT outbox(`pending`) | `offline/sync_push.rs:125-128` |
| **Локальний stock-ефект у ТІЙ САМІЙ транзакції** | **`offline/sync_push.rs:204`** — `stock::apply_stock_delta(&tx, sid, pid, delta)` |
| Знак дельти (продаж −q / повернення +q) | `offline/sync_push.rs:199-203` |
| COMMIT | `offline/sync_push.rs:218` |
| Той самий контур для не-чекових агрегатів | `offline/transactions.rs:153` → `apply_effects` `:200` → `apply_stock_delta` `:88` |

Відповідь на питання-детектор: **ТАК** — офлайн-чек змінює локальний
SQLite-залишок у тій самій транзакції, що й outbox-запис. Отже (a) вже
реалізовано для чеків → для накладних приймаємо **той самий** контур, щоб на
одному вузлі не виникло двох різних семантик залишку.

### 10.1 Що саме змінюється локально на standby

1. нова offline-міграція `offline/migrations/offline/0010_invoices.sql`:
   локальні `invoices` + `invoice_items` (дзеркало payload /v2) з `client_uuid`
   і `synced` — наявні `0001–0009` таблиці `invoices` не мають;
2. `TYPE_INVOICE = "invoice"` (`transactions.rs:26` за зразком),
   `table_of("invoice") → "invoices"` (`transactions.rs:120`),
   гілка `apply_effects` → **+qty** (дзеркало `TYPE_PURCHASE_ORDER`,
   `transactions.rs:86-90`), тип у білий список `is_supported_outbox_type`
   (`transactions.rs:133-144`);
3. один виклик `enqueue_transaction(conn, "invoice", payload, store_id)`
   (`transactions.rs:153-204`): агрегат + outbox(`pending`) + `stock +qty`
   атомарно; будь-яка помилка → ROLLBACK усього трьох;
4. `products.stock` і `supplier_ledger` локально **не** чіпаються: загальний
   залишок і борг — прерогатива primary (їх застосовує приймач, §9).

### 10.2 Маркер «очікує синку» (наявні примітиви, без нових полів)

* документ: `outbox.status` для цього `client_uuid` (`pending`/`sending` →
  «очікує синку»; `sent` → «синхронізовано») + `synced` у локальному агрегаті;
* UI: бейдж на документі + лічильник `pending_count()` — той самий, що для чеків;
* вузол: `/api/v1/local/status` (edge-детектор up/down, `route_local.rs:332`) →
  банер «немає зв'язку з головним сервером».

### 10.3 Звірка після повернення репліки

1. outbox-push доставив накладну → приймач застосував її на primary
   (`stock +qty`, борг, ціна) → WAL доніс зміну до репліки;
2. на standby існують **два** числа залишку: локальний оптимістичний (SQLite
   каси, `stock_with_catalog`, `offline/stock.rs`) і авторитетний (репліка PG,
   `LocalApiState.write`, `route_local.rs:79`);
3. master-pull **не** віддає сутність `stock` (обмеження зафіксовано в шапці
   `offline/stock.rs`, міграція 0005) — тому звірка не може бути «тихим
   перезаписом»: показуємо обидва числа й **дельту** по товарах накладної;
   розбіжність вирівнюється **наявним** механізмом — інвентаризацією
   (`set_stock_level`, `offline/stock.rs`), а не новим reconcile-движком;
4. після ack помилкова дельта не накопичується: агрегат позначено `synced`,
   повторний push не створює другого stock-ефекту (ідемпотентність
   `client_uuid`, §9).

Наслідок, який приймаємо свідомо: локальний залишок на standby — **оцінка**, а
не істина. Для чеків це вже зафіксовано (`offline/stock.rs`: від'ємний
залишок допустимий, продаж не блокується неточним локальним рівнем);
накладні підпадають під те саме правило.

---

## 11. Гейт запису (WriteGate)

**Один компонент, одне місце рішення:** `crates/torgashka-api/src/write_gate.rs`
(новий модуль). Причина: сьогодні класифікація розсіяна — адмін-хендлери
беруть `state.write_pool` напряму (`admin.rs:105-109`), а контракт 503
реалізовано евристикою за текстом PG-помилки в auth-маршруті
(`auth_routes.rs:76-92`: `m.contains("read-only transaction")`). Евристика не
масштабується на 41 write-точку і мовчки пропустить новий DML.

### 11.1 Таблиця політик «сутність → політика»

| Сутність / поверхня | Політика | Механізм і доказ |
|---|---|---|
| POS-документи каси: `receipt`, `return_receipt`, `purchase_order`, `inventory`, `transfer`, `write_off`, **`invoice`** | **`LocalOutbox`** | SQLite агрегат + outbox + stock в одній транзакції: `sync_push.rs:112-219`, `transactions.rs:153-204` |
| `work_session` (логін/логаут) | **`LocalOutbox`** | SQLite 0009 + outbox-op `work_session` (`transactions.rs:37`, `auth.rs:76-92`) |
| heartbeat пристрою (`devices.last_seen_at`, `store_context.rs:108`) | **`LocalOutbox`** | некритичний side-effect, не блокує запит (§3.3) |
| Адмін/мережа: `stores`, `user_stores`, `devices` (activate/status/delete), `store_activation_codes`, `network_nodes` (create/join/heartbeat/archive), `prro_settings`, `migrate_legacy`, `audit_log`, `network_events` | **`ProxyToPrimary`** | HTTP pass-through (§3.1 #1–#20) |
| Агрегатор-only приймачі: `/api/v1/sync/push`, `store_sync_state`, `sync_log` | **`DISABLED_ON_STANDBY`** | §3.2 #22–#33: 503, чужі deltas не приймаються |
| DDL провіжну реплікації (`CREATE/ALTER ROLE`) | **`DISABLED_ON_STANDBY`** | §3.2 #34–#35 |
| фоновий `network_nodes offline-job` | **`DISABLED_ON_STANDBY`** | §3.2 #21 |

### 11.2 Семантика `ProxyToPrimary` (без нової БД-ролі)

* реверс-проксі на рівні хендлера: `method + path + query + body` переносяться
  **як є** у `{server_url}{path}`; `server_url` — те саме налаштування SQLite,
  яке використовує outbox (`offline/commands.rs:74-80`; виклик `sync_push.rs:495`),
  тож нового конфіг-ключа не вводимо;
* заголовки переносяться **автентично**: `Authorization: Bearer <JWT
  користувача>` + `X-Store-Id`; гейт не має власних DB-credentials і **не
  відкриває** з'єднання до БД primary;
* відповідь primary (статус + тіло + `X-Torgashka-*`) повертається **verbatim** —
  контракт маршруту не змінюється, змінюється лише хост-виконавець;
* жодних retry/черги: адмін-операція або виконується зараз, або 503 (§4);
* `upstream_write_url` (F3) лишається **тільки** для машинних side-effect
  записів без HTTP-носія (напр. `note_connectivity_transition` →
  `log_node_event`, `route_local.rs:332`). Для HTTP-поверхонь DB-пул більше не
  ціль запису → **F3 звужено** (див. §7.6).

### 11.3 UX, коли primary недоступний

* POS-екран: працює; документи отримують «очікує синку» (§10.2), банер
  «Немає зв'язку з головним сервером» (за `X-Torgashka-Upstream` / `/local/status`);
* адмін-екран: адмін-дії **заблоковані** (disabled) одразу, без чекання помилки;
  підказка — «Зміни магазину/мережі доступні лише на головному сервері»;
* якщо дія все ж надіслана: `503` + `{"detail": ...}` з §4 (не 500, не сирий
  текст PostgreSQL).

### 11.4 Як CI перевіряє відсутність обходу гейта

1. **Статичний guard.** Рекурсивний скан `crates/torgashka-api/src/**/*.rs`:
   кожен рядок з `INSERT/UPDATE/DELETE` мусить мати або (а) виняток приймача
   (`sync.rs`, `sync_receivers.rs`), або (б) рядок у таблиці політик §11.1;
   новий DML без політики → тест падає (той самий метод, що верифікація §3).
2. **Повнота покриття.** Множина сутностей політик == множина write-точок §3
   (41) — реєстри порівнюються тестом (після реалізації `write_gate.rs` —
   автоматично; до того — у review обов'язковим чек-лістом).
3. **Поведінковий.** standby + primary down: кожен `ProxyToPrimary` маршрут дає
   `503` за політикою §4 і **нуль** рядків у `postgres.log` репліки;
   `LocalOutbox`-маршрути дають бізнес-статус і **не роблять жодного**
   `INSERT/UPDATE` у PG.

---

## 12. Порядок викатки (обов'язковий, не переставляти)

| Крок | Зміст | Критерій «можна далі» |
|------|-------|------------------------|
| **1. Приймач + тест** | Alembic `0016_invoice_push_idempotency.py` (`invoices.client_uuid` + partial UNIQUE, за зразком 0013) → `receiver_table("invoice")` + гілка `process_push_item` (§9) → e2e-тест | AT-11, AT-12, AT-13 зелені: push накладної створює документ на primary, `stock +qty`, борг постачальника; повторний push → `already_exists` без другого stock-ефекту |
| **2. Політика в гейті** | offline `0010_invoices.sql` + `TYPE_INVOICE` + рядок `invoice → LocalOutbox` у `write_gate.rs` (§11.1) | AT-14, AT-15: накладна створюється офлайн, локальний `stock +qty` атомарний з outbox, маркер «очікує синку»; **до Кроку 2** класифікація `invoice` у гейті — `ProxyToPrimary` |

**Заборона:** вмикати клас `LocalOutbox` для `invoice` раніше за Крок 1.
Черга без приймача = накопичення `pending`, яке ніколи не буде застосоване на
primary — саме від такого боргу страхує `sweep_legacy_unsynced`
(`transactions.rs:218` — sweep_legacy_unsynced).
