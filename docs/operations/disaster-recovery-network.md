# Disaster Recovery: первинний сервер мережі недоступний

> Документ відповідає §12 плану
> [`network-replication-etap15-20.md`](../system_documentation/sources/network-replication-etap15-20.md)
> (ЕТАП 19). Реалізація: `POST /api/v1/local/promote`,
> `POST /api/v1/local/repoint-primary` (Rust-фасад, `promote.rs`).
> Цільова аудиторія: власник мережі (role=owner) та оператор.

**Що це:** коли primary-сервер мережі магазинів недоступний (аварія, обрив
мережі, вихід з ладу), каса-standby, яка має **повну копію даних** (локальна
embedded PostgreSQL-репліка на порту 5433), піднімається до ролі **нового
primary**. Операція виконується **локально на касі** і не потребує доступу до
старого сервера.

---

## 0. Передумови

1. Вузол працює у режимі **standby** (`[node] mode = "standby"` у
   `db_sources.toml`) і локальна репліка підключена
   (`/api/v1/local/status` відповідає, `mode: "standby"`).
2. У вас є **JWT access-токен власника** (`role=owner`) — токен валідується
   локально (підпис `jwt_secret`), тому працює **навіть повністю офлайн**
   (без доступу до БД старого primary).
3. Ви фізично біля каси, яку піднімаєте.

---

## 1. Перевірка стану перед promote

```bash
# Стан локального режиму (на самій касі):
curl -s http://127.0.0.1:1420/api/v1/local/status \
  -H "Authorization: Bearer <JWT_OWNER>" \
  -H "X-Store-Id: <store_uuid>"
```

Очікувана відповідь:

```json
{
  "mode": "standby",
  "degrade_to_local": true,
  "local_port": 5433,
  "primary_reachable": false,
  "effective": "offline",
  "queue_pending": 0
}
```

`primary_reachable: false` — primary дійсно недоступний; `effective: "offline"`
— вузол живий, але відрізаний від мережі. Якщо `primary_reachable: true` —
**promote НЕ потрібен** (primary живий; примусовий promote створить split-brain).

Додаткова діагностика локального кластера (виконується promote автоматично):

```sql
-- на локальній репліці (127.0.0.1:5433):
SELECT pg_is_in_recovery();      -- true = standby-режим (очікуємо)
SELECT slot_name FROM pg_catalog.pg_replication_slots WHERE active;
```

---

## 2. Promote: «Зробити цей комп'ютер головним»

У застосунку: **Налаштування → Мережа → «Зробити цей комп'ютер головним»**
(підтвердження з явним попередженням про незворотність).

Еквівалент через API:

```bash
curl -s -X POST http://127.0.0.1:1420/api/v1/local/promote \
  -H "Authorization: Bearer <JWT_OWNER>"
```

> Маршрут монтується **поза** store-middleware і приймає лише JWT owner —
> `X-Store-Id` не потрібен.

Що відбувається всередині (порядок операцій):

1. `SELECT pg_is_in_recovery()` на локальному PG — якщо `false`, вузол уже
   primary (promote виконано раніше) — перехід до кроку 4.
2. `SELECT pg_promote(true, 60)` — вихід з recovery.
   **Вимога:** поточний користувач джерела БД має бути суперкористувачем
   локального кластера (за замовчуванням `postgres`;
   `TORGASHKA_PG_USER=postgres`). Інакше — `403` із поясненням.
3. Poll `pg_is_in_recovery()` до ~30 с — очікування підтвердження.
4. `network_nodes` у локальній БД: рядок **цього** вузла (ідентифікується за
   активним replication-слотом) → `role='primary'`, `status='active'`,
   `replication_role_name/slot_name = NULL`. Інші вузли не чіпаються.
5. Захист split-brain: видаляється `standby.signal` і `primary_conninfo` з
   `postgresql.auto.conf` локального кластера (точний `data_directory`
   читається з `pg_settings`). Після цього вузол **ніколи** не повернеться
   в standby автоматично.
6. `node_config` → `mode="primary"`, `primary_db_url` очищується
   (збереження у `db_sources.toml`).

Очікувана відповідь:

