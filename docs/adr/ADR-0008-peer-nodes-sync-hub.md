# ADR-0008: Рівноправні read-write вузли + центральний хаб синхронізації (відмова від фізичної реплікації)

| Поле | Значення |
|---|---|
| Статус | **Прийнято** (рішення Творця) |
| Дата | 2026-09-12 |
| Замінює | ADR-0007 (standby-write-routing) — рубежі 1/2 стають непотрібними; сам ADR-0007 → Superseded |
| Зачіпає | `torgashka-api`, `torgashka-infrastructure`, ЕТАП 15–20 (фізична реплікація), docs/operations/* |
| Тип зміни | Архітектурна модель мережі (не рефакторинг) |

---

## 1. Контекст

Поточна мережева модель (ЕТАП 15–20, `docs/system_documentation/sources/network-replication-etap15-20.md`)
побудована на **фізичній реплікації PostgreSQL**:

* вузол-«primary» — єдиний автор запису; вузол-«standby» — фізична hot-standby копія
  (`pg_basebackup` + стрімінг WAL), **read-only за визначенням**;
* код провіжинінгу: `frontend/src-tauri/crates/torgashka-infrastructure/src/standby_provision.rs:1-30`
  (`pg_basebackup -h <primary> … -X stream -C -S <slot> -R`, `standby.signal`, `pg_is_in_recovery()`);
* режим вузла: `crates/torgashka-infrastructure/src/node_config.rs:30-41` (`NodeMode::{Primary, Standby}`);
* наслідок для застосунку: «standby не може писати» → з'явився гейт запису
  (`crates/torgashka-api/src/write_gate.rs`), перехоплювач відмов репліки
  (`crates/torgashka-api/src/readonly_net.rs` + `crates/torgashka-infrastructure/src/readonly_guard.rs`),
  DR-операції promote/repoint (`crates/torgashka-api/src/promote.rs:1-16`), реєстр реплікаційних слотів
  у `network_nodes` (`crates/torgashka-api/src/network_nodes.rs`, поля `replication_role_name`,
  `replication_slot_name`, `replication_lag_bytes` — перевірено в БД, `pos_system_fresh_test`).

**Рішення Творця (мотивація, зафіксована як є):**

> Standby за визначенням read-only → вузол не може писати → ламає роботу каси.
> Рішення: кожен магазин — звичайний повністю read-write PostgreSQL. Концепція
> «чи можу я писати» прибирається на рівні вузла повністю. Додатково: одна БД на
> постійно ввімкненому сервері (для перестраховки), яка виконує основну роль
> синхронізації між БД магазинів. **УСІ ВУЗЛИ РІВНОЦІННІ.**

Отже: фізична реплікація скасовується; замість ієрархії «primary → read-only копії» —
**мережа рівноправних read-write вузлів** + **хаб** (окрема завжди-ввімкнена БД), який
виконує роль центральної точки синхронізації та арбітра спільних даних.

Синхронізація в продукті вже існує і вона — **прикладного рівня** (див. §4.1);
фізична реплікація використовувалась як ТРАНСПОРТ КОПІЇ, а не як механізм синку даних.

---

## 2. Рішення

1. **Кожен вузол мережі — самостійний read-write PostgreSQL.** Вузол не має режимів;
   концепція `NodeMode::Standby`, гейт «чи можу я писати», 503-контракт §4, promote/repoint —
   прибираються (перелік у §7).
2. **Джерело істини для операційних даних точки — сам вузол** (single-writer per store):
   документи, чеки, рухи, борги, ПРРО-стан точки пише ЛИШЕ вузол, якому вони належать.
3. **Хаб (завжди-ввімкнена БД на сервері) — центр синхронізації:**
   * приймає операційні дані вузлів (node → hub);
   * роздає спільні довідники (hub → node);
   * арбітрує спільні сутності (products/suppliers/users/stores/… — §5).
4. **Транспорт — наявний прикладний HTTP-протокол** (`POST /api/v1/sync/push`,
   `GET /api/v1/sync/master`), без WAL, без логічної реплікації (§4.1, §6).
5. **Ідемпотентність і порядок — наскрізні:** `client_uuid` + partial UNIQUE, `sync_batch_id`
   для батчів, класи помилок retryable/terminal (§4.3).

---

## 3. Наслідки

### 3.1 Позитивні

| # | Наслідок | Чому |
|---|---|---|
| 1 | Каса/магазин **завжди може писати** — головна мета рішення | вузол read-write, немає жодного режиму/routing-гейта |
| 2 | Зникає цілий клас відмов «standby не може писати» і супутній механізм (гейт, 503, readonly-перехоплювач, promote/DR-раннбук) | код видаляється → менше поверхні помилок |
| 3 | Немає вимог до конфігурації PG (WAL-стрімінг, слоти, `wal_level`, `listen` для реплікації) | синк — HTTP, не WAL (§6) |
| 4 | Синхронізація стає **тестовною локально** (один PG + HTTP), без другого фізичного вузла | наявні e2e вже так і працюють (напр. `tests/sync_push_e2e.rs`) |
| 5 | Хаб можна тримати як «звичайний» одиночний PG (без DR-жонглювання ролями) | роль хаба — дані + арбітраж, не WAL-джерело |
| 6 | Модель проста для оператора: «наша БД завжди наша, хаб — для обміну» | прибирає 4 стани вузла (primary/standby/repoint/promoted) |

### 3.2 Негативні (приймаються свідомо)

| # | Наслідок | Мітигація |
|---|---|---|
| 1 | **Конфлікти на спільних сутностях** (каталог/ціни/користувачі) — кілька вузлів можуть правити одне | політика «хаб — арбітр» §5 (пропозиція → версія → роздача) |
| 2 | **Порядок агрегатів (FK) не гарантований**: дитина може приїхати раніше за батька → відмова приймача | §4.3: `sync_batch_id` + топологічне сортування + retryable-клас для FK |
| 3 | Хаб — **SPOF для обміну** (але не для роботи магазину: вузол пише локально) | хаб на постійно ввімкненому сервері + стандартний бекап/replica-копія хаба поза протоколом синку |
| 4 | Дані дублюються (кожен вузол має повну копію спільних довідників + свою операційну частину) | це ціна автономності; обсяг каталогу/документів точки малий |
| 5 | Немає глобальної транзакції мережі (немає «однієї правди в один момент») | узгодженість — eventual; для каси критична лише локальна транзакція (вона атомарна) |
| 6 | Зворотний потік (hub → node) операційних даних інших точок відсутній → зведені звіти «по мережі» живуть на хабі | звіти мережі виконуються на хабі, а не на вузлі (явно зафіксовано) |

---

## 4. Ключові питання (відповіді з доказами)

### 4.1 Питання №1: синхронізація — прикладний HTTP-протокол чи PG-реплікація?

**Відповідь: прикладний HTTP-протокол. PG логічна реплікація НЕ потрібна. `wal_level` змінювати НЕ потрібно.**

| Твердження | Доказ (файл:рядок) |
|---|---|
| Push-ендпоінт приймає JSON-агрегати каси | `crates/torgashka-api/src/router_v1.rs:178` (`/api/v1/sync/push` → `post(sync::push)`); `crates/torgashka-api/src/sync.rs:422-447` (масив агрегатів, ≤50) |
| Pull-ендпоінт віддає дельти довідників | `router_v1.rs:173` (`/api/v1/sync/master` → `get(sync::master)`); `sync.rs:6-17`, `:117-249` |
| Версіонування — таблицею `sync_meta` + per-row `server_version` | `sync.rs:9-13`; Alembic `backend/alembic/versions/0012_server_version_columns.py:1-20`; тригери в БД: `trg_products_bump`, `trg_categories_bump`, `trg_suppliers_bump`, `trg_users_bump`, `trg_system_settings_bump` (перевірено `pg_trigger` на `pos_system_fresh_test`) |
| Ідемпотентність — `client_uuid` + partial UNIQUE на приймачах | Alembic `0013_sync_push_idempotency.py:1-20`; фактичні індекси в БД: `uq_receipts_client_uuid`, `uq_invoices_client_uuid`, `uq_purchase_orders_client_uuid`, `uq_inventories_client_uuid`, `uq_transfers_client_uuid`, `uq_write_offs_client_uuid`, `uq_debtor_payments_client_uuid`, `uq_work_sessions_client_uuid`, `uq_return_invoices_client_uuid`, `uq_cash_operations_client_uuid` (10 шт., `pg_index`) |
| Кожен прийом логується | `sync.rs:1253-1276` (`log_sync` → `INSERT INTO sync_log … direction='push'`) |
| Клієнт — HTTP-клієнт каси | `crates/torgashka-infrastructure/src/offline/sync_push.rs:15-22` (POST + per-item розбір), `offline/sync_pull.rs:33-38` (перелік сутностей pull) |
| **Жодного артефакту логічної реплікації** | `grep -rn "pg_logical\|logical_replication\|CREATE PUBLICATION\|CREATE SUBSCRIPTION\|pglogrepl\|wal_level" frontend/src-tauri/crates/ backend/app/` → **0 збігів** |
| Фізична реплікація існує, але як ОКРЕМИЙ трек (ЕТАП 15–20), не як транспорт синку | `standby_provision.rs:5-30` (`pg_basebackup`), `promote.rs:84-87` (`pg_promote`, `pg_replication_slots`) — саме цей трек скасовується цим ADR |

**Висновок:** синк читає звичайні таблиці (`sync_meta`, `sync_log`, документи) і ходить по HTTP.
WAL не читається ніде. Логічна реплікація не потрібна; фізична реплікація не потрібна теж.
→ **`wal_level` не змінюємо** (див. §6).

### 4.2 Питання №2: спільні таблиці без `store_id` — хто власник?

**Відповідь: хаб — авторитет для спільних сутностей. Вузол надсилає ПРОПОЗИЦІЮ; хаб арбітрує версію і роздає.**

Факти, що визначають політику:

| Факт | Доказ |
|---|---|
| Спільні (без `store_id`): `products`, `suppliers`, `users`, `stores`, `write_off_reasons`, `owners_db`, `network_nodes`, `network_events`, `sync_meta`, `schema_revision`, `ddl_markers`, `at_phase22_probe` | `information_schema.columns` на `pos_system_fresh_test` (store_id=0) |
| **Операційні таблиці FK-залежать від спільних** — 85 FK, з них: `invoice_items→products`, `receipt_items→products`, `purchase_order_items→products`, `stock→products`, `invoices→suppliers`, `purchase_orders→suppliers`, `supplier_ledger→suppliers`, `receipts→users`, `work_sessions→users`, `debtors→stores`, `*→stores` | `pg_constraint` (85 рядків), напр. `invoice_items | products` |
| Ціна товару лежить у **спільній** `products.price` і вже роздається вузлам | `sync.rs:278-311` (`SELECT … price … FROM products` → delta `products`) |
| Tombstone вже є саме на спільних: `products`, `suppliers`, `users`, `categories` | `is_deleted` (перевірено в БД); Alembic `0011_sync_server_schema.py:16-19` (soft-delete) |
| Версійні тригери вже є саме на спільних: `products`, `suppliers`, `users`, `categories`, `system_settings` | `pg_trigger` (5 шт.) |
| Поточна політика гейта для них — «писати в primary» (підтверджує: це не дані точки) | `write_gate.rs:72` (`stores`), `:126` (`products`), `:127` (`categories`), `:130` (`suppliers`), `:141` (`write_off_reasons`); `users` — `:123` |

**Обрана політика (hub-as-authority, proposal-протокол):**

1. Вузол пише локально (offline-first не ламається): новий/змінений каталог отримує
   локальний `server_version` і статус «не підтверджено» (нова колонка, §7.1-A).
2. Вузол надсилає **пропозицію**: `catalog_change_requests(entity, row_id, op, payload, client_uuid, store_id)`.
3. Хаб арбітрує: присвоює **єдиний** `server_version` (наявний `bump_sync_version()`/`trg_*_bump`),
   для `op=delete` ставить **tombstone** `is_deleted=true` (механізм уже є), позначає пропозицію
   `accepted` / `conflict`.
4. Хаб роздає результат усім вузлам через наявний pull (`server_version > since_version`) —
   вузли застосовують як звичайну дельту (`offline/sync_pull.rs:183-350`).
5. Конфлікт (дві пропозиції на один рядок) вирішує хаб: детерміноване правило
   `(higher server_version / явний пріоритет) >> (час створення пропозиції)`, обидві пропозиції
   лишаються в журналі → конфлікт видимий, а не «злитий» мовчки.

**Чому це безпечніше за альтернативи:**

| Альтернатива | Чому відкинута |
|---|---|
| Last-write-wins за локальним часом вузла (peer-to-peer) | часові зсуви вузлів + мовчазна втрата правки; немає журналу конфлікту; ламає FK-цілісність (рядок існує на одних вузлах, ні — на інших) |
| Поділ за полями (напр. вузол — ціна, хаб — назва) | складність × кількість полів; не вирішує створення рядка (FK) |
| Заборона вузлам писати каталог (лише хаб-адмінка) | вбиває offline-first: менеджер точки не може додати товар, коли VPN/хаб недоступний |
| Один вузол-«власник каталогу» (як primary зараз) | це і є поточна модель, яка ламає роботу інших точок при недоступності власника |

Обрана політика дає: (а) локальну роботу завжди; (б) єдине джерело «канонічної» версії;
(в) tombstone-делеції вже підтримані; (г) конфлікти — видимі події, а не втрачені дані.

### 4.3 Питання №3: атомарність «документ + рядки» при частковому збої

**Відповідь: усередині агрегата — ВЖЕ вирішено. Між агрегатами — НЕ вирішено; потрібні `sync_batch_id` + топологічний порядок + retry-клас.**

Що вже вирішено (доказ — код):

| Механізм | Доказ |
|---|---|
| Кожен агрегат обробляється **окремою транзакцією**, помилка одного не валить пакет | `sync.rs:422-431` (коментар-контракт), `:500-545` (цикл `process_push_item`) |
| «Батько + діти» їдуть **в ОДНОМУ конверті** і пишуться **в ОДНІЙ транзакції** | `sync_receivers.rs:272-317` (PO + `purchase_order_items`), `:341-392` (inventory + items), `:440-487` (transfers + items), `:517-561` (write_offs + items), `:676-729` (debtor_payment + `UPDATE debtors`) — скрізь `pool.begin()` … `commit()` |
| Чек/накладна — через сервіс (агрегат цілком) | `sync.rs:686-1009` (`accept_receipt_kind`, `accept_invoice_kind`, `accept_return_invoice_kind`) |
| Діти **не мають власного kind** → окремо не подорожують | `sync.rs:661-682` (`receiver_table`: receipts, purchase_orders, inventories, transfers, write_offs, invoices, cash_operations, return_invoices, debtor_payments, supplier_ledger — жодної `*_items`) |
| Повтор завжди безпечний (ідемпотентність) | 10 partial UNIQUE на `client_uuid` (див. §4.1) |

Чого НЕ вирішено (доказ — код):

| Прогалина | Доказ | Наслідок |
|---|---|---|
| **Батько без власного kind**: `debtor_payments` приймається, але приймач ВИМАГАЄ наявний `debtors` | `sync_receivers.rs:680-692` («Боржника … не знайдено в цій точці — оплату відхилено»); kind `debtor` у `receiver_table` ВІДСУТНІЙ (`sync.rs:661-682`) | картка боржника, створена на вузлі, на хаб не доїжджає → **перший же платіж відхилено** |
| Між агрегатні FK: `purchase_orders→invoices`, `return_invoices→invoices` | `pg_constraint`: `purchase_orders | invoices`, `return_invoices | invoices` | якщо `invoice` ще не прийнятий — відмова |
| Порядок push — **хронологічний (FIFO), не топологічний** | `offline/sync_push.rs:263` (`ORDER BY id`), `:15-16` (FIFO) | дитина може піти раніше за батька |
| Будь-яка per-item `error` → **failed без retry** | `offline/sync_push.rs:571-579` (400/422 → failed), `:608-613` (status=error → `mark_failed`) | FK-відмова через порядок стає «потребує ручного втручання» назавжди |
| Немає ідентифікатора батча | `sync_log` має лише `client_uuid` (колонки перевірено в БД) | неможливо згрупувати/відкотити пакет, неможливо відрізнити «пакет у процесі» від «загублено» |

**Пропозиція (точково, §7.1):**

1. `sync_batch_id uuid` у `sync_log` + таблиця `sync_batches(status)`: клієнт штампує батч, сервер
   повертає його ж у відповіді; статус батча = сумарний (`accepted`/`partial`/`failed`).
2. Топологічний порядок у батчі за **декларованим DAG** (перелік «дитина → батько», побудований із
   наявних FK, напр. `debtor_payments→debtors`, `purchase_orders→invoices`, `*_items→parents`).
3. Машиночитний `error_class` у per-item відповіді: `RETRYABLE_FK` (SQLSTATE 23503) → `defer`
   (backoff), а не `failed`; `VALIDATION` → `failed` (як зараз); `CONFLICT` → окрема черга конфліктів.
4. Нові push-kinds: `debtor`, `work_session`, `prro_shift` (§7.1-B) — щоб батьки взагалі подорожували.

---

## 5. Матриця: таблиця → власник запису → напрямок синхронізації (усі 48)

Легенда власника: **В** = вузол-магазин (single writer), **Х** = хаб, **ХР** = хаб-реєстр мережі,
**Л** = локально-технічна (не синхронізується як бізнес-дані), **✖** = видалити.
Напрямок: **↑** node→hub, **↓** hub→node, **↕** обидва (пропозиція↔версія), **⛔** немає, **✖** немає (видалення).

| № | Таблиця | store_id | Власник | Напрямок | Доказ / примітка |
|---|---|---|---|---|---|
| 1 | `at_phase22_probe` | ні | ✖ | ✖ | тестова проба (integer PK), не бізнес-таблиця |
| 2 | `audit_log` | так | В (↑ на хаб — аудит мережі) | ↑ | `network.rs` пише події; централізований аудит — на хабі |
| 3 | `barcodes` | так | Х | ↓ | гейт: `write_gate.rs:129` (ProxyToPrimary); у pull-переліку НЕМАЄ → §7.1-C |
| 4 | `cash_operations` | так | В | ↑ | `sync.rs:672` (kind), UNIQUE `uq_cash_operations_client_uuid` |
| 5 | `categories` | так | Х (пропозиції з вузла) | ↕ | pull-сутність (`sync.rs:56`); `trg_categories_bump`; tombstone |
| 6 | `ddl_markers` | ні | Л | ⛔ | маркери застосованих DDL, локальні для кожної БД |
| 7 | `debtor_payments` | так | В | ↑ | `sync.rs:678` (kind); вимагає `debtors` на приймачі — `sync_receivers.rs:680-692` |
| 8 | `debtors` | так | В | ↑ | картка боржника створюється на вузлі; kind ВІДСУТНІЙ → §7.1-B (критично) |
| 9 | `devices` | так | ХР | ↓ | активація каси: `network.rs:337`; реєстр пристроїв — на хабі |
| 10 | `inventories` | так | В | ↑ | `sync.rs:665`; tx з items `sync_receivers.rs:341-392` |
| 11 | `inventory_items` | так | В | ↑ | усередині агрегата `inventory` (власного kind немає) |
| 12 | `invoice_items` | так | В | ↑ | усередині агрегата `invoice` (`sync.rs:762-883`) |
| 13 | `invoices` | так | В | ↑ | `sync.rs:669`; FK → `suppliers`, `users`, `stores` |
| 14 | `network_events` | ні | ХР | ⛔ | журнал подій вузлів: `network.rs:296` (`log_node_event`) |
| 15 | `network_nodes` | так | ХР | ⛔ | реєстр вузлів; `replication_*` поля → §7.2 (видалити) |
| 16 | `owners_db` | ні | Х | ⛔ | мапа owner→db (мета-рівень, не дані магазину) |
| 17 | `print_templates` | так | Х (каталог) → В (локальний оверрайд) | ↓ | шаблони друку точки; синку НЕМАЄ → §7.1-C |
| 18 | `product_images` | так | Х | ↓ | `write_gate.rs:128` (ProxyToPrimary); у pull НЕМАЄ → §7.1-C |
| 19 | `products` | ні | Х (пропозиції з вузла) | ↕ | pull `sync.rs:58`; `trg_products_bump`; tombstone; гейт `:126` |
| 20 | `prro_queue_items` | так | Л (фіскальна черга вузла) | ⛔ | межа ПРРО §11.7.9.7: фіскалізація — на вузлі |
| 21 | `prro_settings` | так | Л | ⛔ | налаштування ПРРО точки (КЕП вузла) |
| 22 | `prro_shifts` | так | В (аудит на хаб) | ↑ | зміни ПРРО; push-шляху НЕМАЄ → §7.1-B |
| 23 | `purchase_order_items` | так | В | ↑ | усередині агрегата `purchase_order` (`sync_receivers.rs:300`) |
| 24 | `purchase_orders` | так | В | ↑ | `sync.rs:664`; FK → `invoices` (порядок! §4.3) |
| 25 | `receipt_items` | так | В | ↑ | усередині агрегата чека (`sync.rs:686-761`) |
| 26 | `receipts` | так | В | ↑ | `sync.rs:663`; UNIQUE `uq_receipts_client_uuid` |
| 27 | `return_invoice_items` | так | В | ↑ | усередині агрегата (`sync.rs:884-1009`) |
| 28 | `return_invoices` | так | В | ↑ | `sync.rs:677`; FK → `invoices`, `suppliers` |
| 29 | `schema_revision` | ні | Л | ⛔ | версія схеми БД (кожна БД мігрує себе). `major`/`minor` — E9 (§10 №8, варіант A): хаб відхиляє push чужої major; **синхронізації не підлягає** — версія локальна, по мережі їде лише в заголовку `X-Schema-Major` |
| 30 | `stock` | так | В (похідне) | ↑ | залишок точки (FK → `products`, `stores`); на хаб — для зведення → §7.1-B |
| 31 | `stock_projection` | так | Х (зведення) | ↓ | серверна проєкція: Alembic `0011:11-13`; на хабі наповнюється приймачами |
| 32 | `store_activation_codes` | так | ХР | ↓ | `network.rs:411` (коди активації) |
| 33 | `store_product_prices` | так | **Х** (спільна сутність — рішення Б1, 2026-09-12) | ↕ | ціна в точці — атрибут МЕРЕЖІ: пропозиція з вузла → арбітраж хаба → роздача всім вузлам, як `products.price` (`sync.rs:278-311`); сьогодні немає ні версій, ні шляху → §7.1-C3 |
| 34 | `store_sync_state` | так | Х | ⛔ | стан синку точки/пристрою: `sync.rs:173` |
| 35 | `stores` | ні | ХР | ↓ | реєстр точок; гейт `write_gate.rs:72`; RLS |
| 36 | `supplier_ledger` | так | В | ↑ | `sync.rs:679`; tx `sync_receivers.rs:783-830` |
| 37 | `suppliers` | ні | Х (пропозиції з вузла) | ↕ | pull `sync.rs:59`; `trg_suppliers_bump`; tombstone; гейт `:130` |
| 38 | `sync_log` | так | Л (вузол) / Х (журнал прийому) | ↑ | `sync.rs:1253-1276`; **готове джерело** node→hub форвардингу (§7.1-A) |
| 39 | `sync_meta` | ні | Х | ↓ | версії довідників (6 рядків: `products`, `categories`, `suppliers`, `employees`, `settings`, `stock_norms`) |
| 40 | `system_settings` | так | Х (пропозиції точки) | ↕ | pull-сутність `settings` (`sync.rs:60`); `trg_system_settings_bump`; tombstone ВІДСУТНІЙ (`sync.rs:384-396` — op завжди upsert) |
| 41 | `transfer_items` | так | В | ↑ | усередині агрегата (`sync_receivers.rs:466`) |
| 42 | `transfers` | так | В | ↑ | `sync.rs:666`; UNIQUE `uq_transfers_client_uuid` |
| 43 | `user_stores` | так | ХР | ↓ | мапа користувач↔точки (RLS) |
| 44 | `users` | ні | Х (пропозиції з вузла) | ↕ | pull-сутність `employees` (`sync.rs:353-381`); `trg_users_bump`; tombstone; гейт `:123` |
| 45 | `work_sessions` | так | В | ↑ | UNIQUE `uq_work_sessions_client_uuid` (0013), kind ВІДСУТНІЙ → §7.1-B |
| 46 | `write_off_items` | так | В | ↑ | усередині агрегата (`sync_receivers.rs:544`) |
| 47 | `write_off_reasons` | ні | Х | ↓ | довідник причин; у pull НЕМАЄ, tombstone/версії НЕМАЄ → §7.1-C |
| 48 | `write_offs` | так | В | ↑ | `sync.rs:667`; tx `sync_receivers.rs:517-561` |

**Підсумок матриці (пораховано по файлу):** 48/48 рядків, без дублікатів і пропусків (звірено з `information_schema.tables`);
з них **В (вузол) — 21**, **Х/ХР (хаб/реєстр) — 21** (рядок 33 `store_product_prices` переведено В→Х рішенням Б1 від 2026-09-12), **Л (локально-технічні) — 5**, **✖ — 1** (`at_phase22_probe`).
Покриття синком у коді СЬОГОДНІ: ↑ 10 kinds (`sync.rs:661-682`), ↓ 6 entities (`sync.rs:56-62`);
решта — прогалини, перелічені в §7.1.

---

## 6. Висновок щодо `wal_level`

**`wal_level` змінювати НЕ потрібно. Доказ — код, не припущення:**

1. Синк не читає WAL: `POST /api/v1/sync/push` (`router_v1.rs:178`) і `GET /api/v1/sync/master`
   (`router_v1.rs:173`) працюють із звичайними таблицями (`sync.rs:117-249`, `:500-545`;
   `sync_receivers.rs` — SQL/сервіси).
2. `grep -rn "pg_logical|logical_replication|CREATE PUBLICATION|CREATE SUBSCRIPTION|pglogrepl|wal_level"`
   по `frontend/src-tauri/crates/` і `backend/app/` → **0 збігів**.
3. Єдине місце, де взагалі згадується реплікація, — скасований трек ЕТАП 15–20:
   `standby_provision.rs:5-30` (фізичний `pg_basebackup`), `promote.rs:84-87` (`pg_promote`,
   `pg_replication_slots`) — тобто **фізична** реплікація, і вона теж не потребує `logical`.
4. Фактичний стан БД: `SHOW wal_level` = `replica` (кластер 16-main, 5432) — це дефолт PG і
   **достатньо** для звичайної read-write БД. Для нової моделі WAL-лог нікому не потрібен;
   знижувати до `minimal` теж не обов'язково (не вимога, а опція економії I/O).

**Ітог: жодних вимог до `postgresql.conf` щодо реплікації (`wal_level`, `max_wal_senders`,
`max_replication_slots`, `hot_standby`) нова модель не створює.**

---

## 7. Необхідні зміни (точковий список, БЕЗ реалізації)

### 7.1 Схема БД

**A. Транспорт node→hub (нове):**
| # | Об'єкт | Опис |
|---|---|---|
| A1 | `sync_log.hub_forwarded_at timestamptz NULL`, `sync_log.hub_forward_status text NULL` (або окрема `hub_outbox`) | стан форвардингу прийнятого вузлом у хаб; індекс `(store_id, hub_forwarded_at) WHERE hub_forwarded_at IS NULL` |
| A2 | `sync_batches(id uuid PK, store_id uuid, node_id uuid, created_at, items int, status text CHECK(accepted/partial/failed))` + `sync_log.batch_id uuid NULL` | `sync_batch_id` з §4.3; індекс `(store_id, created_at DESC)` |
| A3 | `nodes(id uuid PK, store_id, name, node_token_hash, last_seen_at, status)` — **або** перевикористати `network_nodes` без `replication_*` | ідентифікація вузла-клієнта хаба (роль `node` у JWT) |

**B. Нові push-kinds (батьки, які сьогодні не подорожують):**
| # | Kind | Таблиця | Чому критично |
|---|---|---|---|
| B1 | `debtor` | `debtors` | без нього `debtor_payments` завжди відхиляється (`sync_receivers.rs:680-692`) |
| B2 | `work_session` | `work_sessions` | зміни/касові зміни не доїжджають (UNIQUE client_uuid уже є) |
| B3 | `prro_shift` | `prro_shifts` | аудит фіскалізації по точках |
| B4 | `stock_snapshot` (опц.) | `stock` | зведені залишки мережі на хабі |
| B5 | `store_product_price` | `store_product_prices` | **СПІЛЬНА сутність** (рішення Б1, 2026-09-12): ціна — атрибут МЕРЕЖІ, не точки; той самий шлях, що `products.price`. Потрібен арбітраж D1 + `server_version` (C3) |

**C. Сутності, які треба зробити придатними для pull (сьогодні не мають ні версій, ні шляху):**
| # | Таблиця | Що додати |
|---|---|---|
| C1 | `barcodes`, `product_images` | `server_version` + `trg_*_bump` + запис у `sync_meta` + entity у `ALLOWED_ENTITIES` |
| C2 | `write_off_reasons` | `server_version` + `is_deleted` + entity (tombstone-делеція довідника) |
| C3 | `store_product_prices` | `server_version` + entity |
| C4 | `print_templates` | `server_version` + entity (або явно «локальний оверрайд») |
| C5 | `system_settings` | `is_deleted` (сьогодні `sync.rs:384-396` змушений видавати лише `upsert`) |
| C6 | `stock_norms` | прибрати рядок із `sync_meta`/`ALLOWED_ENTITIES` — таблиці не існує (Alembic `0012:16-18`), pull завжди порожній |

**D. Арбітраж спільних сутностей:**
| # | Об'єкт | Опис |
|---|---|---|
| D1 | `catalog_change_requests(id uuid PK, store_id, entity, row_id uuid, op text CHECK(upsert/delete), payload jsonb, client_uuid uuid UNIQUE, status text CHECK(pending/accepted/rejected/conflict), hub_version bigint, created_at, decided_at, decided_by)` | пропозиції вузлів (§4.2); індекси `(status, created_at)`, `(entity, row_id, status)` |
| D2 | `catalog_change_requests.applied_server_version` | зв'язок із `sync_meta.version`, щоб вузол бачив «моя правка прийнята як версія N» |
| D3 | Локальний маркер на вузлі в таблицях-довідниках: `sync_state text CHECK(local/pending_hub/confirmed) DEFAULT 'confirmed'` | розрізняти «локально створене, ще не підтверджене» (offline-first) від «канонічного» |

**E. Порядок і retry:**
| # | Об'єкт | Опис |
|---|---|---|
| E1 | `push_log.error_class text` / поле у відповіді `PushItemResult` | `RETRYABLE_FK` / `VALIDATION` / `CONFLICT` (§4.3) |
| E2 | DAG-таблиця залежностей (код-константа або `sync_dependencies(child, parent)`) | топологічне сортування батча |
| E3 | `uq_debtors_client_uuid` (`debtors.client_uuid`) | ідемпотентність нового kind B1 |
| E9 | `schema_revision.major`/`.minor` + заголовок `X-Schema-Major` + 409 у `POST /api/v1/sync/push` | протокол major-сумісності схеми хаб↔вузол (§10 №8, варіант A; Alembic 0025, `sync_schema.rs`, `tests/schema_version_guard_e2e.rs`) |

### 7.2 Ендпоінти

| # | Метод/шлях | Дія |
|---|---|---|
| 1 | `POST /api/v1/sync/push` | **перевикористати** як приймач від ВУЗЛІВ (новий kind-набір, `store_id` у контексті, роль `node`) |
| 2 | `GET /api/v1/sync/master` | **перевикористати** як роздачу довідників вузлам |
| 3 | `POST /api/v1/sync/catalog-proposal` | прийом пропозицій спільних сутностей (або kind `catalog_proposal`) |
| 4 | `GET /api/v1/sync/status` | стан синку вузла (лаг, черга, конфлікти) — для моніторингу |
| 5 | `GET /api/v1/admin/sync/conflicts` | черга конфліктів на хабі (рішення оператора) |
| 6 | `DELETE` | `/api/v1/local/promote`, `/api/v1/local/repoint-primary`, `/api/v1/network-nodes/:id/force-resync`, `/api/v1/network-nodes/join`, `/api/v1/network-nodes/:id/heartbeat` |

### 7.3 Код

| # | Що | Де |
|---|---|---|
| 1 | Forwarder node→hub (tokio-задача на вузлі): читає `sync_log` (+нові kinds), POST у хаб, backoff | перевикористати як бібліотеку `offline/sync_push.rs` (FIFO/backoff/MAX_ATTEMPTS уже є) |
| 2 | Топологічне сортування батча + класифікація помилок | `offline/sync_push.rs:253-283` (`pending_outbox`), `:596-620` |
| 3 | Прибрати `NodeMode` і всі розвилки «standby/primary» | `node_config.rs:30-41` |
| 4 | Вузол → «завжди Primary»: решта коду спрощується (гейт/перехоплювач видаляються) | §8 |

### 7.4 Оцінка (order of magnitude)

| Блок | Обсяг |
|---|---|
| Схема (A–E) | 1 Alembic-міграція (~150 рядків) + 3–4 тригери |
| Ендпоінти (7.2) | 2 нові хендлери + 5 видалень; ~400 рядків |
| Форвардер node→hub | ~600 рядків (з перевикористанням клієнта) |
| Топопорядок + error_class | ~250 рядків + тести |
| Видалення (див. §8) | ~4 000 рядків коду + ~6 000 рядків тестів |

---

## 8. Що ВИДАЛИТИ (мертвий код після зміни моделі)

| # | Артефакт | Рядків | Чому мертвий |
|---|---|---|---|
| 1 | `crates/torgashka-infrastructure/src/standby_provision.rs` | 1091 | фізичний `pg_basebackup`, слот, hot_standby |
| 2 | `crates/torgashka-api/src/promote.rs` | 606 | `pg_promote` + repoint — операції режиму реплікації |
| 3 | `crates/torgashka-api/src/write_gate.rs` | 850 | `POLICY_TABLE`/`classify_request`/`gate_middleware`/`standby_503` — концепція «чи можу я писати» |
| 4 | `crates/torgashka-api/src/readonly_net.rs` | 216 | перехоплювач відмов репліки (рубіж 2 ADR-0007) |
| 5 | `crates/torgashka-infrastructure/src/readonly_guard.rs` | 389 | маркер 25006 + метрики репліки |
| 6 | `crates/torgashka-infrastructure/src/node_config.rs` | 1149 (частково) | `NodeMode::Standby`, `RepointPending`, `DEFAULT_LOCAL_PG_PORT`, `PRIMARY_CHECK_TIMEOUT` |
| 7 | `crates/torgashka-infrastructure/src/standby_heartbeat.rs` | 393 (частково) | standby-специфічна частина; SQLite-канал присутності пристроїв — лишити |
| 8 | `crates/torgashka-api/src/network_nodes.rs` | 1118 (частково) | `join_node` (replication creds), `force_resync_node`, `replication_role_name/slot_name/lag_bytes` |
| 9 | `route_local.rs:396, :401-416` + тест `:968` | ~30 | поле `readonly_net` у `/local/status` |
| 10 | ADR-0007 (`docs/adr/ADR-0007-standby-write-routing.md`) | 1571 | → статус **Superseded** (не видаляти: історичний запис) |
| 11 | docs: `operations/network-two-devices.md`, `operations/setup-two-devices.md`, `operations/disaster-recovery-network.md`, `system_documentation/sources/network-replication-etap15-20.md` | ~77 KB | описують скасовану модель → архів/переписування |
| 12 | Тести, що емулюють read-only репліку: `readonly_net_e2e.rs`, `readonly_guard.rs`(tests), `promote_drain_e2e.rs`, `write_gate_behavior.rs`, `write_gate_guard.rs`, `adr0007_at_contract.rs`, `*_standby_outbox_e2e.rs` (5 файлів) | ~6 000 | предмет (standby) зникає; e2e offline-черги — переписати під «хаб недоступний», а не «вузол read-only» |

---

## 9. Відкинуті альтернативи (фізична реплікація standby)

| Альтернатива | Аргументи «за» | Чому відкинута |
|---|---|---|
| **Фізична hot-standby (поточна, ЕТАП 15–20)** | повна копія, WAL-стрімінг, DR-promote | вузол **read-only** → каса не може писати без primary (корінь проблеми); ламає автономність магазину; вимагає DR-процедур (promote/repoint), реплікаційних слотів, моніторингу лагу |
| **Логічна реплікація PG (publication/subscription)** | начебто «нативна» синхронізація | (1) у коді її немає (0 збігів) — це була б нова розробка з нуля; (2) не дає арбітражу конфліктів і per-item статусу; (3) не дає ідемпотентності `client_uuid` (замість неї — WAL-порядок); (4) вимагає `wal_level=logical` і реплікаційних слотів на кожен вузол; (5) DDL-еволюція (Alembic) стає окремою проблемою |
| **Файловий обмін дампами** (`pg_dump`/`psql`, уже є в `admin_db_sources`) | просто | повний перезапис, відсутність інкрементальності, конфлікти «останній дамп виграв», непридатно для безперервного синку |
| **Спільна БД для всіх вузлів (без локальних)** | один writer, немає конфліктів | повертає ту саму проблему: без мережі до спільної БД каса не працює |

---

## 10. Відкриті питання

1. **`store_product_prices` — ЗАКРИТО (2026-09-12).**
   Рішення Творця (дослівно): «Ціна товару в точці (`store_product_prices`) — атрибут МЕРЕЖІ, не точки.
   Хаб — авторитет для спільних сутностей. Вузол надсилає ПРОПОЗИЦІЮ щодо ціни; хаб арбітрує єдиний
   `server_version` і роздає всім вузлам — точно так само, як уже робиться з `products.price` (`sync.rs:278-311`).»
   Обґрунтування: мережева ціна — єдина канонічна ціна для всіх точок; узгоджено з наявною моделлю
   `products.price`. Наслідки: B5 та C3 застосовуються як до спільної сутності (див. §5 рядок 33, §7.1-B5).
   Пункт збережено як історичний слід рішення.
2. **Конфлікт-політика для `users`**: PIN/роль спільні; чи дозволяємо точці створювати касира локально з підтвердженням хаба?
3. **ПРРО-аудит**: чи достатньо push `prro_shifts` (B3) для фіскального аудиту мережі, чи потрібен окремий нічний звіт хаба?
4. **Зворотний потік мережі**: чи потрібні вузлам чужі операційні дані (напр. взаємні борги/переміщення між точками), чи достатньо «все операційне — вгору, довідники — вниз»?
5. **Ретеншн**: як довго зберігати `sync_log`/`catalog_change_requests` на хабі?
6. **Хаб-резервування**: бекап/репліка хаба як звичайна БД (не через цей протокол) — процедура не описана.
7. **`audit_log` (kind ↑)**: чи потрібен мережевий аудит на хабі, чи аудит лишається локальним (`Л`)?
8. **Координація DDL/міграцій**: `schema_revision`/`ddl_markers` — локальні для кожної БД, отже кожен вузол мігрує себе сам. Хто гарантує однаковість версій схеми на всіх вузлах і порядок викатки (несумісна зміна на хабі → вузол із старою схемою)? Потрібен протокол «мінімально сумісної версії схеми» + відмова приймати push від вузла з іншою major-версією.

   **ЗАКРИТО (2026-09-16, рішення Творця; РЕАЛІЗОВАНО 2026-10-02, етап E9) — варіант A.**
   Протокол major-сумісності схеми (нижче — реалізований контракт, не намір):

   | Елемент | Рішення та місце в коді |
   |---|---|
   | Зберігання версії | `schema_revision.major` / `.minor` (обидві `NOT NULL`; базова `1`/`0`): Alembic `0025_schema_revision_major.py` + Rust-DDL `SCHEMA_REVISION_DDL` (`db.rs`); константи `SCHEMA_MAJOR`/`SCHEMA_MINOR` — `crates/torgashka-infrastructure/src/sync_schema.rs` |
   | Оголошення версії вузлом | HTTP-заголовок `X-Schema-Major` (не поле тіла) у `POST /api/v1/sync/push`; шле як вузол→хаб (`hub_forwarder.rs`), так і каса→сервер (`post_push_batch`, `offline/sync_push.rs`) |
   | Фолбек для вузлів до E9 | Заголовка немає/порожній → приписується major базового протоколу **1** (`UNVERSIONED_MAJOR`). Хаб на major 1 → усе як раніше (жодного регресу); хаб ПІСЛЯ несумісного підняття major → **явна** відмова з обома версіями та `node_major_assumed=true` (жодного тихого псування даних). Сміття в заголовку ≠ «вузол до E9»: це 400 |
   | Валідація | ПЕРЕД прийомом хоч одного агрегата: major вузла ≠ major хаба (хаб читає свою `schema_revision`, а не константу) → батч відхилено ЦІЛКОМ. Код **409 Conflict**, `error_class='VALIDATION'` (наявний перелік E2b, без нового класу), тіло: `schema_major.{node,hub}`, `node_major_assumed`, `action='update_node'`, людський `detail`. ЧОМУ 409, а не 422/400: тіло валідне, конфлікт — у СТАНІ (версії схеми двох інстансів), а не в даних; 409 не змішується з наявними 400/422 валідаційними помилками payload у журналі оператора |
   | Скоуп гвардії | Діє лише на **хабі** (роль — з власної БД: `sync.hub_url` немає). Плече каса→вузол не чіпається: там приймач пише у СВОЮ БД своїм кодом, і відмова лише зламала б локальний прийом чеків (offline-first, §2.2) |
   | Вузол бачить версію хаба | `GET /api/v1/sync/status` → `schema_major` інстанса (у хаба — версія хаба) + заголовок `X-Schema-Major` у КОЖНІЙ відповіді push (у т.ч. у 409) |
   | Класифікація retry | Відмова за версією — **НЕ retryable**: 4xx → `failed` наявним механізмом (`offline/sync_push.rs:571-579`, `hub_forwarder.rs:264-270`), без backoff-циклу |
   | Доказ | `crates/torgashka-api/tests/schema_version_guard_e2e.rs` (7 тестів: чужа major → 409+VALIDATION+обидві версії+жодного прийнятого агрегата; сумісна → прийнято; `status.schema_major`; вузол без заголовка на базовому хабі; після підняття major — відмова; наскрізно через форвардер — `hub_outbox=failed`, `attempts=1`, повторний цикл `sent=0`) |

   ⚠ Порядок викатки (важливий): спершу на вузли (вони почнуть оголошувати версію), і лише потім піднімати `SCHEMA_MAJOR` хаба разом із несумісною міграцією.

---

## 11. Що НЕ змінюється

* Протокол `POST /api/v1/sync/push` + `GET /api/v1/sync/master` (перевикористовується як є).
* `client_uuid` + partial UNIQUE (10 індексів) — ідемпотентність залишається основою.
* `sync_meta` + per-row `server_version` + BEFORE-тригери (5) — механізм дельт без змін.
* Офлайн-черга каси (`offline/*`, SQLite outbox, FIFO, backoff) — лишається; зникає лише
  залежність від того, «read-only вузол чи ні».
* RLS (30 таблиць) — лишається як другий контур ізоляції точок.
* Модель «каса = окремий пристрій зі своєю SQLite» — лишається.
