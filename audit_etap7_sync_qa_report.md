# Приймальний аудит ЕТАПУ 7 offline-first sync (QA_Agent)

> Репо: Projects/Torgashka · Гілка: sync-offline · Голова: 5f73d99 (дерево чисте)
> Дата: 2026-09-01 · Аудитор: QA_Agent (незалежний, код НЕ змінювався)
> Джерело істини: docs/design/sync-schema-design.md, розділ 9 «ЕТАП 7»
> Перевірено реальним читанням коду/схеми/БД + незалежним прогоном тестів.

---

## 0. Незалежний прогін: `cargo test --workspace`

**51 тестова ціль · 381 passed · 0 failed** (лог: `frontend/src-tauri`, запуск у межах цього аудиту).
Ключові для ЕТАПУ 7:
- `sync_4stores_outage_e2e` — 1 passed (1.91 s)
- `sync_push_e2e` — 2 passed · `sync_pull_e2e` — 1 passed
- `offline_migrations` — 6 passed · `pos_crud` — 5 passed (write_off_and_transfer_flow зелений)
- Unit: sync_push (backoff/FIFO/sync_log/sync_health) + sync_pull — зелений

Вердикт завдання «всі тести зелені» — **✅ підтверджено незалежним прогоном**.

---

## 1. Покриття дизайну ЕТАПУ 7 (розділ 9)

### 1.1 Задача: «інтеграційні тести синку (pull дельти, push ідемпотентність, FIFO, backoff, edge-cases)»

| Пункт | Стан | Свідчення |
|---|---|---|
| pull дельти | ✅ тест є | `sync_pull_e2e` 1 passed; unit `multi_page_delta_advances_in_steps` |
| push ідемпотентність | ✅ | `sync_push_e2e::push_idempotent_single_server_record`; UNIQUE на рівні БД (нижче) |
| FIFO | ✅ | unit `pending_outbox_is_fifo`, `clock_skew_does_not_reorder_outbox` |
| backoff | ✅ | unit `backoff_is_exponential_with_cap` (min(2^attempts,3600)), `ten_failed_attempts_marks_failed` |
| edge-cases | ⚠️ частково | Є unit (rollback mid-tx, local stock, RLS-контекст), але QA-край «багатоденний офлайн → дата звіту» не покритий (див. §4.3) |

### 1.2 Задача: «QA-сценарії (вимкнення сервера на годину, відновлення, зведений звіт власника по 4 точках)»

| Пункт | Стан | Свідчення / коментар |
|---|---|---|
| Симуляція 4 кас × outage → відновлення | ✅ (чеки) | `sync_4stores_outage_e2e`: фаза offline (pending росте, sync_log push_fail), фаза відновлення (flush), фаза ідемпотентності (done→pending → already_exists) |
| «після відновлення **всі дані** на сервері» | ⚠️/❌ частково | На сервер потрапляють **ЛИШЕ чеки** (sale/return). Закупки/інвентаризації/переміщення/списання (типи ЕТАПУ 6) лишаються на касі `synced=0` — **outbox для них не заповнюється свідомо** (документовано в `offline/transactions.rs:15-24`: серверний фасад приймає лише receipt/return_receipt; «ЕТАП 7 — розширення фасаду» — **не зроблено**). E2e прямо це визнає: purchase_order залишається synced=0 |
| «дублікатів 0» | ✅ | Див. §4.1 — гарантія на рівні БД (Alembic 0013), не тільки в тесті |
| «зведений звіт точний» | ⚠️ | Див. §4 — ендпоінта-агрегата по точках НЕ існує (підтверджено); дані-чеки збігаються; закупки відсутні; created_at сервера зміщує період |

### 1.3 Задача: «моніторинг sync_log (алерт на failed/стагнацію)»