```json
{
  "ok": true,
  "promoted_at": "2026-09-10T14:22:01+00:00",
  "old_mode": "standby",
  "node_id": "<uuid>",
  "network_nodes_updated": true,
  "standby_markers_cleared": true,
  "config_file": "/path/to/db_sources.toml",
  "note": "вузол тепер primary. ..."
}
```

### Після promote (на цій касі)

1. **Переналаштувати активне джерело БД** фасаду на власну БД:
   `127.0.0.1:5433` (Налаштування → джерела даних → активне джерело →
   host `127.0.0.1`, port `5433`; user/database/пароль — ті самі, що були).
2. **Перезапустити застосунок** — фасад стартує як звичайний primary
   (`/api/v1/local/*` маршрути більше не монтуються, каса працює проти
   власної БД).
3. Переконатись:

```bash
curl -s http://127.0.0.1:1420/api/v1/health
# {"status":"ok"}

# локальний PG тепер primary:
psql "postgresql://postgres@127.0.0.1:5433/torgashka" -c "SELECT pg_is_in_recovery();"
#  f
```

---

## 3. Інші вузли: «Вказати новий головний вузол» (repoint)

Кожен ІНШИЙ вузол мережі (інші каси) після promote має бути переналаштований
на НОВИЙ primary. Їхні старі реплікаційні позиції **невалідні** — потрібен
новий `pg_basebackup`.

На кожному такому вузлі (у застосунку: **Налаштування → Мережа →
«Вказати новий головний вузол»**), або через API:

```bash
curl -s -X POST http://127.0.0.1:1420/api/v1/local/repoint-primary \
  -H "Authorization: Bearer <JWT_OWNER>" \
  -H "Content-Type: application/json" \
  -d '{"new_primary_host": "192.168.1.50", "new_primary_port": 5432}'
```

Що робить фасад (MVP — лише конфігурація):

1. Оновлює `[node] primary_db_url` на `postgresql://<ті самі creds>@<new_host>:<new_port>/<db>`
   (креденшалі/БД беруться з поточного URL).
2. Записує позначку `repoint_pending = { new_primary_host, new_primary_port, requested_at }`.
3. **Фактичний `pg_basebackup` виконує оператор ВРУЧНУ** (розділ 4 нижче).

Відповідь містить `repoint_pending` та інструкцію. До завершення нового
`pg_basebackup` вузол працює зі **старими** даними — не від'єднуйте його від
мережі без потреби.

---

## 4. Ручний pg_basebackup (новий standby під новий primary)

Виконується оператором на вузлі, який має стати standby (з тих самих
бінарників PostgreSQL, що й провіжинінг ЕТАП 16):

```bash
# Приклад (Linux; TORGASHKA_PG_DIR — каталог бінарників, data_dir — каталог даних):
export TORGASHKA_PG_DIR=/opt/torgashka/postgres
DATA_DIR="$HOME/.local/share/Torgashka/pgdata"
BIN="$TORGASHKA_PG_DIR/bin"

# 1) Зупинити локальний PG, якщо працює:
"$BIN/pg_ctl" -D "$DATA_DIR" stop -m fast || true

# 2) Пересоздати каталог даних (СТАРІ дані невалідні — позиція WAL зі старого primary):
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"

# 3) Новий basebackup з НОВОГО primary (-R: standby.signal + primary_conninfo;
#    -C: створити replication slot на новому primary):
"$BIN/pg_basebackup" -h 192.168.1.50 -p 5432 \
  -U replicator -D "$DATA_DIR" -R -C -S standby_<node_hex> \
  --checkpoint=fast --wal-method=stream -P

# 4) Запустити PG:
"$BIN/pg_ctl" -D "$DATA_DIR" start

# 5) Перевірка — вузол у standby:
psql "postgresql://postgres@127.0.0.1:5433/torgashka" -c "SELECT pg_is_in_recovery();"
#  t
```

Після цього у `db_sources.toml` вузла має стояти `[node] mode = "standby"` із
`primary_db_url` на новий primary (крок 3 вище вже це зробив через
`repoint-primary`). Якщо `repoint_pending` лишився від попередньої спроби —
його можна залишити (історія) або прибрати вручну з файлу.

