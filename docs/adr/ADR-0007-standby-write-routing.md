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

**Фаза 3.4 (2026-09-11): AT-тести зроблено виконуваними там, де це можливо
наявним харнесом.** Колонка «Статус» має машинно-перевірну відповідність:
✅ executable — тест існує і зелений; ⚠️ частково — виконувана частина є, решта
потребує 2 вузлів; 🕐 — потребує 2-вузлового середовища (точні перешкоди в
примітках). Харнес: тести піднімають фасад in-process, «репліку» імітує
read-only роль на тій самій БД, «primary down» — URL на вільний порт;
**фізичної реплікації з WAL і `postgres.log` репліки в тестовому середовищі
НЕМАЄ** — тому всі докази, що вимагають рядків логу репліки, позначені 🕐.

| ID | Сценарій | Статус | Доказ / тест (`файл:рядок`) |
|----|----------|--------|------------------------------|
| AT-1 | standby + primary up: `POST /api/v1/admin/stores` (#1,#2) | 🕐 | Дві вимоги поза харнесом: (1) **фізична репліка** на `127.0.0.1:5433` з `pg_is_in_recovery() = true` упродовж тесту — in-process репліки немає; (2) перевірка рядка `stores` на primary `192.168.0.160:5544` — друга машина. Доступна частина (pass-through на живий primary + 0 рядків у локальній БД) покрита `tests/adr0007_at_contract.rs:420` (AT-2, крок 1) |
| AT-2 | standby + primary down: той самий `POST` (#1,#2) | ✅ | `tests/adr0007_at_contract.rs:420` — крок 1: `server_url` → живий фейковий primary ⇒ `201` + заголовок/тіло primary пройшли як є; крок 2: `server_url` → недосяжний порт ⇒ `503` + рівно `{"detail": STANDBY_DETAIL}` + `X-Torgashka-Node-Mode: standby` + `X-Torgashka-Upstream: down` + `Retry-After`; кількість рядків `stores` у БД не змінилась. Замість рядків `postgres.log` репліки — структурний доказ «PG не торкали»: пул у стані тесту writable, але жодного запису не сталося |
| AT-3 | standby + primary down: `POST /api/v1/auth/login` (#38,#39) | 🕐 | Харнес не має standby-в'язки `auth`+`work_sessions`-outbox in-process (login-гілка каси піднімається `run_facade` разом із локальною реплікою на `[node] local_port`, якої в тестовому середовищі немає); вимога «0 рядків `INSERT/UPDATE work_sessions` у `postgres.log` репліки» — лог фізичної репліки |
| AT-4 | standby + primary up: `login` (#38,#39) | 🕐 | Те саме, що AT-3 + «після рестарту primary outbox доставив op у PG» — потрібен окремий живий primary як другий вузол |
| AT-5 | standby, прогін ≥ 3 інтервали job'а (180 с) (#21) | 🕐 | Вимога — рядки `postgres.log` репліки («рівно 1 `offline-job вимкнено`», «0 `network_nodes offline-job …read-only…`») ⇒ фізична репліка + довгий прогін; in-process доказ класу job'а вже є статично (`write_gate_guard`, #21 = `network_nodes_offline_job`, `DisabledOnStandby`) |
| AT-6 | standby: фронт каси опитує `/api/v1/local/status` N разів (#12) | 🕐 | Потрібні: реальна репліка (щоб `read-only transaction` узагалі міг з'явитись у її логу) + запис `degraded_local`/`primary_restored` у `network_events` **primary** (другий вузол); анти-спам-механізм покритий статично (`route_local.rs:104-134`, edge-детектор) |
| AT-7 | standby + primary up: приймачі синку (#22–#33) | ✅ | `tests/write_gate_behavior.rs:295` — `POST /api/v1/sync/push` на standby ⇒ `503` + `X-Torgashka-Node-Mode: standby`; «жодного запису в `stock/purchase_orders/…` на репліці» — структурно: у стані тесту немає PG-пулів, гейт відповідає до маршрутизації (той самий метод доказу, що AT-8) + `tests/adr0007_at_contract.rs:565` (той самий маршрут у наборі AT-10: 0 рядків у БД) |
| AT-8 | статичний guard (CI) | ✅ | `tests/write_gate_guard.rs` — 9 тестів: `real_all_crates_tree_has_no_unclassified_dml` (`:432`), `real_src_tree_has_no_unclassified_dml`, `pg_table_registry_is_complete_and_consistent` (`:870`), `every_write_route_has_explicit_class` (`:1151`), `every_admin_pool_entity_has_policy`, `adr_registry_has_all_points_and_matching_policies`, + 3 синтетичні негативні контроли |
| AT-9 | регресія `mode="primary"` (F2) | ✅ | `tests/write_gate_behavior.rs:381` (`primary_mode_behavior_unchanged_f2`) + unit `infrastructure/src/node_config.rs:1039` (`upstream_write_url_field_does_not_affect_primary_resolution`) |
| AT-10 | негативний: standby без `upstream_write_url` | ✅ | `tests/adr0007_at_contract.rs:565` — 12 `UPSTREAM_NOW`-маршрутів (§3.1 + §11.7.9): усі `503` §4 (0×`500`), тіло рівно `{detail}`, у тілі немає `read-only transaction`/`sqlx`; `stores/devices/network_nodes/products/categories/suppliers` — 0 змін (тихого запису немає). Передумова тесту — `resolve_upstream_write_url() == None` |
| AT-11 | primary: `POST /api/v1/sync/push` з типом `invoice` (приймач §9) | ✅ | `tests/sync_invoice_push_e2e.rs:250` — приймач застосовує накладну: `invoices` видима, `stock.quantity +qty`, `supplier_ledger`; + `tests/invoice_standby_outbox_e2e.rs:589` (payload адаптера прийнято, `s1.done == 1`) |
| AT-12 | ідемпотентність: той самий `client_uuid` двічі (§9) | ✅ | `tests/sync_invoice_push_e2e.rs:250` — другий push ⇒ `already_exists`, `stock` не зростає, `sync_log` містить `already_exists` |
| AT-13 | негативний: `invoices_v1 = None` (без `TORGASHKA_RUST_INVOICES=1`) | ✅ | `tests/sync_invoice_disabled_e2e.rs:70` — приймач ⇒ `PushItemResult::error`, `sync_log.status='error'`, outbox лишається `pending` (тихого ack немає) |
| AT-14 | standby + primary down: створення накладної касою | ✅ | Виконувано: `tests/adr0007_at_contract.rs:709` (`at_14_standby_invoice_atomic_and_marker`) — документ `invoices` + `invoice_items` + `outbox(status='pending')` + SQLite `stock +3.000` в ОДНІЙ транзакції (той самий `client_uuid` у всіх чотирьох), маркер «очікує синку» = `pending` + `pending_count() == 1`, репліка read-only (прямий `INSERT` ⇒ `read-only transaction`), 0 рядків у PG. **Валідація каталогу (закрито Фазою 3.5)**: `tests/adr0007_at_contract.rs:827` — невідомий `product_id` ⇒ **400** з людським текстом («товар … відсутній у локальному каталозі точки») і НУЛЬ слідів (агрегат/позиції/outbox/stock без змін, перевірено лічильниками SQLite); той самий контур для повернення постачальнику (`TYPE_RETURN_INVOICE`). Реалізація: `infrastructure/src/offline/catalog.rs:106` (`ensure_products_known`, виклик у тій самій транзакції — `offline/transactions.rs:268` для `enqueue_transaction`, `:396` для `enqueue_invoice`), бізнес-клас помилки — `repositories/outbox_local.rs` (`queue_err_or_business`) → `repositories/outbox_invoices.rs:183` (`invoice_queue_error`); юніт-доказ — `offline/transactions.rs::tests::invoice_with_unknown_product_leaves_no_trace`. Додатково — `tests/invoice_standby_outbox_e2e.rs:338`, `tests/sync_invoice_push_e2e.rs::invoice_unknown_catalog_ref_rejected_humanly` (серверний pre-flight на черзі, написаній БЕЗ локальної валідації) |
| AT-15 | звірка після відновлення репліки (§10) | ✅ | Виконувано: `tests/adr0007_at_contract.rs:946` (`at_15_local_vs_authoritative_stock_delta_and_alignment`) — локальний (SQLite) і авторитетний (PG `stock.quantity`) залишки читаються ОКРЕМО, дельта обчислюється (після офлайн-накладної `3.000` ⇒ `local − auth = 3.000`; після push на primary ⇒ дельта `0`; локальна невивантажена накладна #2 ⇒ дельта `2.000`), вирівнювання — **наявним** механізмом інвентаризації (`enqueue_transaction(TYPE_INVENTORY)` → `set_stock_level`), після чого дельта `0`. **Поверхня (закрито Фазою 3.5)**: `GET /api/v1/local/stock-reconciliation?invoice_id=<uuid>` — `crates/torgashka-api/src/route_local.rs:519` (`local_stock_reconciliation`) віддає по кожному товару документа `local_qty` / `authoritative_qty` / `delta` / `matches`, прапорці `local_is_estimate: true`, `authoritative_source: replica_pg` і підсумок `{total, matching, mismatching}`; невідомий документ ⇒ **404**. Локальна половина — `infrastructure/src/offline/reconciliation.rs:58` (`local_view`), авторитетна — читання `stock` репліки через RLS-пул `StorePool`; жодного reconcile-движка (вирівнює інвентаризація) |

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
   (`set_stock_level`, `offline/stock.rs`), а не новим reconcile-движком.
   **Поверхня показу (Фаза 3.5):** `GET /api/v1/local/stock-reconciliation?invoice_id=<uuid>`
   (`crates/torgashka-api/src/route_local.rs::local_stock_reconciliation`) — READ-ендпоінт
   (`GET` не гейтується write-gate: читання репліки — §10); по кожному товару документа
   `local_qty` (SQLite каси, **оцінка** — прапорець `local_is_estimate: true`),
   `authoritative_qty` (репліка PG, `authoritative_source: replica_pg`), `delta` = local − authoritative,
   `matches`; підсумок `{total, matching, mismatching}`. Локальна половина —
   `offline/reconciliation.rs::local_view` (агрегат черги каси за `client_uuid` + `stock::get_stock_level`);
   документа немає в черзі цієї каси ⇒ `404`. Ендпоінт **лише показує** — жодних записів і
   жодного reconcile-движка;
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
| **`cash_operation`** (касова операція: внесення/інкасація; §11.6) | **`LocalOutbox`** | SQLite агрегат `cash_ledger` (0006) + outbox-запис `cash_operation` + касовий ефект (`offline/cash.rs`), offline-міграція 0011; приймач `cash_operations` (Alembic 0017) |
| `work_session` (логін/логаут) | **`LocalOutbox`** | SQLite 0009 + outbox-op `work_session` (`transactions.rs:37`, `auth.rs:76-92`) |
| heartbeat пристрою (`devices.last_seen_at`, `store_context.rs:108`) | **`LocalOutbox`** | некритичний side-effect, не блокує запит (§3.3) |
| Адмін/мережа: `stores`, `user_stores`, `devices` (activate/status/delete), `store_activation_codes`, `network_nodes` (create/join/heartbeat/archive), `prro_settings`, `migrate_legacy`, `audit_log`, `network_events` | **`ProxyToPrimary`** | HTTP pass-through (§3.1 #1–#20) |
| **`write_off_reason`** (довідник причин списання; §11.6) | **`ProxyToPrimary`** | §11.2 HTTP pass-through; у черзі довідник став би «документом каси», якого primary як документ не знає |
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

### 11.5 Один шлях запису чека (рішення по дублю маршрутів)

**Рішення: `POST /api/v2/receipts/sale|return` — канонічний і ЄДИНИЙ шлях
запису чека; `POST /api/v1/local/receipts` ВИДАЛЕНО, а `kind: "receipt"` у
`POST /api/v1/local/ops` більше не приймається (`400`, вказівка на канонічний
маршрут).** Причина: обидва входи писали в ту саму SQLite-чергу, але з різними
контрактами (сирий JSON каси vs `ReceiptCreateInput`), тобто була **друга
реалізація запису** чека; після появи `OutboxPos` (§11.1) канонічний маршрут
на standby сам лягає в чергу (`sync_push::enqueue_receipt_with_uuid`), тож
дубль ставав чистим ризиком розходження семантики (валідація, снапшоти,
`client_uuid`, stock-ефект). Перевірено, що маршрут не використовується
жодним клієнтом: `grep -rn "local/receipts" frontend/src frontend/src-tauri/crates
backend` → лише власний тест гейта. НЕ-чекові типи лишаються на
`/api/v1/local/ops` — вони ходять у ту саму примітивну функцію
`transactions::enqueue_transaction`, що й `OutboxPos` (одна реалізація, два
входи не створює другої логіки).

### 11.6 Реєстр write-точок `repositories/pos.rs` (ПОВНА класифікація)

**Навіщо розділ.** §3 класифікував лише write-точки `crates/torgashka-api/src/**`
(40 + накладна). Репозиторій POS-сервісу
`crates/torgashka-infrastructure/src/repositories/pos.rs` (3713 рядків) у жодному
розділі ADR не реєструвався — саме тому `cash_operations` (`pos.rs:3603`) вислизнула
з класифікації: каса на standby отримувала або сирий `500 cannot execute INSERT in a
read-only transaction` (до Фази 1), або відмову адаптера без обґрунтованого класу.

**Метод.** Перелічено всі SQL-літерали `INSERT INTO` / `UPDATE <таблиця>` /
`DELETE FROM` у файлі (виключено `SELECT … FOR UPDATE`, коментарі та
`ON CONFLICT … DO UPDATE` як частину одного statement'а). Результат —
**44 statement-рядки у 13 функціях**. Оцінка «22 мутації» з контракту Фази 1.2
вужча за факт: строгий `grep -nE '"\(INSERT|UPDATE|DELETE'` бачить лише 32 рядки
(не ловить SQL у багаторядкових літералах, як-от `INSERT INTO receipts (`), а
«22» — це, ймовірно, кількість *методів* з мутаціями (їх 13) після об'єднання.
Тут — вичерпний перелік за фактом коду; кожен рядок має клас §11.1 і ціль.

| # | рядок | SQL (statement) | клас §11.1 | куди йде на standby (`OutboxPos`) |
|---|-------|-----------------|-----------|----------------------------------|
| 1 | `pos.rs:304` | `INSERT receipts` | POS-документ `receipt` | → черга (`sync_push::enqueue_receipt_with_uuid`) |
| 2 | `pos.rs:542` | `UPDATE stock` | `receipt` | → черга (локальний stock-ефект) |
| 3 | `pos.rs:569` | `UPDATE products` | `receipt` | → черга (`products.stock` — прерогатива primary, локально не чіпається) |
| 4 | `pos.rs:580` | `INSERT stock` (+`ON CONFLICT DO UPDATE`) | `receipt` | → черга |
| 5 | `pos.rs:593` | `UPDATE products` | `receipt` (повернення) | → черга |
| 6 | `pos.rs:636` | `INSERT receipt_items` | `receipt` | → черга |
| 7 | `pos.rs:1981` | `INSERT receipts` (v1) | `receipt` | → черга (борговий чек — відмова, рядки 13–16) |
| 8 | `pos.rs:2047` | `INSERT receipt_items` (v1) | `receipt` | → черга |
| 9 | `pos.rs:2074` | `UPDATE stock` (v1) | `receipt` | → черга |
| 10 | `pos.rs:2116` | `UPDATE stock` (v1) | `receipt` | → черга |
| 11 | `pos.rs:2129` | `INSERT stock` (v1, `ON CONFLICT DO UPDATE`) | `receipt` | → черга |
| 12 | `pos.rs:2150` | `INSERT debtor_payments` | **немає рядка §11.1** | ✖ відмова (АНОМАЛІЯ §11.6.4) |
| 13 | `pos.rs:2169` | `DELETE debtors` | **немає рядка §11.1** | ✖ відмова (АНОМАЛІЯ §11.6.4) |
| 14 | `pos.rs:2175` | `UPDATE debtors` | **немає рядка §11.1** | ✖ відмова (АНОМАЛІЯ §11.6.4) |
| 15 | `pos.rs:2191` | `DELETE debtors` | **немає рядка §11.1** | ✖ відмова (АНОМАЛІЯ §11.6.4) |
| 16 | `pos.rs:2197` | `UPDATE debtors` | **немає рядка §11.1** | ✖ відмова (АНОМАЛІЯ §11.6.4) |
| 17 | `pos.rs:2949` | `INSERT write_offs` | `write_off` | → черга (`enqueue_transaction`) |
| 18 | `pos.rs:2980` | `INSERT write_off_items` | `write_off` | → черга |
| 19 | `pos.rs:3034` | `UPDATE write_offs` (number) | `write_off` | ✖ відмова (§11.6.2: у черзі немає типу update) |
| 20 | `pos.rs:3038` | `UPDATE write_offs` (reason) | `write_off` | ✖ відмова (§11.6.2) |
| 21 | `pos.rs:3042` | `UPDATE write_offs` (write_off_date) | `write_off` | ✖ відмова (§11.6.2) |
| 22 | `pos.rs:3046` | `UPDATE write_offs` (notes) | `write_off` | ✖ відмова (§11.6.2) |
| 23 | `pos.rs:3050` | `DELETE write_off_items` | `write_off` | ✖ відмова (§11.6.2) |
| 24 | `pos.rs:3063` | `INSERT write_off_items` (перепис) | `write_off` | ✖ відмова (§11.6.2) |
| 25 | `pos.rs:3080` | `UPDATE write_offs` (total_amount) | `write_off` | ✖ відмова (§11.6.2) |
| 26 | `pos.rs:3104` | `DELETE write_offs` | `write_off` | ✖ відмова (§11.6.2) |
| 27 | `pos.rs:3144` | `UPDATE stock` (confirm) | `write_off` | ✖ відмова (§11.6.2) |
| 28 | `pos.rs:3154` | `UPDATE write_offs SET status='confirmed'` | `write_off` | ✖ відмова (§11.6.2) |
| 29 | `pos.rs:3200` | `INSERT write_off_reasons` | **`write_off_reason` = `ProxyToPrimary`** (§11.1) | ✖ адаптер відмовляє; гейт проксіює ДО хендлера (§11.6.3) |
| 30 | `pos.rs:3266` | `INSERT transfers` | `transfer` | → черга (`enqueue_transaction`) |
| 31 | `pos.rs:3296` | `INSERT transfer_items` | `transfer` | → черга |
| 32 | `pos.rs:3349` | `UPDATE transfers` (number) | `transfer` | ✖ відмова (§11.6.2) |
| 33 | `pos.rs:3353` | `UPDATE transfers` (from_location) | `transfer` | ✖ відмова (§11.6.2) |
| 34 | `pos.rs:3357` | `UPDATE transfers` (to_location) | `transfer` | ✖ відмова (§11.6.2) |
| 35 | `pos.rs:3361` | `UPDATE transfers` (transfer_date) | `transfer` | ✖ відмова (§11.6.2) |
| 36 | `pos.rs:3365` | `UPDATE transfers` (notes) | `transfer` | ✖ відмова (§11.6.2) |
| 37 | `pos.rs:3369` | `DELETE transfer_items` | `transfer` | ✖ відмова (§11.6.2) |
| 38 | `pos.rs:3379` | `INSERT transfer_items` (перепис) | `transfer` | ✖ відмова (§11.6.2) |
| 39 | `pos.rs:3412` | `DELETE transfers` | `transfer` | ✖ відмова (§11.6.2) |
| 40 | `pos.rs:3456` | `UPDATE stock` (confirm, −) | `transfer` | ✖ відмова (§11.6.2) |
| 41 | `pos.rs:3466` | `UPDATE transfers SET status='confirmed'` | `transfer` | ✖ відмова (§11.6.2) |
| 42 | `pos.rs:3485` | `UPDATE stock` (cancel, +) | `transfer` | ✖ відмова (§11.6.2) |
| 43 | `pos.rs:3495` | `UPDATE transfers SET status='cancelled'` | `transfer` | ✖ відмова (§11.6.2) |
| 44 | `pos.rs:3603` | `INSERT cash_operations` | **`cash_operation` = `LocalOutbox`** (§11.1) | ✅ черга `cash_ledger` + `cash_operation` (Фаза 1.2) |

### 11.6.1 Касова операція (внесення/інкасація) — клас `LocalOutbox` (остаточно)

**Рішення прийняте за рекомендацією NIKO:** касова операція — це така сама дія
каси, як чек: вона змінює грошовий ящик вузла тут і зараз, а primary лише
фіксує факт. Альтернативи відкинуто:

* `ProxyToPrimary` (проксі на primary) — інкасація/внесення стають неможливими
  при недосяжному primary (503), тобто офлайн-каса втрачає ключову операцію;
* `DISABLED_ON_STANDBY` — те саме, ще жорсткіше (немає навіть спроби проксі).

Стек (Фаза 1.2, файл:рядок):

| # | Елемент | Де |
|---|---------|-----|
| 1 | `TYPE_CASH_OPERATION = "cash_operation"` | `offline/transactions.rs:47` |
| 2 | `table_of("cash_operation") → "cash_ledger"` — окремої таблиці агрегата НЕ заводимо, вона вже є (offline-міграція 0006: `client_uuid` + `data` + `store_id` + `synced`) | `offline/transactions.rs:153` |
| 3 | Касовий ефект у тій самій SQLite-транзакції: `deposit` → `+amount`, `collection` → `−amount`; окремі ящики `cash`/`card`; копійки (scale 2, без f64) | `offline/transactions.rs:93` → `offline/cash.rs` (`cash_delta`, `apply_cash_delta`) |
| 4 | Таблиця похідного стану `cash_balance` (`store_id`, `cash_type`, `balance_cents`, `UNIQUE`) | offline-міграція `0011_local_cash.sql`; `SCHEMA_VERSION` 10 → 11 |
| 5 | `enqueue_cash_operation` — тонкий wrapper над `enqueue_transaction` (одна примітивна функція запису, другої не заводимо — §11.5) | `offline/transactions.rs:386` |
| 6 | `OutboxPos::create_cash_operation` — замість `unavailable` пише в чергу; `user_name` порожній (таблиці `users` на вузлі немає — імʼя резолвить primary) | `repositories/outbox_pos.rs:689` |
| 7 | Гейт: `/api/v1/cash-operations` → `cash_operation` → `LocalOutbox` | `write_gate.rs:70`, `:293` |
| 8 | HTTP-статус: 201 на primary / **202** на standby (той самий `created_or_queued`, що в POS-документів). У DTO касової операції поля-маркера немає, тож ознака «в черзі» — код 202 + `X-Torgashka-Node-Mode` + `/api/v1/local/status` | `pos.rs:1434` |
| 9 | Приймач на primary: `receiver_table("cash_operation") → "cash_operations"` + `accept_cash_operation` (валідація людськими текстами, один `INSERT`, `created_at` каси, `user_id` = JWT sub) | `sync.rs:648`, `:908`; `sync_receivers.rs:565` |
| 10 | Ідемпотентність: `cash_operations.client_uuid` + partial UNIQUE `uq_cash_operations_client_uuid` | Alembic `0017_cash_operation_idempotency.py` (ім'я ревізії — 31 символ: `alembic_version.version_num` це `varchar(32)`, довше валило `upgrade head` ПІСЛЯ DDL) |
| 11 | Дзеркало 0017 для тестової БД (`schema.sql` її не має) | `tests/common/sync_schema.rs` |

Свідомо прийняте обмеження: локальний `cash_balance` — **оцінка** вузла (той
самий статус, що локальний `stock`, §10.3). Авторитетний баланс primary рахує
запитом до `cash_operations`; після ack локальний баланс не «доганяє» репліку
автоматично (master-pull не віддає `cash_operations`) — розбіжність
вирівнюється наступною інкасацією/внесенням.

### 11.6.2 Залишкові відмови `update/delete/confirm` документів — свідомий клас

Клас сутності — `LocalOutbox` (`write_off`, `transfer`), але **клас дії** —
«у черзі немає представлення дії над наявним документом»:

* `transactions.rs` не має типу «update» (лише агрегат-створення);
* приймач `sync.rs` уміє тільки `accept_*` (INSERT агрегата, рядки #17–#18,
  #30–#31), тобто «update через новий INSERT» дав би **другий документ** на
  primary і подвійний stock-ефект (`confirm_*` рухає `stock`) — тиха втрата
  даних, найгірший з можливих наслідків;
* тому кожна така дія повертає `PosError::BadRequest` з людським текстом
  «операція недоступна на цьому вузлі (потрібен головний сервер)» → HTTP
  **400**, не 500 і без сирого тексту PG (`outbox_pos.rs:81`).

**Чому НЕ `ProxyToPrimary`** (хоча спокусливо: «перенаправити на primary»):
документ, який редагують, фізично існує **лише в черзі вузла** — на primary
його ще немає (він там з'явиться після синку). Проксі перенаправив би запит на
primary, де цього документа немає → 404 або, гірше, редагування чужого
документа. Єдина коректна семантика до синку — явна відмова; після синку
документ редагується на primary через звичайний UI.

### 11.6.3 Довідник причин списання — `ProxyToPrimary`

`write_off_reasons` (#29) — не документ каси, а **довідник точки** (спільний
для всіх списань). У черзі він поїхав би як агрегат `write_off`, і приймач
створив би хибний документ списання зі stock-ефектом або відкинув його як
невідомий довідник.

Механізм — **наявний** проксі гейта (§11.2), новий не писався:
`classify_request("/api/v1/write-off-reasons") → "write_off_reason"` →
`ProxyToPrimary` → HTTP pass-through (method+path+query+body як є, JWT
користувача, verbatim-відповідь); primary недосяжний → 503 за §4. Друга лінія
захисту: `OutboxPos::create_write_off_reason` лишається `unavailable` — якщо
хтось викличе сервіс повз гейт.

Наслідок для класифікації: `/api/v1/write-offs` — і далі `write_off` (документ
→ черга), `/api/v1/write-off-reasons` — окрема сутність `write_off_reason`.

### 11.6.4 АНОМАЛІЯ: боргові сутності поза §11.1

`debtor_payments`, `debtors` (#12–#16) не мають рядка ні в §3, ні в §11.1.
Клас не вигадано: чек `v1` із `debt_payment`/`debtor_id` відмовляється
(`outbox_pos.rs:347` — борг не втрачається тихо).

Потрібне рішення NIKO (одне з двох):
1. `LocalOutbox` із борговим ефектом на вузлі — нова локальна таблиця боргів
   (дзеркало `debtors`/`debtor_payments`) + приймач на primary; на standby
   борг стає можливим;
2. `ProxyToPrimary` — борговий чек лише онлайн (відмова/503 без мережі).

До рішення поведінка не змінюється: явна відмова з людським текстом (400).

### 11.6.5 Контрольні підсумки

| Клас | Точок | Рядки реєстру |
|------|-------|---------------|
| Черга (`LocalOutbox`: `receipt`, `return_receipt`, `write_off`, `transfer`, `cash_operation`) | 16 | #1–#11, #17, #18, #30, #31, #44 |
| Відмова-як-клас (у черзі немає дії над документом) | 22 | #19–#28, #32–#43 |
| `ProxyToPrimary` (довідник `write_off_reason`) | 1 | #29 |
| Поза §11.1 (АНОМАЛІЯ §11.6.4) | 5 | #12–#16 |
| **Усього** | **44** | 44 statement-рядки у 13 функціях `pos.rs` |

Критерії прийняття (метод §3.5):

* жоден клас не пише в локальну репліку (F5) → ✅ (черга → SQLite;
  `ProxyToPrimary` → HTTP; відмова → 400 до будь-якого SQL);
* 100 % точок мають клас і `файл:рядок` → ✅ (44/44);
* `cash_operation` більше не безкласова → ✅ (§11.6.1; особливо §11.1);
* CI-guard (*`tests/write_gate_guard.rs`*) оновлено разом з реєстром:
  реєстр §3 + §11.6 = 43 точки (41 + `cash_operation` + `write_off_reason`),
  `POLICY_TABLE` = 25 рядків (23 + 2); негативний контроль guard'а збережено;
* решта 22 відмови — не «просто unavailable», а клас із обґрунтуванням
  (§11.6.2) і посиланням на рядок реєстру;
* довідник — через **наявний** проксі гейта, без нового механізму (§11.6.3).


---

### 11.7 Повний реєстр DML усіх крейтів (guard §11.4, версія 2)

#### 11.7.1 Що змінилося і чому

Guard `crates/torgashka-api/tests/write_gate_guard.rs` до цієї фази сканував ЛИШЕ
`crates/torgashka-api/src/**` (39 файлів, 38 DML-рядків). Основна маса DML живе в
`crates/torgashka-infrastructure/src/**` — саме тому з аудиту вислизнули
`cash_operations` (§11.6) і боргові сутності (§11.6.4). Тепер скан охоплює
`crates/{api,application,domain,infrastructure,ocr,prro}/src/**`: **141 файл,
335 DML-рядків, які guard класифікує за 5 правилами**.

| # | Правило | Механізм у коді |
|---|---------|-----------------|
| 1 | Файл-приймач (§11.4 п.1) | `RECEIVER_FILES` = `sync.rs`, `sync_receivers.rs` |
| 2 | Шар локальної SQLite-копії вузла | `write_gate::is_local_sqlite_layer` (`offline/**`, `standby_heartbeat.rs`) |
| 3 | Політика таблиці | `write_gate::POLICY_TABLE` (§11.1 + §11.6 + §11.7) |
| 4 | Політика БАТЬКА-документа (`satellite`) | `write_gate::satellite_parent` + `policy_for_dml_table` (§11.7.5) |
| 5 | Динамічна назва таблиці (`DELETE FROM {table}`) | явний whitelist `DYNAMIC_TABLE_POINTS` у guard (§11.7.7) |

Обидва шари перевірені НЕГАТИВНИМИ КОНТРОЛЯМИ (реальний синтетичний текст, не
«зламай і відкоти»): DML без класу в `torgashka-infrastructure` ловиться; DML
шару локальної SQLite не ловиться (виняток привʼязаний до ШЛЯХУ, той самий текст
у `repositories/**` — ловиться); `satellite` без батька ловиться; динамічна назва
без декларації ловиться.

#### 11.7.2 `LocalOutbox` — 22 таблиці, 169 точок

| таблиця | точок | точки DML (`crate/файл:рядок`) | куди перенаправлено | примітка |
|---|---|---|---|---|
| `stock` | 24 | `api/sync_receivers.rs:192`, `api/sync_receivers.rs:219`, `infrastructure/repositories/documents.rs:1614`, `infrastructure/repositories/documents.rs:1769`, `infrastructure/repositories/documents.rs:2004`, `infrastructure/repositories/documents.rs:2052`, `infrastructure/repositories/invoices.rs:269`, `infrastructure/repositories/invoices.rs:1349`, `infrastructure/repositories/invoices.rs:1542`, `infrastructure/repositories/pos.rs:542`, `infrastructure/repositories/pos.rs:580`, `infrastructure/repositories/pos.rs:2074`, `infrastructure/repositories/pos.rs:2116`, `infrastructure/repositories/pos.rs:2129`, `infrastructure/repositories/pos.rs:3144`, `infrastructure/repositories/pos.rs:3456`, `infrastructure/repositories/pos.rs:3485`, `infrastructure/repositories/products_v2.rs:276`, `infrastructure/repositories/products_v2.rs:420`, `infrastructure/repositories/return_invoices.rs:302`, `infrastructure/repositories/return_invoices.rs:339`, `infrastructure/repositories/write.rs:210`, `infrastructure/repositories/write.rs:422`, `infrastructure/repositories/write.rs:1402` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | Спільний супутній стан документів (receipt/write_off/transfer/inventory/invoice): усі батьки — LocalOutbox, тому власний рядок, не satellite |
| `invoices` | 18 | `infrastructure/repositories/documents.rs:1089`, `infrastructure/repositories/documents.rs:1692`, `infrastructure/repositories/documents.rs:1913`, `infrastructure/repositories/invoices.rs:514`, `infrastructure/repositories/invoices.rs:620`, `infrastructure/repositories/invoices.rs:644`, `infrastructure/repositories/invoices.rs:705`, `infrastructure/repositories/invoices.rs:1012`, `infrastructure/repositories/invoices.rs:1117`, `infrastructure/repositories/invoices.rs:1178`, `infrastructure/repositories/invoices.rs:1215`, `infrastructure/repositories/invoices.rs:1245`, `infrastructure/repositories/invoices.rs:1377`, `infrastructure/repositories/invoices.rs:1492`, `infrastructure/repositories/invoices.rs:1588`, `infrastructure/repositories/purchase_orders.rs:511`, `infrastructure/repositories/return_invoices.rs:833`, `infrastructure/repositories/return_invoices.rs:933` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | Прибуткова накладна (§3.6: клас умовний — вмикається лише після зеленого приймача; приймач `accept_invoice_kind` є) |
| `purchase_orders` | 14 | `api/sync_receivers.rs:269`, `infrastructure/repositories/documents.rs:1279`, `infrastructure/repositories/documents.rs:1956`, `infrastructure/repositories/purchase_orders.rs:290`, `infrastructure/repositories/purchase_orders.rs:351`, `infrastructure/repositories/purchase_orders.rs:360`, `infrastructure/repositories/purchase_orders.rs:370`, `infrastructure/repositories/purchase_orders.rs:380`, `infrastructure/repositories/purchase_orders.rs:390`, `infrastructure/repositories/purchase_orders.rs:399`, `infrastructure/repositories/purchase_orders.rs:408`, `infrastructure/repositories/purchase_orders.rs:464`, `infrastructure/repositories/purchase_orders.rs:546`, `infrastructure/repositories/purchase_orders.rs:557` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | POS-документ каси (правило C) |
| `transfers` | 12 | `api/sync_receivers.rs:437`, `infrastructure/repositories/documents.rs:1142`, `infrastructure/repositories/documents.rs:2014`, `infrastructure/repositories/pos.rs:3266`, `infrastructure/repositories/pos.rs:3349`, `infrastructure/repositories/pos.rs:3353`, `infrastructure/repositories/pos.rs:3357`, `infrastructure/repositories/pos.rs:3361`, `infrastructure/repositories/pos.rs:3365`, `infrastructure/repositories/pos.rs:3412`, `infrastructure/repositories/pos.rs:3466`, `infrastructure/repositories/pos.rs:3495` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | POS-документ каси (правило C) |
| `debtors` | 11 | `infrastructure/repositories/debtors.rs:121`, `infrastructure/repositories/debtors.rs:167`, `infrastructure/repositories/debtors.rs:176`, `infrastructure/repositories/debtors.rs:185`, `infrastructure/repositories/debtors.rs:263`, `infrastructure/repositories/debtors.rs:272`, `infrastructure/repositories/debtors.rs:284`, `infrastructure/repositories/pos.rs:2169`, `infrastructure/repositories/pos.rs:2175`, `infrastructure/repositories/pos.rs:2191`, `infrastructure/repositories/pos.rs:2197` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | РІШЕННЯ NIKO §11.6.4 варіант 1 (борговий чек → LocalOutbox) |
| `invoice_items` | 11 | `infrastructure/repositories/documents.rs:1111`, `infrastructure/repositories/documents.rs:1673`, `infrastructure/repositories/documents.rs:1941`, `infrastructure/repositories/invoices.rs:197`, `infrastructure/repositories/invoices.rs:634`, `infrastructure/repositories/invoices.rs:1029`, `infrastructure/repositories/invoices.rs:1192`, `infrastructure/repositories/invoices.rs:1200`, `infrastructure/repositories/invoices.rs:1459`, `infrastructure/repositories/purchase_orders.rs:532`, `infrastructure/repositories/return_invoices.rs:854` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `invoice` |
| `write_offs` | 10 | `api/sync_receivers.rs:514`, `infrastructure/repositories/documents.rs:1184`, `infrastructure/repositories/pos.rs:2949`, `infrastructure/repositories/pos.rs:3034`, `infrastructure/repositories/pos.rs:3038`, `infrastructure/repositories/pos.rs:3042`, `infrastructure/repositories/pos.rs:3046`, `infrastructure/repositories/pos.rs:3080`, `infrastructure/repositories/pos.rs:3104`, `infrastructure/repositories/pos.rs:3154` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | POS-документ каси (правило C) |
| `return_invoices` | 8 | `infrastructure/repositories/documents.rs:1228`, `infrastructure/repositories/documents.rs:1870`, `infrastructure/repositories/return_invoices.rs:569`, `infrastructure/repositories/return_invoices.rs:661`, `infrastructure/repositories/return_invoices.rs:713`, `infrastructure/repositories/return_invoices.rs:870`, `infrastructure/repositories/return_invoices.rs:891`, `infrastructure/repositories/return_invoices.rs:951` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | POS-документ каси (повернення; гейт-сутність `return_receipt`) |
| `inventories` | 6 | `api/sync_receivers.rs:338`, `infrastructure/repositories/write.rs:1003`, `infrastructure/repositories/write.rs:1129`, `infrastructure/repositories/write.rs:1251`, `infrastructure/repositories/write.rs:1266`, `infrastructure/repositories/write.rs:1284` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | POS-документ каси (правило C) |
| `prro_queue_items` | 6 | `infrastructure/prro/repository.rs:346`, `infrastructure/prro/repository.rs:433`, `infrastructure/prro/repository.rs:453`, `infrastructure/prro/repository.rs:472`, `infrastructure/prro/repository.rs:499`, `infrastructure/prro/schema.rs:133` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | Правило C: черга фіскалізації — частина документа каси |
| `users` | 6 | `infrastructure/repositories/auth.rs:538`, `infrastructure/repositories/auth.rs:631`, `infrastructure/repositories/auth.rs:676`, `infrastructure/repositories/auth.rs:705`, `infrastructure/repositories/auth.rs:744`, `infrastructure/repositories/setup.rs:230` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | Правило C (сесії/логін). PG-точки — `auth.rs`/`setup.rs`; HTTP-поверхня `/api/v1/users/*` гейтиться як `user_stores` → ProxyToPrimary (§3.1 #7), тому на standby недосяжна |
| `receipt_items` | 5 | `infrastructure/prro/repository.rs:655`, `infrastructure/prro/repository.rs:671`, `infrastructure/prro/repository.rs:726`, `infrastructure/repositories/pos.rs:636`, `infrastructure/repositories/pos.rs:2047` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `receipt` |
| `receipts` | 5 | `infrastructure/prro/repository.rs:622`, `infrastructure/prro/repository.rs:644`, `infrastructure/prro/repository.rs:699`, `infrastructure/repositories/pos.rs:304`, `infrastructure/repositories/pos.rs:1981` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | POS-документ каси (правило C, §11.1 рядок 1) |
| `supplier_ledger` | 5 | `infrastructure/repositories/documents.rs:2116`, `infrastructure/repositories/invoices.rs:241`, `infrastructure/repositories/ledger.rs:105`, `infrastructure/repositories/ledger.rs:365`, `infrastructure/repositories/return_invoices.rs:414` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | Журнал розрахунків із постачальником: пишеться лише з транзакції invoice/return_invoice/документа |
| `transfer_items` | 5 | `api/sync_receivers.rs:459`, `infrastructure/repositories/documents.rs:1162`, `infrastructure/repositories/pos.rs:3296`, `infrastructure/repositories/pos.rs:3369`, `infrastructure/repositories/pos.rs:3379` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `transfer` |
| `write_off_items` | 5 | `api/sync_receivers.rs:537`, `infrastructure/repositories/documents.rs:1204`, `infrastructure/repositories/pos.rs:2980`, `infrastructure/repositories/pos.rs:3050`, `infrastructure/repositories/pos.rs:3063` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `write_off` |
| `inventory_items` | 4 | `api/sync_receivers.rs:364`, `infrastructure/repositories/write.rs:1026`, `infrastructure/repositories/write.rs:1143`, `infrastructure/repositories/write.rs:1150` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `inventory` (у ping-списку NIKO стоїть серед довідників — тут `satellite`, бо точки живуть лише в транзакції документа) |
| `purchase_order_items` | 4 | `api/sync_receivers.rs:293`, `infrastructure/repositories/documents.rs:1301`, `infrastructure/repositories/purchase_orders.rs:166`, `infrastructure/repositories/purchase_orders.rs:418` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `purchase_order` |
| `return_invoice_items` | 3 | `infrastructure/repositories/documents.rs:1252`, `infrastructure/repositories/return_invoices.rs:141`, `infrastructure/repositories/return_invoices.rs:675` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | satellite §11.7.5 → `return_receipt` |
| `work_sessions` | 3 | `infrastructure/repositories/auth.rs:190`, `infrastructure/repositories/auth.rs:225`, `infrastructure/repositories/auth.rs:406` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | §11.1 рядок 2 (сесії/логін вузла — локальні) |
| `cash_operations` | 2 | `api/sync_receivers.rs:608`, `infrastructure/repositories/pos.rs:3603` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | §11.6.1 (касовий агрегат `cash_ledger` + черга `cash_operation`) |
| `debtor_payments` | 2 | `infrastructure/repositories/debtors.rs:294`, `infrastructure/repositories/pos.rs:2150` | SQLite-черга/локальна БД вузла (гейт `Pass`; §10) → push черги на primary | РІШЕННЯ NIKO §11.6.4 варіант 1 |
| **Разом** | **169** | — | — | **22 таблиць** |

#### 11.7.3 `ProxyToPrimary` — 18 таблиць, 89 точок

| таблиця | точок | точки DML (`crate/файл:рядок`) | куди перенаправлено | примітка |
|---|---|---|---|---|
| `products` | 27 | `infrastructure/prro/repository.rs:749`, `infrastructure/repositories/documents.rs:1629`, `infrastructure/repositories/documents.rs:1640`, `infrastructure/repositories/documents.rs:1655`, `infrastructure/repositories/documents.rs:1682`, `infrastructure/repositories/documents.rs:1780`, `infrastructure/repositories/documents.rs:1790`, `infrastructure/repositories/documents.rs:2063`, `infrastructure/repositories/invoices.rs:281`, `infrastructure/repositories/invoices.rs:302`, `infrastructure/repositories/invoices.rs:312`, `infrastructure/repositories/invoices.rs:1105`, `infrastructure/repositories/invoices.rs:1361`, `infrastructure/repositories/invoices.rs:1468`, `infrastructure/repositories/invoices.rs:1553`, `infrastructure/repositories/pos.rs:569`, `infrastructure/repositories/pos.rs:593`, `infrastructure/repositories/products_v2.rs:246`, `infrastructure/repositories/products_v2.rs:354`, `infrastructure/repositories/products_v2.rs:471`, `infrastructure/repositories/return_invoices.rs:329`, `infrastructure/repositories/return_invoices.rs:351`, `infrastructure/repositories/return_invoices.rs:372`, `infrastructure/repositories/return_invoices.rs:382`, `infrastructure/repositories/write.rs:170`, `infrastructure/repositories/write.rs:366`, `infrastructure/repositories/write.rs:528` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Довідник (правило C): ціна/товар спільні для мережі, джерело істини primary; локальна копія їде master-pull |
| `print_templates` | 8 | `infrastructure/repositories/print_templates.rs:339`, `infrastructure/repositories/print_templates.rs:350`, `infrastructure/repositories/print_templates.rs:376`, `infrastructure/repositories/print_templates.rs:386`, `infrastructure/repositories/print_templates.rs:414`, `infrastructure/repositories/print_templates.rs:429`, `infrastructure/repositories/print_templates.rs:437`, `infrastructure/repositories/stores.rs:152` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Шаблони друку точки — глобальний стан (правило C) |
| `stores` | 7 | `api/admin.rs:315`, `api/admin.rs:379`, `api/admin.rs:435`, `api/admin.rs:578`, `api/admin_migrate.rs:156`, `infrastructure/repositories/setup.rs:247`, `infrastructure/repositories/stores.rs:104` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `devices` | 6 | `api/admin.rs:450`, `api/admin_migrate.rs:230`, `api/network.rs:389`, `api/network.rs:593`, `api/network.rs:659`, `api/store_context.rs:108` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C). ВИНЯТОК: `store_context.rs:108` — heartbeat (§11.1 рядок 3 → LocalOutbox), див. §11.7.8 |
| `network_nodes` | 6 | `api/lib.rs:1323`, `api/network_nodes.rs:306`, `api/network_nodes.rs:477`, `api/network_nodes.rs:702`, `api/network_nodes.rs:887`, `api/promote.rs:172` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `prro_shifts` | 6 | `infrastructure/prro/repository.rs:162`, `infrastructure/prro/repository.rs:266`, `infrastructure/prro/repository.rs:293`, `infrastructure/prro/repository.rs:315`, `infrastructure/prro/repository.rs:332`, `infrastructure/prro/schema.rs:128` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Правило C (адмін/мережа): зміни ПРРО — фіскальний стан точки, джерело істини primary |
| `user_stores` | 5 | `api/admin.rs:333`, `api/admin.rs:705`, `infrastructure/repositories/setup.rs:261`, `infrastructure/repositories/stores.rs:123`, `infrastructure/repositories/stores.rs:191` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `system_settings` | 4 | `infrastructure/repositories/auth.rs:784`, `infrastructure/repositories/auth.rs:813`, `infrastructure/repositories/auth.rs:825`, `infrastructure/repositories/stores.rs:139` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Системні налаштування — глобальний стан (правило C) |
| `barcodes` | 3 | `infrastructure/repositories/products_v2.rs:610`, `infrastructure/repositories/products_v2.rs:619`, `infrastructure/repositories/products_v2.rs:655` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Довідник (правило C) |
| `categories` | 3 | `infrastructure/repositories/write.rs:558`, `infrastructure/repositories/write.rs:639`, `infrastructure/repositories/write.rs:679` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Довідник (правило C) |
| `product_images` | 3 | `infrastructure/repositories/products_v2.rs:509`, `infrastructure/repositories/products_v2.rs:526`, `infrastructure/repositories/products_v2.rs:565` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Довідник (правило C) |
| `prro_settings` | 3 | `api/admin_prro.rs:347`, `infrastructure/prro/repository.rs:523`, `infrastructure/prro/schema.rs:123` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `suppliers` | 3 | `infrastructure/repositories/write.rs:715`, `infrastructure/repositories/write.rs:796`, `infrastructure/repositories/write.rs:853` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Довідник (правило C) |
| `audit_log` | 1 | `api/network.rs:273` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `network_events` | 1 | `api/network.rs:304` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `owners_db` | 1 | `infrastructure/repositories/setup.rs:276` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Реєстр БД власників — primary-only (правило C, довідники) |
| `store_activation_codes` | 1 | `api/network.rs:458` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | Адмін/мережа (правило C) |
| `write_off_reasons` | 1 | `infrastructure/repositories/pos.rs:3200` | HTTP pass-through §11.2 (JWT користувача; primary недосяжний → 503 §4) | §11.6.3 (довідник) → ProxyToPrimary через наявний проксі гейта |
| **Разом** | **89** | — | — | **18 таблиць** |

#### 11.7.4 `DisabledOnStandby` — 3 таблиці, 6 точок

| таблиця | точок | точки DML (`crate/файл:рядок`) | куди перенаправлено | примітка |
|---|---|---|---|---|
| `replication_ddl_role` | 4 | `api/network_nodes.rs:513`, `api/network_nodes.rs:521`, `infrastructure/provision.rs:284`, `infrastructure/provision.rs:290` | 503 §4 (вниз не пускається) | Агрегатор-only DDL (§3.2 #34–#35) |
| `store_sync_state` | 1 | `api/sync.rs:179` | 503 §4 (вниз не пускається) | Агрегатор-only (правило C) |
| `sync_log` | 1 | `api/sync.rs:987` | 503 §4 (вниз не пускається) | Агрегатор-only (правило C) |
| `schema_revision` | 1 | `infrastructure/db.rs:663` (`record_schema_revision`, `INSERT … ON CONFLICT`) | не проксіюється: DDL-шлях старту вузла (`ensure_schema`) | **Фаза 2.2**: службовий маркер «ревізія схеми застосована» (fingerprint усіх DDL-констант `ensure_schema`). Гарячий шлях = 1 SELECT → DDL не виконується, якщо ревізія збігається (прибирає AccessExclusiveLock'и, що дедлочились із паралельними INSERT'ами тестів). Пише лише вузол з правом мігрити БД; на standby DDL репліки вимкнено (як `replication_ddl_role`) |
| `ddl_markers` | 1 | `infrastructure/db.rs:735` (`ensure_ddl_once`, `INSERT … ON CONFLICT`) | не проксіюється: DDL-шлях тестів/сервісу | **Фаза 2.2**: універсальний маркер «DDL ревізії X застосовано» під тим самим advisory-локом, що `ensure_schema` (`SCHEMA_DDL_LOCK_KEY`). Ужиток: `tests/common/sync_schema.rs` (`test_sync_schema`) — sync-шар Alembic 0011–0014 застосовується РАЗ на ревізію, а не в кожному тест-бінарі |
| **Разом** | **6** | — | — | **3 таблиць** |

#### 11.7.5 Супутні таблиці документів (`satellite`) — 7

Пишуться ЛИШЕ всередині транзакції документа-батька, власного рядка в
`POLICY_TABLE` не мають — політику успадковують через мапінг
`write_gate::SATELLITE_TABLE` (явний, не хардкод у тесті):

| супутня таблиця | батько (сутність гейта) | політика |
|---|---|---|
| `receipt_items` | `receipt` | LocalOutbox |
| `write_off_items` | `write_off` | LocalOutbox |
| `transfer_items` | `transfer` | LocalOutbox |
| `invoice_items` | `invoice` | LocalOutbox |
| `purchase_order_items` | `purchase_order` | LocalOutbox |
| `return_invoice_items` | `return_receipt` (§3.6: таблиця `return_invoices`) | LocalOutbox |
| `inventory_items` | `inventory` | LocalOutbox |

`stock` (24 точки) і `supplier_ledger` (5) — спільні супутні дані КІЛЬКОХ
документів (receipt/write_off/transfer/inventory/invoice/return_invoice), тому
мають власний рядок `LocalOutbox`, а не мапінг на одного батька.

#### 11.7.6 Шар локальної SQLite-копії — виняток з обґрунтуванням

DML у `crates/torgashka-infrastructure/src/offline/**` і
`standby_heartbeat.rs` — **70 точок у 21 таблицях** — це запис
у ЛОКАЛЬНУ БД вузла (той самий канал, що `LocalOutbox`), а не в PG-репліку.
Політики PG цей шар не має; виняток замикається предикатом на шлях, тому новий
файл поза `offline/**` → guard валить.

Таблиці, що існують **лише** в цьому шарі: `outbox` (21), `settings` (7),
`products_v2` (5), `employees` (2), `stock_norms` (2), `sync_meta` (2),
`cash_balance` (1), `t_user` (1) — разом 8 (`NIKO`-правило C: сесії/логін →
`LOCAL_SQLITE`; довідники → локальна копія master-pull). Перекриті назви
(`products`, `receipts`, `stock`, `categories`, `suppliers`, `invoices`,
`purchase_orders`, `write_offs`, `work_sessions`, `sync_log`, `receipt_items`,
`invoice_items`) класифікуються за правилами 1–5 окремо для PG-шару.

#### 11.7.7 Динамічні назви таблиць — 1 точка

| файл:рядок | SQL | whitelist (перевіряється guard'ом) | клас |
|---|---|---|---|
| `{dyn}` | `DELETE FROM {table} WHERE id = $1` | `invoices`, `transfers`, `write_offs`, `return_invoices`, `purchase_orders` | `LocalOutbox` (усі таблиці whitelist'у) |

`documents.rs:1013 delete_document(id, document_type)` — generic-видалення
чернетки документа: `match document_type` (закритий перелік, `other` →
`BadRequest`) визначає таблицю. Guard вимагає ЯВНОЇ декларації точки + whitelist
у `DYNAMIC_TABLE_POINTS`: нова динамічна точка або нове значення в `match` без
політики → падіння тесту.

#### 11.7.8 ВІДКРИТО (Фаза 3): 21 таблиць, 117 точок

Політика таблиці — `LocalOutbox` (на standby гейт каже `Pass`), але клієнтський
адаптер черги існує ЛИШЕ для POS-документів каси
(`OutboxPos`: `create_sale_receipt`, `create_return_receipt`, `create_receipt_v1`,
`create_write_off`, `create_transfer`, `create_cash_operation`). Решта точок на
standby дійде до локальної **read-only** репліки → сирий `500`
`cannot execute INSERT in a read-only transaction`. За контрактом Фази 2.1 НЕ
виправляється: це карта для Фази 3.

| таблиця | точок Фази 3 | точки (`crate/файл:рядок`) |
|---|---|---|
| `invoices` | 18 | `infrastructure/repositories/documents.rs:1089`, `infrastructure/repositories/documents.rs:1692`, `infrastructure/repositories/documents.rs:1913`, `infrastructure/repositories/invoices.rs:514`, `infrastructure/repositories/invoices.rs:620`, `infrastructure/repositories/invoices.rs:644`, `infrastructure/repositories/invoices.rs:705`, `infrastructure/repositories/invoices.rs:1012`, `infrastructure/repositories/invoices.rs:1117`, `infrastructure/repositories/invoices.rs:1178`, `infrastructure/repositories/invoices.rs:1215`, `infrastructure/repositories/invoices.rs:1245`, `infrastructure/repositories/invoices.rs:1377`, `infrastructure/repositories/invoices.rs:1492`, `infrastructure/repositories/invoices.rs:1588`, `infrastructure/repositories/purchase_orders.rs:511`, `infrastructure/repositories/return_invoices.rs:833`, `infrastructure/repositories/return_invoices.rs:933` |
| `stock` | 14 | `infrastructure/repositories/documents.rs:1614`, `infrastructure/repositories/documents.rs:1769`, `infrastructure/repositories/documents.rs:2004`, `infrastructure/repositories/documents.rs:2052`, `infrastructure/repositories/invoices.rs:269`, `infrastructure/repositories/invoices.rs:1349`, `infrastructure/repositories/invoices.rs:1542`, `infrastructure/repositories/products_v2.rs:276`, `infrastructure/repositories/products_v2.rs:420`, `infrastructure/repositories/return_invoices.rs:302`, `infrastructure/repositories/return_invoices.rs:339`, `infrastructure/repositories/write.rs:210`, `infrastructure/repositories/write.rs:422`, `infrastructure/repositories/write.rs:1402` |
| `purchase_orders` | 13 | `infrastructure/repositories/documents.rs:1279`, `infrastructure/repositories/documents.rs:1956`, `infrastructure/repositories/purchase_orders.rs:290`, `infrastructure/repositories/purchase_orders.rs:351`, `infrastructure/repositories/purchase_orders.rs:360`, `infrastructure/repositories/purchase_orders.rs:370`, `infrastructure/repositories/purchase_orders.rs:380`, `infrastructure/repositories/purchase_orders.rs:390`, `infrastructure/repositories/purchase_orders.rs:399`, `infrastructure/repositories/purchase_orders.rs:408`, `infrastructure/repositories/purchase_orders.rs:464`, `infrastructure/repositories/purchase_orders.rs:546`, `infrastructure/repositories/purchase_orders.rs:557` |
| `invoice_items` | 11 | `infrastructure/repositories/documents.rs:1111`, `infrastructure/repositories/documents.rs:1673`, `infrastructure/repositories/documents.rs:1941`, `infrastructure/repositories/invoices.rs:197`, `infrastructure/repositories/invoices.rs:634`, `infrastructure/repositories/invoices.rs:1029`, `infrastructure/repositories/invoices.rs:1192`, `infrastructure/repositories/invoices.rs:1200`, `infrastructure/repositories/invoices.rs:1459`, `infrastructure/repositories/purchase_orders.rs:532`, `infrastructure/repositories/return_invoices.rs:854` |
| `return_invoices` | 8 | `infrastructure/repositories/documents.rs:1228`, `infrastructure/repositories/documents.rs:1870`, `infrastructure/repositories/return_invoices.rs:569`, `infrastructure/repositories/return_invoices.rs:661`, `infrastructure/repositories/return_invoices.rs:713`, `infrastructure/repositories/return_invoices.rs:870`, `infrastructure/repositories/return_invoices.rs:891`, `infrastructure/repositories/return_invoices.rs:951` |
| `debtors` | 7 | `infrastructure/repositories/debtors.rs:121`, `infrastructure/repositories/debtors.rs:167`, `infrastructure/repositories/debtors.rs:176`, `infrastructure/repositories/debtors.rs:185`, `infrastructure/repositories/debtors.rs:263`, `infrastructure/repositories/debtors.rs:272`, `infrastructure/repositories/debtors.rs:284` |
| `prro_queue_items` | 6 | `infrastructure/prro/repository.rs:346`, `infrastructure/prro/repository.rs:433`, `infrastructure/prro/repository.rs:453`, `infrastructure/prro/repository.rs:472`, `infrastructure/prro/repository.rs:499`, `infrastructure/prro/schema.rs:133` |
| `users` | 6 | `infrastructure/repositories/auth.rs:538`, `infrastructure/repositories/auth.rs:631`, `infrastructure/repositories/auth.rs:676`, `infrastructure/repositories/auth.rs:705`, `infrastructure/repositories/auth.rs:744`, `infrastructure/repositories/setup.rs:230` |
| `inventories` | 5 | `infrastructure/repositories/write.rs:1003`, `infrastructure/repositories/write.rs:1129`, `infrastructure/repositories/write.rs:1251`, `infrastructure/repositories/write.rs:1266`, `infrastructure/repositories/write.rs:1284` |
| `supplier_ledger` | 5 | `infrastructure/repositories/documents.rs:2116`, `infrastructure/repositories/invoices.rs:241`, `infrastructure/repositories/ledger.rs:105`, `infrastructure/repositories/ledger.rs:365`, `infrastructure/repositories/return_invoices.rs:414` |
| `receipts` | 3 | `infrastructure/prro/repository.rs:622`, `infrastructure/prro/repository.rs:644`, `infrastructure/prro/repository.rs:699` |
| `receipt_items` | 3 | `infrastructure/prro/repository.rs:655`, `infrastructure/prro/repository.rs:671`, `infrastructure/prro/repository.rs:726` |
| `purchase_order_items` | 3 | `infrastructure/repositories/documents.rs:1301`, `infrastructure/repositories/purchase_orders.rs:166`, `infrastructure/repositories/purchase_orders.rs:418` |
| `return_invoice_items` | 3 | `infrastructure/repositories/documents.rs:1252`, `infrastructure/repositories/return_invoices.rs:141`, `infrastructure/repositories/return_invoices.rs:675` |
| `inventory_items` | 3 | `infrastructure/repositories/write.rs:1026`, `infrastructure/repositories/write.rs:1143`, `infrastructure/repositories/write.rs:1150` |
| `work_sessions` | 3 | `infrastructure/repositories/auth.rs:190`, `infrastructure/repositories/auth.rs:225`, `infrastructure/repositories/auth.rs:406` |
| `transfers` | 2 | `infrastructure/repositories/documents.rs:1142`, `infrastructure/repositories/documents.rs:2014` |
| `write_offs` | 1 | `infrastructure/repositories/documents.rs:1184` |
| `write_off_items` | 1 | `infrastructure/repositories/documents.rs:1204` |
| `transfer_items` | 1 | `infrastructure/repositories/documents.rs:1162` |
| `debtor_payments` | 1 | `infrastructure/repositories/debtors.rs:294` |
| **Разом** | **117** | — |

**Статус після Фаз 3.3a/3.3b (ADR §11.7.9.7).** Ці 117 DML-точок — шлях
**primary** (F2: поведінка не змінена). Для каси на standby закрито всі
HTTP-поверхні, що вели до них: `invoice`, `purchase_order`, `inventory`
(3.3a) та `return_invoice`, `debtor_payment`, `supplier_ledger` + heartbeat
(3.3b) — документ пишеться в SQLite-чергу й доїжджає приймачем на primary.
Поза адаптерами лишаються лише точки, які на standby фізично не досяжні або
класифіковані інакше: довідники й POS-документи каси (`ProxyToPrimary` через
гейт §11.2), агрегаторні `users`/`auth`/`setup` (DisableOnStandby §11.7.4),
`prro_queue_items` (межа §11.7.9.7) та внутрішні сервіс-jobs.

Винятки в межах таблиці `devices` (ProxyToPrimary): точка
`api/store_context.rs:108` — це **heartbeat** пристрою (§11.1 рядок 3 →
`LocalOutbox`), у реєстрі §11.7.3 вона позначена як Фаза 3 (на standby має йти
SQLite-каналом, а не PG).

#### 11.7.9 Класифікація HTTP-поверхонь — МАШИННИЙ аудит (Фаза 3.2)

**Замінює ручний перелік Фази 2.1.** Ручний список давав 19 поверхонь і був
неповний: парсер + реальний `classify_request` знайшли **137** write-поверхонь,
з них **47** із `classify_request → None` (→ `Pass` → сирий `500` у read-only
репліку). Після Фази 3.2 `None` = **0**.

Метод (без здогадок і без копії мапи)

1. Парсер `.route("…", post(..).put(..))` по ВСІХ файлах, де реєструються
   маршрути фасаду: `api/router_v1.rs`, `api/return_invoices.rs` (власний
   `router()`), `api/route_local.rs`, `api/promote.rs` → 137 write-поверхонь
   (`POST`/`PUT`/`PATCH`/`DELETE`); динамічні сегменти (`:id`, `:barcode`)
   підставляються конкретними значеннями.
2. Для кожної — справжній `classify_request(method, path)` + `policy_for(entity)`
   (не копія мапи: тест викликає код гейта).
3. Guard на регресію: `tests/write_gate_guard.rs::
   every_write_route_has_explicit_class` — кожна поверхня мусить мати або клас,
   або обґрунтований запис у `CONSCIOUS_PASS`; інакше падіння з переліком.
   Другий guard: `every_admin_pool_entity_has_policy` (§11.7.9.6).
4. Поведінкова верифікація pass-through (§11.2, критерій E контракту):
   `tests/write_gate_behavior.rs::standby_proxy_routes_return_503_contract`
   розширено з 25 до **63** шляхів — кожен `ProxyToPrimary`-маршрут мусить дати
   `503 §4` + маркери `X-Torgashka-*` і **жодного** `500`.

| Клас | Поверхонь |
|---|---|
| `ProxyToPrimary` | 71 |
| `LocalOutbox` | 47 |
| `DisabledOnStandby` | 1 (`POST /api/v1/sync/push`) |
| Свідомо `Pass` (жодного DML у PG) | 18 |
| **Усього** | **137** |
| `None` ДО Фази 3.2 → ПІСЛЯ | **47 → 0** |

Реєстрація класів: `write_gate.rs` (`classify_request`, нові правила — каталог,
друк, налаштування, ПРРО, документи складу, борги, журнал, setup);
`POLICY_TABLE` = **58** рядків (25 гейт-сутностей: 23 §11.1 + 2 §11.6; 28 таблиць
PG-шару §11.7; 5 поверхневих §11.7.9) — звіряється тестом
`write_gate_guard.rs::pg_table_registry_is_complete_and_consistent` /
`every_policy_entity_is_covered_by_adr_registry_or_document_channel`.

#### 11.7.9.1 `ProxyToPrimary` — 71 поверхонь (усі на standby → HTTP pass-through §11.2)

**`barcodes`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/products/:product_id/barcodes` | `products_v2.rs:312` `products_v2::add_barcode` |
| `DELETE /api/v1/products/:product_id/barcodes/:barcode_id` | `products_v2.rs:336` `products_v2::delete_barcode` |
| `POST /api/v2/products/:product_id/barcodes` | `products_v2.rs:312` `products_v2::add_barcode` |
| `DELETE /api/v2/products/:product_id/barcodes/:barcode_id` | `products_v2.rs:336` `products_v2::delete_barcode` |

**`categories`** — 6 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/categories` | `crud.rs:685` `crud::create_category` |
| `DELETE /api/v1/categories/:id` | `crud.rs:716` `crud::delete_category` |
| `PUT /api/v1/categories/:id` | `crud.rs:701` `crud::update_category` |
| `POST /api/v2/categories` | `categories_v2.rs:306` `categories_v2::create` |
| `DELETE /api/v2/categories/:category_id` | `categories_v2.rs:354` `categories_v2::delete` |
| `PUT /api/v2/categories/:category_id` | `categories_v2.rs:325` `categories_v2::update` |

**`devices`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `DELETE /api/v1/admin/devices/:device_id` | `network.rs:635` `network::delete_device` |
| `POST /api/v1/admin/devices/:device_id/block` | `network.rs:617` `network::block_device` |
| `POST /api/v1/admin/devices/:device_id/unblock` | `network.rs:625` `network::unblock_device` |
| `POST /api/v1/devices/activate` | `network.rs:337` `network::activate_device` |

**`documents_batch`** — 3 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `DELETE /api/v1/documents/:document_id` | `documents.rs:388` `documents::delete` |
| `POST /api/v1/documents/:document_id/copy` | `documents.rs:412` `documents::copy` |
| `POST /api/v1/documents/batch-confirm` | `documents.rs:359` `documents::batch_confirm` |

**`migrate_legacy`** — 1 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/admin/migrate/legacy` | `admin_migrate.rs:129` `admin_migrate::migrate_legacy` |

**`network_nodes`** — 5 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/admin/network-nodes` | `network_nodes.rs:273` `network_nodes::create_node` |
| `POST /api/v1/admin/network-nodes/:node_id/archive` | `network_nodes.rs:929` `network_nodes::archive_node` |
| `POST /api/v1/admin/network-nodes/:node_id/force-resync` | `network_nodes.rs:939` `network_nodes::force_resync_node` |
| `PUT /api/v1/network-nodes/:id/heartbeat` | `network_nodes.rs:596` `network_nodes::heartbeat_node` |
| `POST /api/v1/network-nodes/join` | `network_nodes.rs:388` `network_nodes::join_node` |

**`print_templates`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/print-templates` | `print_templates.rs:443` `print_templates::create_template` |
| `DELETE /api/v1/print-templates/:template_id` | `print_templates.rs:511` `print_templates::delete_template` |
| `PUT /api/v1/print-templates/:template_id` | `print_templates.rs:477` `print_templates::update_template` |
| `POST /api/v1/print-templates/:template_id/set-default` | `print_templates.rs:523` `print_templates::set_default` |

**`product_images`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/products/:product_id/images` | `products_v2.rs:221` `products_v2::upload_image` |
| `DELETE /api/v1/products/:product_id/images/:image_id` | `products_v2.rs:299` `products_v2::delete_image` |
| `POST /api/v2/products/:product_id/images` | `products_v2.rs:221` `products_v2::upload_image` |
| `DELETE /api/v2/products/:product_id/images/:image_id` | `products_v2.rs:299` `products_v2::delete_image` |

**`products`** — 6 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/products` | `crud.rs:630` `crud::create_product` |
| `DELETE /api/v1/products/:id` | `crud.rs:658` `crud::delete_product` |
| `PUT /api/v1/products/:id` | `crud.rs:643` `crud::update_product` |
| `POST /api/v2/products` | `products_v2.rs:362` `products_v2::create_product` |
| `DELETE /api/v2/products/:product_id` | `products_v2.rs:478` `products_v2::delete_product` |
| `PUT /api/v2/products/:product_id` | `products_v2.rs:425` `products_v2::update_product` |

**`prro_settings`** — 2 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `PUT /api/v1/admin/stores/:store_id/prro-settings` | `admin_prro.rs:503` `admin_prro::prro_settings_put` |
| `PUT /api/v2/prro/settings` | `prro.rs:667` `prro::settings_put` |

**`prro_shifts`** — 5 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v2/prro/fiscal/shift/close` | `prro.rs:252` `prro::close_shift` |
| `POST /api/v2/prro/fiscal/shift/open` | `prro.rs:223` `prro::open_shift` |
| `POST /api/v2/prro/receipts/:receipt_id/fiscalize` | `prro.rs:762` `prro::fiscalize_receipt` |
| `POST /api/v2/prro/shift/close` | `pos.rs:1390` `pos::close_shift` |
| `POST /api/v2/prro/shift/open` | `pos.rs:1354` `pos::open_shift` |

**`prro_sync`** — 2 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v2/prro/sync` | `prro.rs:621` `prro::sync_queue` |
| `POST /api/v2/prro/fiscal/sync` | `prro.rs:621` `prro::sync_queue` |

**`setup`** — 1 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/setup` | `setup.rs:133` `setup::setup` |

**`store_activation_codes`** — 1 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/admin/stores/:store_id/activation-code` | `network.rs:411` `network::generate_activation_code` |

**`stores`** — 6 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/admin/network-config/import` | `admin_network_config.rs:386` `admin_network_config::import_config` |
| `POST /api/v1/admin/stores` | `admin.rs:297` `admin::create_store` |
| `DELETE /api/v1/admin/stores/:store_id` | `admin.rs:421` `admin::archive_store` |
| `PUT /api/v1/admin/stores/:store_id` | `admin.rs:353` `admin::update_store` |
| `POST /api/v1/admin/stores/:store_id/delete` | `admin.rs:502` `admin::delete_empty_store` |
| `POST /api/v1/stores` | `stores.rs:90` `stores::create_store` |

**`suppliers`** — 3 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/suppliers` | `crud.rs:752` `crud::create_supplier` |
| `DELETE /api/v1/suppliers/:id` | `crud.rs:783` `crud::delete_supplier` |
| `PUT /api/v1/suppliers/:id` | `crud.rs:768` `crud::update_supplier` |

**`system_settings`** — 2 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `PUT /api/v1/settings` | `auth_routes.rs:993` `auth_routes::settings_batch_update` |
| `PUT /api/v1/settings/:name` | `auth_routes.rs:1014` `auth_routes::settings_update_key` |

**`user_stores`** — 11 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/admin/stores/:store_id/workers` | `admin.rs:661` `admin::create_worker` |
| `POST /api/v1/admin/users/:user_id/activate` | `admin.rs:756` `admin::activate_user` |
| `POST /api/v1/admin/users/:user_id/deactivate` | `admin.rs:748` `admin::deactivate_user` |
| `POST /api/v1/admin/users/:user_id/reset-password` | `admin.rs:803` `admin::reset_password` |
| `POST /api/v1/admin/users/:user_id/reset-pin` | `admin.rs:832` `admin::reset_pin` |
| `POST /api/v1/user-stores` | `stores.rs:99` `stores::assign_user_store` |
| `POST /api/v1/users` | `auth_routes.rs:854` `auth_routes::create_user` |
| `DELETE /api/v1/users/:user_id` | `auth_routes.rs:923` `auth_routes::delete_user` |
| `PUT /api/v1/users/:user_id` | `auth_routes.rs:867` `auth_routes::update_user` |
| `PUT /api/v1/users/:user_id/hourly-rate` | `auth_routes.rs:908` `auth_routes::update_hourly_rate` |
| `PUT /api/v1/users/:user_id/permissions` | `auth_routes.rs:882` `auth_routes::update_permissions` |

**`write_off_reason`** — 1 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/write-off-reasons` | `pos.rs:1193` `pos::create_write_off_reason` |

#### 11.7.9.2 `LocalOutbox` — 47 поверхонь (на standby → локальний шлях; ⚠ = історичний знімок Фази 3.2 — див. статус)

**Статус «АНОМАЛІЯ 3» — ЗАКРИТО (Фази 3.2/3.3a/3.3b/3.8): боргу немає.** Усі 47 поверхонь
мають або локальний канал (адаптери `outbox_*` Фаз 3.3a/3.3b/3.8, §11.7.9.7), або належать
свідомому класу відмови §11.6.2. Позначка ⚠ лишається історичним слідом знімка Фази 3.2
(тоді хендлер ішов у PG напряму); на сьогодні такі поверхні закриті адаптерами — це не борг.
Дві поверхні синхронізації ПРРО (`POST /api/v2/prro/sync`, `POST /api/v2/prro/fiscal/sync`)
**більше не належать цьому класу**: у них власна поверхнева сутність `prro_sync` =
`ProxyToPrimary` (§11.7.9.1, §11.7.9.5): фіскалізація через ДПС із КЕП-ключем вузла — межа,
а не канал каси (§11.7.9.7).

**`cash_operation`** — 1 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/cash-operations` | `pos.rs:1443` `pos::create_cash_operation` |

**`debtors`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/debtors` ⚠ PG-only | `debtors.rs:188` `debtors::create` |
| `POST /api/v1/debtors/:debtor_id` ⚠ PG-only | `debtors.rs:223` `debtors::pay` |
| `PUT /api/v1/debtors/:debtor_id` ⚠ PG-only | `debtors.rs:210` `debtors::update` |
| `POST /api/v1/debtors/:debtor_id/pay` ⚠ PG-only | `debtors.rs:223` `debtors::pay` |

**`inventory`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/inventory` ✅ адаптер 3.3a | `crud.rs:832` `crud::create_inventory` |
| `DELETE /api/v1/inventory/:id` ✅ адаптер 3.3a | `crud.rs:866` `crud::delete_inventory` |
| `PUT /api/v1/inventory/:id` ✅ адаптер 3.3a | `crud.rs:851` `crud::update_inventory` |
| `POST /api/v1/inventory/:id/confirm` ✅ адаптер 3.3a | `crud.rs:880` `crud::confirm_inventory` |

**`invoice`** — 11 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/invoices` ✅ адаптер 3.3a | `invoices.rs:195` `invoices::v1_create` |
| `DELETE /api/v1/invoices/:invoice_id` ✅ адаптер 3.3a | `invoices.rs:236` `invoices::v1_delete` |
| `PUT /api/v1/invoices/:invoice_id` ✅ адаптер 3.3a | `invoices.rs:219` `invoices::v1_update` |
| `POST /api/v1/invoices/:invoice_id/confirm` ✅ адаптер 3.3a | `invoices.rs:266` `invoices::v1_confirm` |
| `POST /api/v1/invoices/:invoice_id/print-items` ✅ адаптер 3.3a | `invoices.rs:298` `invoices::v1_print_items` |
| `POST /api/v2/invoices` ✅ адаптер 3.3a | `invoices.rs:374` `invoices::v2_create` |
| `DELETE /api/v2/invoices/:invoice_id` ✅ адаптер 3.3a | `invoices.rs:422` `invoices::v2_delete` |
| `PUT /api/v2/invoices/:invoice_id` ✅ адаптер 3.3a | `invoices.rs:406` `invoices::v2_update` |
| `POST /api/v2/invoices/:invoice_id/cancel` ✅ адаптер 3.3a | `invoices.rs:487` `invoices::v2_cancel` |
| `POST /api/v2/invoices/:invoice_id/print-items` ✅ адаптер 3.3a | `invoices.rs:471` `invoices::v2_print_items` |
| `POST /api/v2/invoices/confirm` ✅ адаптер 3.3a | `invoices.rs:391` `invoices::v2_confirm` |

**`purchase_order`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/purchase-orders` ✅ адаптер 3.3a | `purchase_orders.rs:180` `purchase_orders::create` |
| `DELETE /api/v1/purchase-orders/:order_id` ✅ адаптер 3.3a | `purchase_orders.rs:215` `purchase_orders::delete` |
| `PUT /api/v1/purchase-orders/:order_id` ✅ адаптер 3.3a | `purchase_orders.rs:197` `purchase_orders::update` |
| `POST /api/v1/purchase-orders/:order_id/confirm` ✅ адаптер 3.3a | `purchase_orders.rs:236` `purchase_orders::confirm` |

**`receipt`** — 5 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/local/ops` | `route_local.rs:431` `local_enqueue_op` |
| `POST /api/v1/local/sync/now` | `route_local.rs:484` `local_sync_now` |
| `POST /api/v1/receipts` | `pos.rs:822` `pos::create_receipt_v1` |
| `POST /api/v2/receipts/return` | `pos.rs:808` `pos::create_return` |
| `POST /api/v2/receipts/sale` | `pos.rs:794` `pos::create_sale` |

**`return_receipt`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/return-invoices` ⚠ PG-only | `return_invoices.rs:184` `create_return` |
| `DELETE /api/v1/return-invoices/:return_id` ⚠ PG-only | `return_invoices.rs:219` `delete_return` |
| `PUT /api/v1/return-invoices/:return_id` ⚠ PG-only | `return_invoices.rs:202` `update_return` |
| `POST /api/v1/return-invoices/:return_id/confirm` ⚠ PG-only | `return_invoices.rs:234` `confirm_return` |

**`supplier_ledger`** — 2 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/ledger` ⚠ PG-only | `ledger.rs:376` `ledger::create_entry_v1` |
| `POST /api/v2/ledger/entries` ⚠ PG-only | `ledger.rs:438` `ledger::create_entry_v2` |

**`transfer`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/transfers` | `pos.rs:1270` `pos::create_transfer` |
| `DELETE /api/v1/transfers/:id` | `pos.rs:1300` `pos::delete_transfer` |
| `PUT /api/v1/transfers/:id` | `pos.rs:1285` `pos::update_transfer` |
| `POST /api/v1/transfers/:id/confirm` | `pos.rs:1314` `pos::confirm_transfer` |

**`work_session`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/auth/login` | `auth_routes.rs:670` `auth_routes::login` |
| `POST /api/v1/auth/login-pin` | `auth_routes.rs:685` `auth_routes::login_pin` |
| `POST /api/v1/auth/logout` | `auth_routes.rs:725` `auth_routes::logout` |
| `POST /api/v1/auth/refresh` | `auth_routes.rs:700` `auth_routes::refresh` |

**`write_off`** — 4 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/write-offs` | `pos.rs:1125` `pos::create_write_off` |
| `DELETE /api/v1/write-offs/:id` | `pos.rs:1155` `pos::delete_write_off` |
| `PUT /api/v1/write-offs/:id` | `pos.rs:1140` `pos::update_write_off` |
| `POST /api/v1/write-offs/:id/confirm` | `pos.rs:1169` `pos::confirm_write_off` |

#### 11.7.9.3 `DisabledOnStandby` — 1 поверхня

**`sync_push`** — 1 поверхонь

| метод + шлях | хендлер (визначення) |
|---|---|
| `POST /api/v1/sync/push` | `sync.rs:501` `sync::push` |

#### 11.7.9.4 Свідомо `Pass` — 18 поверхонь (жодного DML у PostgreSQL)

| метод + шлях | хендлер | причина |
|---|---|---|
| `POST /api/v1/admin/db-sources` | `admin_db_sources.rs:459` `admin_db_sources::create_source` | СВІДОМО Pass (немає DML у PG; |
| `DELETE /api/v1/admin/db-sources/:id` | `admin_db_sources.rs:583` `admin_db_sources::delete_source` | СВІДОМО Pass (немає DML у PG; |
| `PUT /api/v1/admin/db-sources/:id` | `admin_db_sources.rs:522` `admin_db_sources::update_source` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/admin/db-sources/:id/activate` | `admin_db_sources.rs:647` `admin_db_sources::activate_source` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/admin/db-sources/:id/test` | `admin_db_sources.rs:617` `admin_db_sources::test_source` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/admin/db-sources/export-dump` | `admin_db_sources.rs:681` `admin_db_sources::export_dump` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/admin/db-sources/import-dump` | `admin_db_sources.rs:793` `admin_db_sources::import_dump` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/admin/db-sources/provision` | `admin_db_sources.rs:969` `admin_db_sources::provision_source` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/admin/network-config/export` | `admin_network_config.rs:286` `admin_network_config::export_config` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/invoice-ocr/analyze` | `ocr.rs:107` `ocr::analyze_with_matching` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/local/promote` | `promote.rs:90` `promote_handler` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/local/repoint-primary` | `promote.rs:338` `repoint_primary_handler` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/ocr/invoice` | `ocr.rs:76` `ocr::analyze_invoice` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/print-templates/:template_id/render` | `print_templates.rs:536` `print_templates::render_template` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/print/labels/render` | `print_templates.rs:239` `print_templates::labels_render` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/print/price-tags/render` | `print_templates.rs:186` `print_templates::price_tags_render` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v1/print/test` | `print_templates.rs:304` `print_templates::test_print` | СВІДОМО Pass (немає DML у PG; |
| `POST /api/v2/prro/test-connection` | `prro.rs:435` `prro::test_connection` | СВІДОМО Pass (немає DML у PG; |

#### 11.7.9.5 Нові поверхневі сутності (не таблиці) — 5

Поверхня не завжди дорівнює таблиці: три хендлери пишуть PG через
mode-agnostic сервіс, а таблиці-цілі належать РІЗНИМ класам. Клас — найбезпечніший
член набору (ProxyToPrimary: на standby їде на primary, у репліку не пише).

| Сутність | Політика | Чому поверхнева | Поверхні |
|---|---|---|---|
| `catalog_directories` | `ProxyToPrimary` | друга лінія `crud.rs` (`require_admin`): довідники каталогу | `POST/PUT/DELETE /api/v1/products`,`/categories`,`/suppliers`, `/api/v2/*` |
| `documents_batch` | `ProxyToPrimary` | batch-операції над наявними документами складу пишуть і `products` (ProxyToPrimary), і `stock`/`supplier_ledger` | `POST /api/v1/documents/batch-confirm`, `POST /api/v1/documents/:id/copy`, `DELETE /api/v1/documents/:id` |
| `setup` | `ProxyToPrimary` | первинна ініціалізація власника/точки — глобальні таблиці (`stores`, `users`, `user_stores`, `owners_db`) | `POST /api/v1/setup` |
| `outbox_drain` | `LocalOutbox` | Фаза 3.8: drain залишку SQLite-черги у ВЛАСНИЙ PG пише агрегати різних класів (`receipts`, `invoices`, `return_invoices`, `inventories`, …) тим самим ядром `sync::process_push_item` | `POST /api/v1/local/outbox/drain` |
| `prro_sync` | `ProxyToPrimary` | друга половина таблиці `prro_queue_items`: `sync`/`fiscal/sync` — фіскалізація через ДПС із КЕП-ключем вузла (межа §11.7.9.7) | `POST /api/v2/prro/sync`, `POST /api/v2/prro/fiscal/sync` |

`setup` — свідоме рішення: хендлер іде в PG лише коли в БД **немає жодного
користувача** (`repositories/setup.rs:196-204` → інакше `409 Conflict` без DML).
На standby репліка вже містить `users` → відповідь та сама (`409` з primary),
але F5 гарантований: запис ніколи не піде в репліку.

#### 11.7.9.6 АНОМАЛІЯ 1 (ВИПРАВЛЕНО): друга лінія `admin_pool` без політики

`api/crud.rs:165` передавав `admin_pool(state, "catalog_directories")`, але рядка
`catalog_directories` у `POLICY_TABLE` **не існувало** → `decide(Standby,
Some("catalog_directories"))` → `policy_for` → `None` → `Pass` → друга лінія
віддавала **локальну репліку**. Тут це давало лише `SELECT` ролі (500 не
виникав), але інваріант «друга лінія теж під гейтом» був зламаний: будь-який
наступний запис у цій функції пішов би в репліку. Виправлено: рядок додано
(`ProxyToPrimary`), додано guard `every_admin_pool_entity_has_policy` — сканує
`api/src/**` і вимагає політику для КОЖНОГО літерала, переданого в `admin_pool`
(9 викликів).

#### 11.7.9.7 АНОМАЛІЯ 2: клас `LocalOutbox`, але хендлер пише PG → сирий `500` на standby (Фази 3.3a+3.3b: 31 з 31 закрито, 2 поверхні → межа: клас `prro_sync`)

Клас цих таблиць у §11.7.2 **оголошений** (черга/локальна БД), однак
HTTP-хендлер ішов у PG через mode-agnostic сервіс. Наслідок на standby:
`LocalOutbox` → `Pass` → `INSERT/UPDATE` у **read-only** репліку → сирий `500`.

**Фаза 3.3a** (19 поверхонь) і **Фаза 3.3b** (12 поверхонь + heartbeat) додали
клієнтські адаптери (контур §10, зразок `OutboxPos`) і підключили їх в
`api/lib.rs` ОДИН раз на старті за `node_cfg.mode` (`node_cfg_mode()`).

| Сутність | Поверхонь | Стан | Адаптер / межа |
|---|---|---|---|
| `invoice` | 11 | ✅ 3.3a | `repositories/outbox_invoices.rs` (`OutboxInvoicesV1`/`V2`), `lib.rs::init_invoices` |
| `purchase_order` | 4 | ✅ 3.3a | `repositories/outbox_purchase_orders.rs`, `lib.rs::init_purchase_orders` |
| `inventory` | 4 | ✅ 3.3a | `repositories/outbox_write.rs` (`OutboxWrite`), `lib.rs::init_readdirs` |
| `return_invoice` | 4 | ✅ 3.3b | `repositories/outbox_return_invoices.rs` + тип `return_invoice` (`transactions.rs`) + локальний агрегат `return_invoices` (offline-0012, stock **−qty**) + приймач-СЕРВІС `sync.rs::accept_return_invoice_kind` + partial UNIQUE `uq_return_invoices_client_uuid` (Alembic 0018) |
| `debtor_payment` | 4 | ✅ 3.3b | `repositories/outbox_debtors.rs` + тип `debtor_payment`, агрегат `debtors_ledger` (0006), похідний борг `debtor_balances` (0012) + приймач `sync_receivers::accept_debtor_payment` (`debtor_payments` + `UPDATE debtors.total_debt`) + UNIQUE 0018 |
| `supplier_ledger` | 2 | ✅ 3.3b | `repositories/outbox_ledger.rs` + тип `supplier_ledger`, агрегат `supplier_ledger` (0012), похідний `supplier_balances` + приймач `sync_receivers::accept_supplier_ledger` (`SUM(amount)+amount` в одній транзакції) + UNIQUE 0018 |
| `prro_queue_items` | 2 | 🚫 межа `ProxyToPrimary` — поверхня `prro_sync` (§11.7.9.5) | фіскалізація — див. пояснення |
| heartbeat `api/store_context.rs:108` | внутр. (не маршрут) | ✅ 3.3b | `standby_heartbeat::record_device_seen` → SQLite `device_heartbeats` (offline-0013) |
| **Разом** | **31 + heartbeat** | **29 ✅ / 2 🚫** | |

**Клас `prro_sync` (Фаза 1):** дві поверхні `POST /api/v2/prro/sync` і `POST /api/v2/prro/fiscal/sync` — рядок `("prro_sync", WritePolicy::ProxyToPrimary)` (`write_gate.rs:179`), тоді як політика самої ТАБЛИЦІ `prro_queue_items` лишилась `LocalOutbox` (`write_gate.rs:111`); решта цього класу тепер додатково захищена рубцем 1 (`store_ctx.rs:147` → SQLSTATE 25006) і рубцем 2 (`readonly_net.rs:87`) — §11.8.

**`return_invoice` — повний стек (була АНОМАЛІЯ §11.7.9.7 фази 3.3a).** Тип
`TYPE_RETURN_RECEIPT` (`sync_push.rs:50`) — це **чек повернення ПОКУПЦЯ**
(`sync.rs::receiver_table` → `receipts`, диспетчер `accept_receipt_kind`), тож
для повернення **ПОСТАЧАЛЬНИКУ** додано ОКРЕМИЙ тип `return_invoice`
(`transactions.rs::TYPE_RETURN_INVOICE`) з локальною таблицею
`return_invoices` (offline-0012) і приймачем-сервісом
(`ReturnInvoicesService`: draft → confirm = stock −qty + `supplier_ledger` +
борг постачальнику; `client_uuid` у `return_invoices.client_uuid`, ідемпотентність
при повторному push). Це той самий підхід, що для `invoice`
(`accept_invoice_kind`): на primary діє ОДНА бізнес-логіка — сервісна, а не
другий SQL-приймач.

**`debtor_payment` — рішення NIKO §11.6.4 ВАРІАНТ 1 (`LocalOutbox`).**
Боргові сутності мають рядок політики (§11.1), оплата створюється офлайн:
агрегат → `debtors_ledger` (0006), похідний локальний борг → `debtor_balances`
(0012), приймач → `INSERT debtor_payments` + `UPDATE debtors.total_debt` в ОДНІЙ
транзакції. Свідома **відміна від Python** (`SqlxDebtors::pay`): повне погашення
НЕ видаляє боржника (`DELETE FROM debtors` → каскад зносить рядок оплати, і
повторний push не знайшов би `client_uuid`); борг лишається `0.00`, історія
оплат ціла. `create`/`update` боржника — відмова-як-клас (§11.6.2): у черзі
немає дії над довідником, ключа ідемпотентності в нього немає.

**`supplier_ledger` — самостійний документ, а не похідна накладної.**
`POST /api/v1/ledger` і `POST /api/v2/ledger/entries` створюють РУЧНИЙ запис
книги (`operation_type` + `amount` + `notes`), тож «увійти в ефект документа» не
можна — потрібен власний тип. `balance_after` локально = баланс репліки +
сума несинхронізованих записів каси (0.00 → та сама арифметика, що в приймачі
`SUM(amount) + amount`).

**`prro_queue_items` → межа `ProxyToPrimary` (АНОМАЛІЯ 4).**
`POST /api/v2/prro/fiscal/sync` і `POST /api/v2/prro/sync` виконують
`SyncOfflineQueueUseCase::sync`: дістають чеки з ПГ-черги `prro_queue_items`,
відправляють їх у **ДПС через gRPC sendChkV2 з КЕП-підписом** і лише потім
роблять `UPDATE prro_queue_items SET status='sent'` + `INSERT receipts`.
Це не «локальна черга каси», а **черга фіскальної зміни вузла, який тримає
КЕП-ключ**: (а) без живого зв'язку з ДПС операція неможлива фізично — саме тому
вона і є «sync»; (б) фіскальний чек має бути виданий РІВНО ОДИН раз, а
`KEY_LAST_MAC_NUMBER`/номер чека зміни — глобальний стан (`prro_shifts`,
політика вже `ProxyToPrimary`, §11.1); дублювання фіскалізації на другому вузлі
дало б подвійні чеки в ДПС. Тому клас поверхні на standby — прохід на primary
(§11.2) / відмова користувачу людським текстом, а НЕ SQLite-черга каси: локально
каса не має ні КЕП-ключа, ні права фіскалізувати чужу зміну. **Класифікація
`write_gate.rs` ЗМІНЕНА (Фаза 1 фіксу):** обидві поверхні мають власну поверхневу
сутність `prro_sync` = `ProxyToPrimary` (`write_gate.rs:179` — рядок `POLICY_TABLE`,
`:502` — `classify_request`), політика самої ТАБЛИЦІ `prro_queue_items` лишається
`LocalOutbox` для DML-точок `prro/repository.rs`; guard
`tests/write_gate_guard.rs::prro_sync_surfaces_are_proxy_to_primary_not_local_outbox`
фіксує саме це (було `Pass` → запис у read-only репліку → 400 із сирим текстом PG).
Межу зафіксовано тут як свідоме рішення, а не борг.

**`heartbeat` (§11.7.9.8) закрито у Фазі 3.3b**: `store_context.rs` на standby
більше не робить `UPDATE devices` у репліку → `standby_heartbeat::record_device_seen`
пише в SQLite `device_heartbeats` (offline-0013), помилки лише логуються;
авторитетне `devices.last_seen_at` пише primary.

#### 11.7.9.8 ВИПРАВЛЕНО (Фаза 3.3b): внутрішня write-точка heartbeat

`api/store_context.rs:108`: `UPDATE devices SET last_seen_at = now()` на КОЖЕН
запит пристрою (store-middleware), пул — `state.store_pool` = на standby це
локальна **репліка**. Помилка лише логується (`store_context.rs:113`), тому
`500` не виникав, але серцебиття в PG на standby не писалося і кожен запит давав
рядок помилки в лог.

**Закрито**: на standby middleware НЕ йде в PG, а пише локальний SQLite-журнал
`device_heartbeats` (offline-міграція 0013) через
`standby_heartbeat::record_device_seen` (у `spawn_blocking`, помилки лише
логуються). Авторитетне `devices.last_seen_at` пише primary — саме туди йде
device-авторизація. Точка належить шару локальної копії §11.7.4
(`is_local_sqlite_layer`: `offline/**`, `standby_heartbeat.rs`), тому політики PG
не потребує. Поверхня не є маршрутом, тому в 137 не входить.

#### 11.7.9.9 Критерій прийняття Фази 3.2

* 137/137 write-поверхонь мають явний клас; `classify_request → None` = 0 → ✅
  (guard `every_write_route_has_explicit_class`, 9 тестів guard'а, 0 failed);
* кожен `ProxyToPrimary`-маршрут реально проходить pass-through: 63 шляхи → `503
  §4` + маркери, жодного `500` (`write_gate_behavior`, 5 passed) → ✅;
* друга лінія `admin_pool` під гейтом: 9/9 літералів мають політику → ✅;
* жодного нового механізму, жодного адаптера, `SqlxPos`/`OutboxPos` не змінені → ✅.

#### 11.7.11 ФАЗА 3.8: безпека черги при `promote` (ґейт push + drain)

Аномалія, знайдена NIKO: `promote` робив вузол primary, але **не** вимикав HTTP-push
(мета даних «push вимкнено» жила лише в `node_config`, а ціль береться з SQLite
`server_url`), і **не мав шляху** застосування залишку SQLite-черги до власного PG.
Наслідок: залишок черги йшов на мертвий старий primary (ризик split-brain), а
офлайн-продажі лишались у SQLite назавжди.

| Що | Рішення | Файл:рядок |
|---|---|---|
| Ґейт HTTP-push | `mode == Primary` **І** апстріму немає (`[node] primary_db_url`/`upstream_write_url` очищені, а розв'язаний primary — «посилання на себе») → 0 HTTP, `Ok(sent=0, gated=true)`, лог раз на процес | `offline/sync_push.rs:507` (`push_pending_batch_with_node`), прод-обгортка `:483`; предикат `node_config.rs:246` (+`has_configured_upstream` `:218`, `is_self_local_url` `:539`, `load_explicit` `:273`) |
| Норма не зламана | `mode=Standby` і `mode=Primary` **із** апстрімом (standalone POS на віддалений сервер) пушать як раніше; без ЯВНОЇ секції `[node]` ґейт не діє | тести `offline/sync_push.rs:1548`, `:1524`, `:1567` |
| Прозорість для оператора | `sync_now` → `gated: true` + `gate_reason` | `offline/commands.rs:518`, `:560` |
| Drain черги у власний PG | `pending_outbox` (FIFO) → `PushEnvelope` → **наявне ядро** `sync::process_push_item` (без дублювання apply); `created`/`already_exists` → `mark_done`, `error` → лишається `pending`; RLS-контекст = точка документа | `route_local.rs:689` (`drain_local_outbox`), ядро `sync.rs:564`, `mark_done` `offline/sync_push.rs:729` |
| Поверхня | `POST /api/v1/local/outbox/drain` — owner-only (`require_owner_offline`), без store-middleware (DR: він ходить у недоступний primary-пул), монтується ЗАВЖДИ (потрібен і після рестарту, коли `mode=Primary` і локальних маршрутів немає); клас `LocalOutbox` | `route_local.rs:800`, `:822`; `router_v1.rs:824`; `write_gate.rs:166`, `:323` |
| Автоматизм | `promote` крок 7 викликає drain (best-effort; підсумок у `outbox_drain`, помилка не валить promote) | `promote.rs:246`, `:270` |
| Ідемпотентність | повторний drain → `already_exists` (0 дублів) | `tests/promote_drain_e2e.rs:500` |
| E2E доказ | накладна офлайн → `promote` → агрегат у власному PG (`invoices`+`invoice_items`+`stock 3.000`), `pending_outbox` = 0, 0 HTTP до старого сервера | `tests/promote_drain_e2e.rs:371` |

**Межі (свідомі):** (1) `mode=Primary` **без** апстріму більше не пушить — якщо в
майбутньому з'явиться топологія «локальна БД + HTTP-sync на інший вузол», їй
потрібен **явний** апстрім у `[node]`; (2) drain застосовує агрегати від імені
власника (`claims.sub`), бо SQLite-черга не зберігає касира — те саме обмеження,
що й у device-режимі push (касир = sub токена); (3) `repoint-primary` не оновлює
SQLite `server_url` — окрема тема (АНОМАЛІЯ звіту Фази 3.8).

#### 11.7.10 Контрольні підсумки

| Клас | Таблиць | Точок |
|------|---------|-------|
| `LocalOutbox` (§11.1 рядки 1–3 + §11.6) | 22 | 169 |
| `ProxyToPrimary` (§11.1 рядок 4) | 18 | 89 |
| `DisabledOnStandby` (§11.1 рядки 5–7) | 3 | 6 |
| `satellite` (успадковують батька) | 7 | 37 |
| динамічна таблиця (§11.7.7) | 1 | 1 |
| **Разом PG-шар** | **43** | **265** |
| шар локальної SQLite (виняток) | 21 | 70 |
| приймачі `sync.rs`/`sync_receivers.rs` (виняток §11.4 п.1) | — | 13 |
| `price_tags` (рядок реєстру без DML) | 1 | 0 |
| з них **ВІДКРИТО (Фаза 3)** | 21 | 117 |

Критерії прийняття:

* guard бачить УСІ 6 крейтів: 141 файл, 335 DML-рядків → ✅;
* 100 % таблиць PG-шару мають політику або батька: 43/43 → ✅;
* `POLICY_TABLE` = **58** рядків (25 гейт-сутностей: 23 §11.1 + 2 §11.6; 28 таблиць PG-шару §11.7; 5 поверхневих §11.7.9) — звіряється тестом `write_gate_guard.rs::pg_table_registry_is_complete_and_consistent` / `every_policy_entity_is_covered_by_adr_registry_or_document_channel` → ✅;
* два нові негативні контролі (infrastructure-файл без класу; `satellite`/динамічна/шар SQLite) → ✅ (7 тестів, 0 failed);
* **Фаза 3.2 (§11.7.9)**: 137/137 write-поверхонь мають явний клас, `classify_request → None` = 0 (було 47); guard 9 тестів, 0 failed; pass-through верифіковано на 63 шляхах (`write_gate_behavior`, 5 passed) → ✅;
* жодного виправлення коду Фази 3 (адаптерів) — лише карта + запобіжник → ✅.
* **Фаза 3.3a (§11.7.9.7)**: 19 із 31 поверхні закрито адаптерами (`outbox_invoices.rs`, `outbox_purchase_orders.rs`, `outbox_write.rs` + спільна обв'язка `outbox_local.rs`), `lib.rs` вибирає адаптер раз на старті за `node_cfg.mode`; e2e: `invoice_standby_outbox_e2e` (2, включно з push на primary) + `purchase_order_standby_outbox_e2e` + `inventory_standby_outbox_e2e` → ✅
* **Фаза 3.3b (§11.7.9.7)**: 12 поверхонь закрито — `return_invoice` (4, ПОВНИЙ стек: тип + локальний агрегат 0012 + stock −qty + приймач-сервіс + UNIQUE 0018), `debtor_payment` (4, §11.6.4 варіант 1: `debtors_ledger` + `debtor_balances` + приймач `debtor_payments`/`UPDATE debtors`), `supplier_ledger` (2, самостійний документ: `supplier_ledger` + `supplier_balances` + приймач з `SUM(amount)+amount`); heartbeat `store_context.rs:108` закрито SQLite-каналом (§11.7.9.8); e2e `return_invoice_standby_outbox_e2e` (2, включно з push на primary) + `debtor_standby_outbox_e2e` (2, включно з ідемпотентним push) → ✅
* **межа, а не борг (§11.7.9.7)**: `prro_queue_items` (2 поверхні `sync`) — фіскалізація через ДПС із КЕП-ключем вузла: `ProxyToPrimary`/відмова; SQLite-черга каси тут неможлива (подвійна фіскалізація зміни) → 🚫

### 11.8 Генеричний перехоплювач запису в read-only репліку (Фаза 2)

Рубіж 1 — **ручна** таблиця політик (`write_gate::classify_request`, §11.1): вона
описує світ, а світ змінюється швидше. Рубіж 2 — **генеричний** перехоплювач,
який не знає ні назв таблиць, ні маршрутів і спрацьовує на самому факті фізичної
відмови PostgreSQL (`read_only_sql_transaction`).

#### 11.8.1 Проблема: таблиця політик завжди відстає (3 інциденти)

| # | Інцидент | Що показав | Де закрито / описано |
|---|---|---|---|
| 1 | **чек** — друга реалізація запису чека (`POST /api/v1/local/receipts` + `kind: "receipt"` в `/api/v1/local/ops`) жила паралельно канонічному маршруту | класифікація знала про один вхід, а писав інший | §11.5: дубль видалено, `400` + вказівка на канонічний маршрут |
| 2 | **аудит DML усіх крейтів** (`write_gate_guard`: 141 файл / 335 DML-рядків, §11.7) | виявив цілий клас, якого таблиця не знала — **21 таблиця / 117 точок** «ВІДКРИТО (Фаза 3)» | §11.7.8 (карта для Фази 3) |
| 3 | **ПРРО** — клас таблиці оголошений `LocalOutbox`, а HTTP-хендлер ішов у PG через mode-agnostic сервіс | **31 поверхня** з сирим `500` на standby | §11.7.9.7: Фази 3.3a/3.3b закрили 29 адаптерами, 2 — свідома межа (поверхня `prro_sync`) |

Спільний корінь усіх трьох: безпека трималась на **перелікові, який треба
пам'ятати**. Фаза 2 прибирає цю залежність: безпеку дає фізика БД, а не список;
ручна таблиця лишається картою дрейфу й швидкою смугою в CI (§11.8.8).

#### 11.8.2 Рубіж 1 — фунел пулу: `Executor for &StorePool` (SQLSTATE 25006)

| Що | Де |
|---|---|
| `impl<'p> Executor<'p> for &'_ StorePool` — ЄДИНЕ фізичне місце, через яке проходять усі одиночні запити репозиторіїв (`query(...).fetch_*` / `execute` з `&self.pool`) | `crates/torgashka-infrastructure/src/store_ctx.rs:147` |
| перекрито `fetch_many`, `fetch_optional`, `prepare_with`, `describe`; `fetch_one` / `execute` — дефолтні методи трейту, ідуть через `fetch_many` | `store_ctx.rs:150`, `:190`, `:216`, `:232` |
| інваріант зафіксовано в коді: ручна таблиця політик більше **не є обов'язковою умовою безпеки** | `store_ctx.rs:133-141` |
| ловиться SQLSTATE **25006** (`read_only_sql_transaction`) — сигнал **бібліотеки** (sqlx), не текст помилки | `store_ctx.rs:179`, `:208`, `:227`, `:242` → `readonly_guard::normalize_with_sql` |
| константа коду | `readonly_guard.rs:36` (`SQLSTATE_READ_ONLY`) |
| типізований маркер `ReadOnlyReplicaError`, `impl DatabaseError`: `code() == "25006"`, `kind() == ErrorKind::Other` (у sqlx 0.8 немає варіанта `ReadOnly` — єдиний сигнал класу це SQLSTATE) | `readonly_guard.rs:91`, `:104`, `:110`, `:114` |
| `Display` = `MARKER + HUMAN_MESSAGE`; сирий текст PostgreSQL у `Display` НЕ потрапляє (лише stderr під час нормалізації) | `readonly_guard.rs:43` (`[READ_ONLY_REPLICA]`), `:98` |
| `is_read_only_replica` — два незалежні сигнали: SQLSTATE 25006 АБО downcast маркера (коли помилку завернув інший шар) | `readonly_guard.rs:145` |
| метрики: `hits()`, `fallback_hits()`, `sanitized_hits()`, `last_hit()`, `hits_by_fingerprint()` (топ-20) | `readonly_guard.rs:274`, `:280`, `:289`, `:309`, `:299` |
| fingerprint = SQL із підстановкою `?` замість літералів, стиснуті пробіли, обрізано до 80 символів | `readonly_guard.rs:157`, `:164` |
| `db_error_class` — машинний клас для діагностики іншого вузла: `[DB_ERROR <sqlstate>]`, без SQLSTATE — `[DB_ERROR]` | `readonly_guard.rs:75`, `:78` |

#### 11.8.3 Рубіж 2 (останній) — middleware `readonly_net`

`readonly_net_middleware` (`crates/torgashka-api/src/readonly_net.rs:87`)
змонтований у `router_v1.rs:842` **зовнішнім** щодо `write_gate::gate_middleware`
(`router_v1.rs:833`), тобто бачить уже сформовану відповідь хендлера.

Умови втручання:

| Умова | Значення | Де |
|---|---|---|
| метод | write-методи (`write_gate::is_write_method`); читання не інспектується НІКОЛИ | `readonly_net.rs:93` |
| режим | `standby` **АБО** `readonly_guard::hits() > 0` (пули фактично read-only, хоч `mode` ще `primary`: невдалий promote / кабель у репліку) | `readonly_net.rs:107` |
| розмір тіла | інспектуються ЛИШЕ тіла відомого розміру, ≤ 64 КіБ; стрімінгові та більші не читаються | `readonly_net.rs:56` (`MAX_INSPECT_BYTES = 64 * 1024`), перевірка `:114` |

Дві сигнатури (в порядку пріоритету):

| # | Сигнатура | Дія |
|---|---|---|
| 1 | маркер `[READ_ONLY_REPLICA]` у тілі (пошук побайтовий, без припущень про UTF-8) | тіло → `503` за контрактом §4 (`write_gate::standby_503`, `write_gate.rs:562`) + `X-Torgashka-Node-Mode: standby` (`readonly_net.rs:208-214`) + `X-Torgashka-Upstream: down` (`write_gate.rs:239`, `:569`) + `Retry-After: 30` (`write_gate.rs:241`, `:243`, `:570`); метрика `record_fallback()` (`readonly_guard.rs:332`) |
| 2 | префікс `Display` помилки БД sqlx `error returned from database: `, жорстко прив'язаний до `sqlx-core-0.8.6/src/error.rs:44` | тіло → `{"detail": "помилка бази даних: деталі приховано (див. журнал сервера)"}` (`readonly_net.rs:74`), **статус хендлера збережено** (`400` лишається `400`, `500` — `500`), додано `X-Torgashka-Sanitized: db-error` (`readonly_net.rs:67`, `:70`), сирий фрагмент → stderr з throttle 1 с (`:80`); метрики `record_sanitized()` (`readonly_guard.rs:326`), `last_sanitized()` (`:294`) |

Тіла без обох сигнатур повертаються **байт-у-байт** (`readonly_net.rs:157`; тести — §11.8.7).

#### 11.8.4 Видимість дрейфу: `GET /api/v1/local/status`

Поле `readonly_net` — **адитивне**: маршрут `route_local.rs:912`, поле `route_local.rs:396`, реалізація `readonly_net_status()` — `route_local.rs:401`.

| Поле | Зміст |
|---|---|
| `funnel_hits` | скільки разів рубіж 1 спіймав 25006 (`guard::hits()`) |
| `fallback_hits` | скільки разів рубіж 2 переписав відповідь на `503` §4 |
| `sanitized_hits` | скільки разів рубіж 2 санував сирий текст БД (гілка 2) |
| `last` | `{fingerprint, at}` — останнє влучання фунела |
| `last_sanitized` | `{fingerprint, at}` — остання санація |
| `top` | `[[fingerprint, n], …]` — топ-20 за кількістю (спадання) |

#### 11.8.5 Санація сирого тексту БД (клас A) — окремий рубіж

16 витоків сирого тексту PostgreSQL у тіла відповідей (**клас A**) закрито в **12 файлах**: `categories_v2.rs`, `crud.rs`, `readdirs.rs`, `print_templates.rs`, `products_v2.rs`, `documents.rs`, `setup.rs`, `suppliers.rs`, `route_local.rs`, `promote.rs`, `sync.rs`, `admin_db_sources.rs` — `docs/audit/db_error_leak_audit.md:106`.

Правило: сирий текст → `pg_log`/stderr; клієнту — людський текст; коли потрібна діагностика **іншого вузла** — машинний клас `[DB_ERROR <sqlstate>]` (`readonly_guard.rs:75`).

Метод аудиту й повний перелік: `docs/audit/db_error_leak_audit.md` (`to_string()` → 479 входжень; прод-виклики `api_err(` → 17, усі в `prro.rs`, клас A = 0 — `:75-76`).

#### 11.8.6 Що НЕ покрито (межі, свідомі)

| # | Межа | Наслідок / доказ |
|---|---|---|
| (a) | **Транзакції**: `StorePool::begin()` (`store_ctx.rs:73`) віддає `sqlx::Transaction` з виконавцем `&mut PgConnection` — це минає фунел | рубіж 1 бачить лише одиночні запити; такі випадки ловить ЛИШЕ рубіж 2 (межу описано в коді: `store_ctx.rs:142-146`) |
| (b) | тіла **> 64 КіБ** не інспектуються | `readonly_net.rs:56`, `:114` — тіло не читається, щоб його не зруйнувати |
| (c) | **GET-тіла** не інспектуються | рубіж 2 діє лише на write-методи (`readonly_net.rs:93`) |
| (d) | на вузлі **`primary`** рубіж 2 не втручається, **поки** фунел не зафіксував жодного 25006 (`!is_standby() && hits() == 0`) | `readonly_net.rs:107`; після першого влучання — втручається (тест `primary_node_with_read_only_pool_answers_503_not_raw_400`) |
| (e) | помилки **connect/IO** SQLx (напр. `admin_db_sources.rs:378`, `:382` — `ping_source`) не мають префікса `error returned from database: `, сигнатура відсутня | покриваються лише тим, що їх прибрано per-handler (`docs/audit/db_error_leak_audit.md:100`) |
| (f) | **«прозоре перекладення операції в SQLite-чергу на рівні пулу» — НЕ реалізовано і недосяжне** | пул не знає предметної семантики запиту (що це за документ, чи можна його відкласти, який `client_uuid`), тому автоматичний переклад у чергу живе там, де відома суть — в обробниках + ручній таблиці політик (§11.1). Рубежі 1/2 зупиняють запис і роблять відмову чесною, але **не** перетворюють її на відкладене виконання |

#### 11.8.7 Докази (без другого вузла)

| Факт | Доказ (`файл:рядок`) |
|---|---|
| справжній SQLSTATE **25006** відтворюється локально read-only сесією `PgConnectOptions::options("-c default_transaction_read_only=on")`; фізична репліка не потрібна | `crates/torgashka-infrastructure/tests/readonly_guard.rs::real_postgres_read_only_session_raises_25006_and_is_normalized` (`:236`, опція — `:250`) |
| нормалізація, маркер, класифікація SQLSTATE | `readonly_guard.rs::sqlstate_25006_is_read_only_other_codes_are_not` (`:83`), `::normalize_replaces_plain_pg_error_and_keeps_marker_without_pg_text` (`:106`), `::typed_error_exposes_sqlstate_and_marker` (`:159`), `::db_error_class_25006_contains_sqlstate_without_pg_text` (`:192`) |
| гілка 1: маркер → `503` за контрактом §4, без тексту PG | `crates/torgashka-api/tests/readonly_net_e2e.rs::marker_body_is_rewritten_to_503_contract_without_pg_text` (`:228`) |
| гілка 2: `400`/`500` сануються, **статус хендлера збережено**; маркер має пріоритет | `::sqlx_db_error_prefix_status_400_is_sanitized_without_pg_text` (`:477`), `::sqlx_db_error_prefix_status_500_keeps_handler_status` (`:525`), `::read_only_marker_takes_priority_over_db_error_prefix` (`:551`) |
| тіла без сигнатур — байт-у-байт | `::bodies_without_marker_are_byte_identical` (`:272`), `::bodies_without_either_signature_stay_byte_identical` (`:592`) |
| читання та тіла > 64 КіБ не інспектуються | `::reads_and_oversized_bodies_are_never_inspected` (`:313`), `::reads_and_oversized_db_error_bodies_are_never_sanitized` (`:619`) |
| сценарій «гейт не знав про маршрут»: пул read-only при `mode = primary` → `503`, не сирий `400` | `::primary_node_with_read_only_pool_answers_503_not_raw_400` (`:371`) |
| поле `/local/status` адитивне та коректне | `crates/torgashka-api/src/route_local.rs::readonly_net_status_is_additive_and_well_formed` (`route_local.rs:968`) |

🕐-позначки в таблиці §5 (докази, що вимагають рядків `postgres.log` **репліки**) лишаються чинними для свого предмета; рубіж 1/2 закриває той клас доказів, який раніше вимагав фізичної репліки.

#### 11.8.8 Як це виявляє дрейф

Дрейф таблиці політик більше не «тихий»: якщо додано новий шлях запису, якого
таблиця не знає, то на standby (або на вузлі, де пули фактично read-only)
відповідь дасть маркер `[READ_ONLY_REPLICA]` чи префікс sqlx — і це **збільшить
`funnel_hits` / `fallback_hits` / `sanitized_hits` у `GET /api/v1/local/status`**.
Тобто борг видно в моніторингу, а не зі скарги користувача. Швидкою смугою
лишається статичний guard `write_gate_guard::every_write_route_has_explicit_class`
(`crates/torgashka-api/tests/write_gate_guard.rs`) — він ловить це в CI **до**
релізу; рубежі 1/2 — страховка в рантаймі на те, чого статичний guard не бачить
(транзакції, більші тіла, точки поза маршрутами).

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