| Пункт | Стан | Свідчення |
|---|---|---|
| sync_log (SQLite, каса) | ✅ | Міграція `0008_sync_log.sql`: kind CHECK (push_ok/push_fail/pull_ok/pull_fail/retry), entity, detail, attempts, індекс `idx_sync_log_kind_ts` |
| Події push у тій самій транзакції, що статус | ✅ | `mark_done`/`mark_failed`/`defer_or_fail` (sync_push.rs:642-687,593-641) — INSERT sync_log у tx статусу; unit `sync_log_rollback_leaves_no_fake_push_ok` |
| Події pull після COMMIT | ✅ | `pull_all` (sync_pull.rs:381-426): pull_ok/pull_fail per entity |
| sync_health/degraded | ✅ | outbox_failed>0 АБО stale pending (next_attempt_at < now-3600); поля last_push_ok_at/last_pull_ok_at/last_push_fail_at/last_error |
| Tauri-команда | ✅ | `sync_health` зареєстрована (lib.rs:339); `sync_status` доповнено полем `health` без ламання старого контракту (commands.rs:343) |
| **Алерт на failed (UI)** | ✅ | `SyncStatus` (useOfflineSync.tsx): failedCount>0 → червоний «⚠ Потребує уваги» |
| **Алерт на стагнацію (UI)** | ❌ | `degraded` **не споживається фронтом**: у offline.ts немає навіть обгортки `sync_health`; `SyncStatus` не читає `status.health`; стагнація показується лише як жовтий лічильник «N очікують» без розрізнення «активний backoff» vs «цикл мертвий» |

### 1.4 Вихід: «зелені тести, приймальні сценарії пройдені, моніторинг працює»

Тести зелені ✅ · Сценарій пройдено частково (тільки чеки) ⚠️ · Моніторинг працює частково (failed — так, стагнація — дані без UI-алерту) ⚠️

---

## 2. Аудит логіки моніторингу — детальні відповіді

### 2.1 Чи реально ловить degraded стагнацію циклу і failed>0? — ✅ так, з нюансом
- `failed > 0` → degraded (unit `sync_health_degraded_when_failed`).
- stale pending: `COUNT(*) FROM outbox WHERE status='pending' AND next_attempt_at < now-3600` → degraded. Оскільки `next_attempt_at` у схемі **NOT NULL DEFAULT now()** (ніколи не NULL), умова спрацьовує і для свіжих pending, що не рухались >1 год (unit `sync_health_degraded_on_stale_pending_only`).
- **Нюанс:** при **мережевій** помилці (сервер недоступний) `push_pending_batch` повертає Err **без** `defer_or_fail` → `next_attempt_at` НЕ оновлюється → stale стає через 1 год після created_at. Це коректно (каса не синкає >1 год → degraded), але семантика «стагнація циклу» змішується з «мережа лежить» — для оператора обидва випадки = «потребує уваги», прийнятно.
- **Не ловиться:** «pull не оновлювався > N» при живому push (див. 2.3); «останній успіх давно» як самостійний критерій у degraded відсутній (є лише поля last_push_ok_at/last_pull_ok_at для зовнішнього споживача).

### 2.2 Чи є шлях «outbox застряг у in_flight назавжди»? — НІ, за конструкцією. Висновок: ризик відсутній.
- Статус `in_flight` існує лише в CHECK-обмеженні схеми outbox (0002) як спадщина дизайну. **Жоден рядок коду не встановлює `in_flight`** (grep по всіх crates — порожньо, включно з тестами). `pending_outbox` обирає лише `status='pending'`.
- Наслідок: проміжного стану «взято в роботу» немає → **немає стану, з якого можна застрягти**. Падіння каси під час push:
  - до HTTP-відправки → агрегат pending → наступний цикл відправляє;
  - після COMMIT сервера, до mark_done → агрегат pending → повторний push → сервер `already_exists` (SELECT + UNIQUE 0013) → done. Ідемпотентно.
- **Health бачить стагнацію:** якщо цикл мертвий (таск упав / додаток закритий), pending старіє → stale_pending → degraded за ≤1 год + outbox_pending видно в статусі. Детекція з затримкою ≤ ~1 год — прийнятна для «алерту на стагнацію».
- Дрібниця: CHECK дозволяє стан, який код не використовує → ризик майбутнього misuse. Рекомендація LOW: прибрати `in_flight` з CHECK або задокументувати «стан зарезервовано, не використовується».

