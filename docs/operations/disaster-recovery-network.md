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

## Часті помилки

| Симптом | Причина | Рішення |
|---|---|---|
| `403` на promote | роль токена ≠ `owner` | Увійти як власник мережі |
| `403` «pg_promote … суперкористувача» | роль джерела БД не superuser | `TORGASHKA_PG_USER=postgres` або `ALTER ROLE … SUPERUSER` на локальному кластері |
| `503` «локальна репліка недоступна» | локальний PG на 5433 не запущено / режим не standby | Запустити локальний PG (провіжинінг ЕТАП 16), перевірити `[node] mode` |
| `409` «не вийшов з recovery» | PG не встиг за 30 с / проблеми WAL | Перевірити `postgres.log`, повторити promote |
| `network_nodes_updated: false` | self-вузол не ідентифіковано (немає активного слота) | Оновити роль вузла вручну (SQL/UI адміна) |
| `standby_markers_cleared: false` | файли кластера не доступні | Видалити `standby.signal` і `primary_conninfo` вручну (розділ 2, крок 5) |