---

## 5. Старий primary при поверненні

Старий сервер після відновлення **НЕ підключається автоматично** (захист від
split-brain). Його дані тепер застарілі (новий primary міг прийняти записи).

Порядок дій оператора:

1. **Не запускати** старий primary у мережі, поки не вирішено, хто головний.
2. Якщо новий primary лишається головним — старий сервер **приєднується як
   НОВИЙ standby** через стандартний join-код (розділ «приєднати вузол»
   інтерфейсу; створюється новий рядок `network_nodes` з новим
   replication-слотом), потім — кроки розділу 4 з `pg_basebackup` із нового
   primary.
3. Якщо потрібно повернути головну роль старому серверу — повторіть promote
   на ньому (розділ 2) і знову repoint решти вузлів.

**Правило:** жоден вузол не має права автоматично «дізнатись», що став
primary, і жоден старий primary не підключається сам. Рішення приймає
власник вручну — це єдиний захист від двох primary в одній мережі.

---

## 6. Несинхронізована черга каси при promote (ФАКТИЧНА поведінка коду)

На вузлі-standby документи каси пишуться в **локальну SQLite-чергу** (`outbox`), звідки
доїжджають на primary приймачами (`sync.rs`, `sync_receivers.rs`) — ADR-0007 §9, §10, §11.6.
Несинхронізована черга = невивантажені продажі (чеки, борги, залишки).

### 6.1 Код: promote чергу НЕ торкається

- `promote.rs` не містить роботи з чергою: grep `outbox|sqlite|offline` по
  `frontend/src-tauri/crates/torgashka-api/src/promote.rs` → єдине згадування — **коментар**
  `promote.rs:87-89`. Файл `offline.db` (шлях: `offline/db.rs:78-88`) promote не читає,
  не чистить і не перекладає в PG.
- Що promote робить фактично (`promote.rs:75-90` doc, реалізація `:90-235`):
  1. `pg_is_in_recovery()` на локальному PG (`:91-110`);
  2. `pg_promote(true, 60)` + poll виходу з recovery (`:131-165`);
  3. `network_nodes`: self-вузол → `role='primary', status='active'`, replication-creds = NULL
     (`:170-186`);
  4. видалення `standby.signal` / `primary_conninfo` (split-brain guard, `:249-300`);
  5. `node_config` → `mode=primary`: `into_promoted_primary()` очищає `primary_db_url` **і**
     `upstream_write_url` (`crates/torgashka-infrastructure/src/node_config.rs:214-221`) —
     фасад пише вже лише локально;
  6. подія `promoted` у `network_events` (`:219-227`, `network.rs:304`).

### 6.2 Код: після promote поверхня `/api/v1/local/*` зникає

- `route_local::router` монтується лише за `is_standby() && local.is_some()`
  (`route_local.rs:649`); адмін-маршрути DR — так само (`promote.rs:432-437`).
- Наслідок: `GET /api/v1/local/status` і `GET /api/v1/local/stock-reconciliation` після promote
  → **404**. Тобто звірка §10.3 доступна **лише поки вузол standby** (до promote).

### 6.3 Код: push-цикл після promote — ВИПРАВЛЕНО (Фаза 3.8)

**Було (аномалія, знайдена NIKO):** фоновий push стартував за наявності
`server_url`+токена в SQLite-налаштуваннях (`src/lib.rs:347`,
`offline/commands.rs:123-140`, `read_sync_auth` — `:74-120`) і **не перевіряв
`mode` вузла**; `promote` не чистив SQLite-налаштування активації каси
(`server_url`/`api_token`/`device_token`), тож залишок черги після promote
продовжував слатися на **старий** `server_url` (ризик split-brain). Коментар
`promote.rs:89` («push-черги вимкнені») **суперечив коду**.

**Стало:** ґейт у push-клієнті — `sync_push::push_pending_batch_with_node`
(`offline/sync_push.rs:507`; прод-обгортка `push_pending_batch` — `:483`) +
чистий предикат `NodeConfig::push_blocked_reason` (`node_config.rs:246`,
`has_configured_upstream` — `:218`, `is_self_local_url` — `:539`):