### 2.3 sync_log: чи всі шляхи push/pull пишуть події? — є 2 шляхи помилки без журналу (LOW)
Покриті: 5xx/429 → retry (у tx); 10-та спроба → push_fail (у tx); 400/422 пакет → push_fail; per-item error → push_fail; created/already_exists → push_ok; мережева помилка → push_fail (log_event поза tx, sync_push.rs:486-491); pull_ok/pull_fail per entity.

**Без журналу (рідкісні аномальні стани):**
1. `sync_push.rs:537` — HTTP 200, але тіло — невалідний JSON → `return Err(...)` без log_event.
2. `sync_push.rs:549-551` — сервер не повернув результат для агрегата (None) → `deferred` без події, статус/next_attempt_at не змінюються; при систематичному ігноруванні сервером — вічні спроби кожні 30 с без сліду в журналі (health побачить stale лише через 1 год, причину — ні).

Обидва — наслідок дефектного/аномального сервера, не штатний шлях. LOW.

### 2.4 Відповідність 0008 конвенціям міграцій — ✅
- `SCHEMA_VERSION = 8` (migrations.rs), 0008 у реєстрі MIGRATIONS (include_str).
- Ідемпотентність: `CREATE TABLE IF NOT EXISTS` + `CREATE INDEX IF NOT EXISTS`.
- Тести: `offline_migrations` 6 passed (fresh_db, legacy без втрат, **rerun_is_idempotent**, engine_reports_current_version=8); unit fresh_db_migrates_to_actual (migrations.rs:292).
- Стиль (datetime('now'), idx_*, CHECK) відповідає 0002-0007.
- Дрібниця: fresh_db_migrates_to_actual не асертить наявність таблиці `sync_log` (лише версію) — покриття 0008 опосередковане; не критично (unit sync_push реально пише в sync_log на свіжій БД).

---

## 3. Безпека/краї (ідемпотентність push) — детальна відповідь

### 3.1 Гарантія «0 дублікатів» — ✅ на рівні БД, не тільки в тесті
- Alembic **0013_sync_push_idempotency**: `client_uuid uuid` + partial `UNIQUE uq_{table}_client_uuid (client_uuid) WHERE client_uuid IS NOT NULL` на **8 приймачах**: receipts, return_invoices, purchase_orders, inventories, transfers, write_offs, debtor_payments, work_sessions.
- Перевірено в БД `pos_system_fresh`: alembic_version = 0013, усі 8 індексів присутні.
- Приймач push: попередній SELECT (`find_by_client_uuid`, sync.rs:575) + catch UNIQUE-гонки (sync.rs:555-565: msg.contains("uq_receipts_client_uuid") → already_exists).
- **Коректність для return:** `create_return_receipt` пише в **receipts** (create_receipt_impl, receipt_type='return') — той самий контур UNIQUE `uq_receipts_client_uuid`; `return_invoices` (Python-модель) push-приймачем не використовується. Тому contains("uq_receipts_client_uuid") ловить обидва типи — вади немає.
- E2e Фаза 3a (done→pending, повторний push) + перевірка `COUNT(*)-COUNT(DISTINCT client_uuid)=0` — підтверджує.

### 3.2 Межа ідемпотентності
- Серверний приймач підтримує **тільки** `receipt`/`return_receipt` (sync.rs:439-449): інші типи дизайну 2.2 → per-item `error` «тип не підтримується push ЕТАП 4» (TODO ЕТАП 6 — **не закрито в ЕТАПІ 7**). Для 6 інших типів UNIQUE-індекси готові (0013), але приймачів немає.

---

## 4. «Зведений звіт власника точний» — висновок

### 4.1 Ендпоінта-агрегата по точках НЕ існує — ✅ ПІДТВЕРДЖУЮ відкрите питання дизайну
- Rust-роути (router_v1.rs): `today_stats` (/stats/today — **глобально по всіх точках**, без store-фільтра, адмін), `work_report` (по work_sessions), list/search по X-Store-Id (одна точка). **Немає** `/reports/summary?stores=[4]` чи подібного per-store агрегата.
- Python backend (app/api v1/v2): inventory/work_sessions/settings/users/prro — також немає.
- **Висновок:** «зведеного звіту власника по 4 точках» як готового ендпоінта немає — відкрите питання дизайну ПІДТВЕРДЖЕНО. E2e чесно перевіряє точність прямим SQL по `receipts` (count+sum per store), а не через ендпоінт звіту.

### 4.2 Дані, що приходять через push, потрапляють у ті самі таблиці, з яких рахуються звіти — ✅ (для чеків)
`today_stats`/звіти читають `receipts`/`receipt_items`; push-прийом пише в `receipts`/`receipt_items` (create_receipt_impl). Спільна таблиця → чеки 4 кас після sync коректно відображатимуться у будь-якому майбутньому зведеному звіті на основі receipts.

### 4.3 Розриви точності звіту (фіксуються)
1. **Закупки та інші типи ЕТАПУ 6 не на сервері** (§1.2) → звіт, що включає витрати/собівартість/залишки, буде неповним. Блокуючий розрив для повноти «зведеного звіту».
2. **created_at = now() сервера** (insert_receipt, pos.rs:330 — `(now() AT TIME ZONE 'UTC')::timestamp`), payload.created_at каси (RFC3339) **ігнорується** → чеки багатоденного офлайну зараховуються у звіт за датою синхронізації, а не продажу. При «відмові мережі на годину» в межах дня — не помітно; при офлайні через добу — денний/періодний звіт неточний. E2e не ловить (перевіряє суми за весь період без фільтра created_at).
3. `today_stats` — глобальний (усі точки разом), без розбивки по точках; per-store точність e2e перевіряє прямим SQL.

---

## 5. Інші знахідки (не блокують, але фіксуються)

| # | Ризик | Пріоритет |
|---|---|---|
| 5.1 | **spawn_pull_task НЕ запускається в додатку**: pull_all/pull_entity викликаються лише з тестів; `sync_now()` робить тільки push. Каса в prod не оновлює довідники; `last_pull_ok_at` у health ніколи не заповниться. Питання ЕТАПУ 3/інтеграції, але прямо впливає на моніторинг pull | HIGH (інтеграція pull) |
| 5.2 | e2e sync_* (torgashka-api) підключаються через `resolve_database_url()` → **робоча dev-БД pos_system_fresh**, а не `_test` (на відміну від pos_crud → connect_test_pool з _test і захистом). Тестові дані забруднюють робочу БД | MEDIUM (гігієна тестів) |
| 5.3 | Стан `in_flight` у CHECK outbox — фантом (§2.2) | LOW |
| 5.4 | 2 шляхи помилки без журналу sync_log (§2.3) | LOW |
| 5.5 | Рудиментарна колонка `receipts.client_receipt_uuid` (kasa-спадок) лишається в БД поряд із `client_uuid` (задокументовано в docstring 0013) | LOW (прибрати при нагоді) |
| 5.6 | Позитив: фікс pos_crud підтверджено — Rust-код і тестова БД `pos_system_fresh_test` консистентні (transfers.from_location/to_location), 5/5 зелений. Python dev-БД (from_store_id) — окремий стек, не конфліктує з Rust-тестами | — |

---

## 6. Фінальний вердикт: **НЕ ПРИЙНЯТО** (критерій ЕТАПУ 7 виконано частково)

Обґрунтування: критерій дизайну містить три складові — (а) «всі дані на сервері», (б) «дублікатів 0», (в) «моніторинг sync_log (алерт на failed/стагнацію)». Виконано: тести зелені (381/0), дублікатів 0 гарантовано на рівні БД, моніторинг-дані (sync_health/degraded) і журнал — якісні. НЕ виконано:

1. **[БЛОКУЮЧЕ] «Всі дані на сервері»** — push приймає лише чеки; закупки/інвентаризації/переміщення/списання (реальні офлайн-операції каси ЕТАПУ 6, synced=0) на сервер не доставляються. `transactions.rs` сам фіксує це як «ЕТАП 7 — розширення фасаду», але в коміті 5f73d99 цього немає. Наслідок: зведений звіт власника (з витратами/собівартістю) буде неповним.
2. **[БЛОКУЮЧЕ] Алерт на стагнацію** — `degraded`/`sync_health` не споживаються фронтом (SyncStatus читає лише failed_count/pending_count; обгортки sync_health у offline.ts немає). Алерт на failed — є; алерт на стагнацію — відсутній.