* **умова ґейта = САМЕ КОМБІНАЦІЯ** `mode == Primary` **І** апстріму немає →
  HTTP-push **не виконується взагалі** (0 з'єднань), повертається `Ok` із
  `sent=0` і маркером `gated`; повідомлення в stderr — **один раз на процес**
  (`sync_push.rs:452`, анти-спам для циклу 30 с);
* **норма НЕ зламана:** `mode=Standby` (штатна каса) і `mode=Primary` **із**
  заданим `[node] primary_db_url` (standalone POS, що пише на віддалений
  сервер) пушать як і раніше — тести `gate_lets_standby_push`,
  `gate_lets_primary_with_remote_upstream_push`,
  `gate_is_inactive_without_explicit_node_section`
  (`offline/sync_push.rs:1548`, `:1524`, `:1567`);
* ґейт читає **ЯВНО записану** секцію `[node]` (`NodeConfig::load_explicit` —
  `node_config.rs:273`): файлу/секції немає → ґейт не діє (дефолт `Primary` —
  не рішення вузла);
* «посилання на себе» апстрімом не вважається (`is_self_local_url`): після
  promote оператор переналаштовує активне джерело на `127.0.0.1:<local_port>`
  — ґейт лишається увімкненим, інакше черга знову пішла б на мертвий сервер
  (тест `self_reference_is_not_an_upstream` — `:1583`);
* `sync_now` віддає це оператору явно: `gated: true` + `gate_reason`
  (`offline/commands.rs:518`, `:560`) — «push не пішов» більше не виглядає як
  «немає мережі». Коментар `promote.rs:92` тепер відповідає коду;
* шлях вузла на МЕРТВИЙ порт доведено тестом `gate_blocks_http_push_on_promoted_primary`
  (`offline/sync_push.rs:1493`) — 0 TCP-з'єднань, черга лишається `pending`.

### 6.4 Drain залишку черги → ВЛАСНИЙ PG — РЕАЛІЗОВАНО (Фаза 3.8)

**Було:** шляху застосування залишку SQLite-черги до власного PostgreSQL після
promote не існувало (grep `replay|drain|flush_outbox|apply_outbox` → лише ПРРО).

**Стало:** `route_local::drain_local_outbox`
(`crates/torgashka-api/src/route_local.rs:689`) + ендпоінт
`POST /api/v1/local/outbox/drain` (owner-only, як `promote`;
`drain_outbox_handler` — `:800`, `outbox_router` — `:822`, монтується завжди —
`router_v1.rs:824`).

* джерело черги — `pending_outbox` (FIFO) з SQLite каси; **кожен `payload` — уже
  серіалізований конверт `PushEnvelope`**, який десеріалізується й
  застосовується **наявним ядром** `sync::process_push_item`
  (`crates/torgashka-api/src/sync.rs:564`, `pub(crate)`) — жодного дублювання
  логіки apply;
* ціль — **власний** PG: `state.local.pool` (standby, у т.ч. щойно promote-нутий
  до рестарту), інакше `state.store_pool` (рестарт після promote:
  `mode=Primary`); сервіси будуються **сирі**
  (`SqlxPos`/`SqlxInvoices`/`SqlxReturnInvoices`), НЕ outbox-адаптери — інакше
  drain замкнувся б сам на себе;
* `created`/`already_exists` → `sync_push::mark_done`
  (`offline/sync_push.rs:729`) → черга зменшується; `error` → агрегат
  **лишається `pending`** (дані не втрачено), причина у `errors_detail` (до 10);
* **ідемпотентність** — `client_uuid` (як у push): повторний drain →
  `already_exists`, 0 дублів (E2E
  `repeated_drain_is_idempotent_already_exists` —
  `tests/promote_drain_e2e.rs:500`);
* повертає `{drained, created, already_exists, errors, pending_left,
  errors_detail}`;
* **promote викликає drain автоматично** (best-effort, крок 7 —
  `crates/torgashka-api/src/promote.rs:246`, підсумок у відповіді `outbox_drain`,
  `:270`); помилка drain promote НЕ валить (він уже відбувся) — оператор
  повторює ендпоінт вручну;
* Е2Е доказ «накладна офлайн → promote → drain → агрегат у власному PG, черга
  порожня»: `tests/promote_drain_e2e.rs:371` (`promote_drains_local_outbox_into_own_pg`).

**Бекап черги — РЕАЛІЗОВАНО (Фаза 3.9):** `scripts/backup.sh` повним циклом
(без прапорців) знімає `offline.db` консистентно (`sqlite3 .backup`, `scripts/backup.sh:186-247`,
виклик `:304`, ротація `:310`); відновлення — `scripts/backup-restore.sh --queue <файл> --yes`
(`scripts/backup-restore.sh:95-186`). Деталі й застереження:
`docs/infrastructure/backup-restore.md` §9.4, §9.6-§9.7. Перед DR усе одно робіть
**окремий** бекап черги безпосередньо перед операцією — drain (§6.4) не заміна бекапу.

**Правило для оператора:** promote з непорожньою чергою БІЛЬШЕ НЕ лишає продажі
в SQLite — вони застосовуються у власний PG (крок 7) або пізніше вручну
(`POST /api/v1/local/outbox/drain`). Бекап `offline.db` перед DR усе одно
обов'язковий (`docs/infrastructure/backup-restore.md` §9): drain — не заміна
бекапу, а шлях доїзду.

### 6.5 Порядок дій, коли черга накопичилась за час офлайну

1. **Не робити promote з непорожньою чергою, якщо старий primary живий хоч частково:**
   спершу підняти канал і дати черзі поїхати (фоновий цикл або ручний `sync_now` —
   `offline/commands.rs:489-556`).
2. Якщо primary втрачено назавжди і promote неминучий:
   1. бекап `offline.db` (`docs/infrastructure/backup-restore.md` §9);
   2. зафіксувати борг: `sqlite3 <db_path> "SELECT status, COUNT(*) FROM outbox GROUP BY status;"`
      (`db_path` — з `get_offline_stats`, `offline/commands.rs:559-578`);
   3. promote;
   4. drain виконується АВТОМАТИЧНО кроком 7 promote (§6.4) і застосовує залишок у
      власний PG; якщо він не пройшов (помилка в `outbox_drain`) — повторити
      `POST /api/v1/local/outbox/drain` (ідемпотентно) і звірити `pending_left` = 0.
      HTTP-push при цьому свідомо вимкнено (§6.3, `gated: true` у `sync_now`).
3. Не видаляти й не перезаписувати `offline.db` до звірки — це єдина копія невивантажених
   продажів цього вузла.

---

## 7. Звірка залишків після відновлення (§10.3)

Локальний залишок каси і авторитетний на primary можуть різнитися: каса застосовує
stock-ефект документа **відразу** (offline-first), primary — після push.

**Ендпоінт (READ, працює лише на standby):**
`GET /api/v1/local/stock-reconciliation?invoice_id=<uuid>` — `route_local.rs:518-608`
(маршрут — `:670-671`).

- Локальна половина (SQLite каси): `offline/reconciliation.rs::local_view`
  (`reconciliation.rs:58-90`) — агрегат черги за `client_uuid` + `stock::get_stock_level`.
- Авторитетна половина (репліка PG): `authoritative_milli` (`route_local.rs:612-630`).
- Відповідь: `local_qty` з прапорцем `local_is_estimate: true` (`route_local.rs:596-600`),
  `authoritative_qty`, `delta = local − authoritative`, `matches`,
  `summary {total, matching, mismatching}` (`route_local.rs:601-608`).
- Документа немає в черзі цієї каси → **404** (`route_local.rs:539-543`); документ іншої точки →
  **404** (`route_local.rs:544-551`).

**Ключове:** локальне число — **ОЦІНКА**, не істина (ADR-0007 §10.3;
`reconciliation.rs:5-11`; прапорець `local_is_estimate: true`). Ендпоінт **лише показує** —
жодних записів і жодного reconcile-движка (`reconciliation.rs:16`). Вирівнювання —
**інвентаризацією**: `stock::set_stock_level` (`offline/stock.rs:116`) через тип `inventory`
(`offline/transactions.rs:28`, `:152`).

Порядок звірки:

1. `queue_pending` = 0 або стабільно не зменшується → черга доїхала
   (`/api/v1/local/status`, `route_local.rs:332-347`).
2. Для кожного документа з підозрою на розбіжність — запит до
   `GET /api/v1/local/stock-reconciliation?invoice_id=<client_uuid>`
   (id документа = `client_uuid` локального агрегата, `route_local.rs:511-514`).
3. `delta != 0` → перелічити фактично й провести **інвентаризацію** (наявний механізм), а не
   правити число в PG вручну.

---

## 8. Операційні події та індикатор режиму вузла

### 8.1 `network_events` (primary)

- Схема й перелік значень: `crates/torgashka-infrastructure/src/db.rs:451-474`; запис —
  `torgashka-api/src/network.rs:304`; читання —
  `network_nodes.rs:813-850` (`GET /api/v1/network/nodes/events`, `router_v1.rs:800`).
- `degraded_local` (level `warn`) — primary став недоступним;
  `primary_restored` (level `info`) — зв'язок повернувся.
- Запис — **лише на ПЕРЕХОДІ** стану (анти-спам, edge-детектор `LAST_PRIMARY_UP`):
  `route_local.rs:96, :103-140` (`note_connectivity_transition`). Перше спостереження сесії
  одразу в деградації теж логується раз (`:118-125`). На read-only репліці (до promote) INSERT
  неможливий — помилка глушиться в stderr (`:139-140`, `network.rs:315`).
- Інші значення того ж журналу: `promoted`, `repoint_requested`, `archived`,
  `resync_requested`, `sync_error`, `reject_stale` (`db.rs:451-452`, `network_nodes.rs:791-792`).

### 8.2 Індикатор режиму вузла в UI

- Джерело істини — `GET /api/v1/local/status` (`route_local.rs:332-347`):
  `{mode:"standby", primary_reachable, effective, queue_pending, …}`; `effective` =
  `effective_status(primary_up, pending)` (`route_local.rs:237-245`):
  - `active` — primary доступний, черга 0;
  - `lagging` — primary доступний, черга > 0 (**норма**, синк іде — не тривога);
  - `offline` — primary недоступний (деградація, черга росте).
- UI: `frontend/src/hooks/useNodeStatus.tsx` (полінг 20 с — `:22`; тости лише на переходах —
  `:78-100`; рендер трьох станів — `:130-170`), сервіс `frontend/src/services/nodeStatusService.ts:10-18`.
  На primary-вузлі маршруту `/api/v1/local/*` немає → індикатор не рендериться (404,
  `route_local.rs:649`).
- **Окремий (грубіший) контур heartbeat:** `standby_heartbeat.rs:248-258` шле
  `status="active"` + `app_version`; допустимі значення —
  `["provisioning","syncing","active","lagging","offline"]` (`network_nodes.rs:549`); lag > 50 МБ
  примусово дає `lagging` (`network_nodes.rs:551-552`, `:686-688`). Це не те саме, що
  `effective` з `/local/status`.

---

## Часті помилки

| Симптом | Причина | Рішення |
|---|---|---|
| `403` на promote | роль токена ≠ `owner` | Увійти як власник мережі |
| `403` «pg_promote … суперкористувача» | роль джерела БД не superuser | `TORGASHKA_PG_USER=postgres` або `ALTER ROLE … SUPERUSER` на локальному кластері |
| `503` «локальна репліка недоступна» | локальний PG на 5433 не запущено / режим не standby | Запустити локальний PG (провіжинінг ЕТАП 16), перевірити `[node] mode` |
| `409` «не вийшов з recovery» | PG не встиг за 30 с / проблеми WAL | Перевірити `postgres.log`, повторити promote |
| `network_nodes_updated: false` | self-вузол не ідентифіковано (немає активного слота) | Оновити роль вузла вручну (SQL/UI адміна) |
| `standby_markers_cleared: false` | файли кластера не доступні | Видалити `standby.signal` і `primary_conninfo` вручну (розділ 2, крок 5) |