### Рекомендація щодо приймання
Критерій ЕТАПУ 7 у поточній інтерпретації («чеки 4 кас + моніторинг-дані») — пройдено. Критерій дизайну в повному обсязі («всі дані», «алерт на стагнацію») — ні. Пропоную:
- **Варіант А (суворий):** НЕ ПРИЙНЯТО → передати Rust_Agent: (1) розширення серверного приймача push на purchase_order/inventory/transfer/write_off + переведення synced=0 → outbox (міграція даних каси); (2) UI-споживач `degraded` (алерт стагнації в SyncStatus).
- **Варіант Б (умовне приймання):** ПРИЙНЯТО як «ЕТАП 7a: моніторинг + e2e чеків», з явним винесенням «ЕТАП 7b: push не-чекових типів + зведений звіт-ендпоінт + UI-алерт стагнації» окремим контрактом. Відкриті питання §4-5 зафіксувати в дизайні (розділ 10).

Як QA-аудит, зобов'язаний зафіксувати фактичний стан: **критерій дизайну не закрито повністю → НЕ ПРИЙНЯТО (до Варіанта Б)**. Залишкові ризики §4.3.2 (created_at) і §5.1 (pull не інтегровано) — окремими контрактами, не блокують приймання моніторингу, але мають бути в реєстрі відкритих питань.

---

## Rust-доопрацювання ЕТАП 7b: статус блокерів

> Виконавець: Rust_Agent · Коміт: sync(ЕТАП 7b) · Дата: 2026-09-02
> Обсяг: ТІЛЬКИ Rust/схеми (frontend/src-tauri/crates/* + Alembic-шар сервера);
> Python backend і React — НЕ чіпались (фронт — окремий агент).

### БЛОКЕР 1 — «Всі дані на сервері» → ✅ ЗАКРИТО (Rust-частина)
- Серверний push-приймач (`sync.rs`) розширено на типи каси ЕТАПУ 6:
  **purchase_order, inventory, transfer, write_off** (аліаси purchase/
  transfer_out/transfer_in приймаються). Новий модуль `sync_receivers.rs` —
  SQL-приймачі, кожен в ОДНІЙ транзакції: документ (status `confirmed` —
  касовий факт, не чернетка власника; Python-підтвердження касових
  документів неможливе — вони вже confirmed, повторний confirm — no-op)
  + items + stock-ефект (та сама таблиця `stock` сервера: purchase +qty,
  write_off −qty, transfer ±qty за стороною каси (ctx=from → −, ctx=to → +),
  inventory — АБСОЛЮТНИЙ рівень факту).
- Ідемпотентність: попередній SELECT + catch UNIQUE-гонки
  `uq_{table}_client_uuid` (Alembic 0013) — як для чеків.
- **Каса кладе агрегати в outbox ОДРАЗУ** (`transactions.rs::enqueue_
  transaction` переписано: агрегат synced=1 + outbox pending + stock-ефект
  атомарно — контур чека enqueue_receipt). Для synced=0, накопичених
  старою версією, — `sweep_legacy_unsynced()` на початку кожного push
  (INSERT OR IGNORE за client_uuid + synced=1) — «при першому sync після
  оновлення все потрапляє в outbox».
- **Фактично мертві типи** (канал аномалій): `debtor_payment` і
  `work_session` каса офлайн НЕ генерує — `transactions.rs` має таблиці
  лише для 4 типів; фронтові форми purchase/inventory/transfer/write_off
  (ЕТАП 6) — єдині offline-шляхи. Серверні UNIQUE для debtor_payments/
  work_sessions (0013) залишаються як резерв майбутніх типів; приймачів
  для них НЕ додано (роздуття обсягу виключено). Тест sync_edge_cases
  оновлено: невідомий тип (`work_session`) → per-item error → failed видимий.

### БЛОКЕР 2 — серверний created_at → ✅ ЗАКРИТО
- Усі приймачі (чеки + 4 типи) пишуть `created_at` = RFC3339 created_at
  каси з конверта PushEnvelope (нормалізація `parse_created_at_utc`:
  RFC3339 → UTC naive; ISO без tz — як є; невалідний → now(), м'який
  fallback не ламає прийом). Для чеків — поле `ReceiptCreateInput.created_at`
  (domain+repo: `COALESCE($n::timestamp, now())` в insert_receipt, атомарно).
- E2e-доказ: sync_4stores (перший чек каси + purchase_order у минулому —
  сервер зберіг дату каси 1:1) і sync_typed_push_e2e (created_at усіх
  4 типів = 2026-08-30T07:00:00Z).

### HIGH — pull-цикл у додатку → ✅ ЗАКРИТО
- `ensure_pull_task_started()` у commands.rs (той самий патерн, що push;
  конфіг з тих самих SQLite settings; один на процес AtomicBool; spawn через
  tauri::async_runtime) — викликається з setup (lib.rs) і після set_setting.
  Каса в prod оновлює довідники; `last_pull_ok_at` у sync_health
  заповнюється реальними pull-циклами (DEFAULT_PULL_INTERVAL_SECS=30,
  дизайн 5).

### MEDIUM — гігієна тестів → ✅ ЗАКРИТО
- `tests/common/mod.rs` (torgashka-api): `force_test_db()` — env
  `DATABASE_URL` = `<робоча>_test` (TEST_DATABASE_URL має пріоритет; захист
  «ім'я БД має містити test»). Викликається першим рядком КОЖНОГО e2e
  (sync_*, cash_operations, onboarding, prro_*) — run_facade і всі пули
  процесу на _test БД.
- **Перевірка**: робоча `pos_system_fresh` після повного прогону
  (388 passed) — 0 нових записів: receipts client_uuid=263 (було 263 до
  прогону), sync_log=457 (=), stores E2E=32 (=), purchase_orders/
  inventories/transfers/write_offs з client_uuid = 0. Жоден e2e не пише в
  робочу БД.

### LOW
- (a) `in_flight` у CHECK outbox — задокументовано як «РЕЗЕРВОВАНО» прямо в
  міграції 0002 (коментар); CHECK не змінювався (recreate таблиці = ризик).
- (b) 2 шляхи помилки без журналу — закрито: невалідний JSON у 200-відповіді
  → log_event(push_fail) + defer; сервер без результату для агрегата →
  log_event(retry) + defer.
- (c) `receipts.client_receipt_uuid` — видалено: Alembic **0014** (drop
  column; перевірено grep: колонку не читає жоден код backend/Rust).
  Застосовано до pos_system_fresh і pos_system_fresh_test (alembic_version
  = 0014).

### Свідчення прогону
`cargo test --workspace`: **388 passed / 0 failed / 23 ignored** (2 у infra —
дозволені). Нові: sync_typed_push_e2e (1), оновлені: sync_4stores_outage_e2e
(+purchase через outbox, created_at чеків і закупок, серверний stock),
sync_edge_cases (невідомий тип), unit: transactions (+sweep/outbox, 9),
sync_receivers (parse/scaled/items, 5), sync_push (log_event аномалій).
До: 381 passed / 0 failed. Після: 388 passed / 0 failed.

### Відкриті питання (зафіксовані, НЕ блокують)
1. **Конвергенція схем transfers Rust/Python**: Rust-схема (schema.sql,
   pos_system_fresh_test) — `from_location/to_location varchar`; робоча
   Python-БД pos_system_fresh має ручний спадок `from_store_id/to_store_id`
   (uuid). Push-приймач transfer пише за Rust-конвенцією (from_location =
   uuid сторони як рядок). Python-стек (alembic-модель TransferCreate теж
   використовує from_location рядки, а БД — from_store_id) потребує
   окремого рішення дизайну. Тести цього не зачіпають (ізольована _test БД).
2. **Зведений ендпоінт-агрегат по точках** — як і раніше відсутній
   (відкрите питання дизайну; точність перевіряється по таблицях
   транзакцій). Не входить в Rust-обсяг.
3. **UI-алерт стагнації (degraded)** — фронтовий споживач sync_health
   робить окремий агент (React) паралельно.
