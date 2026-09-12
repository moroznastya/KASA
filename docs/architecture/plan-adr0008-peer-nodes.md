# План реалізації ADR-0008: рівноправні read-write вузли + хаб синхронізації

| Поле | Значення |
|---|---|
| Джерело істини | `docs/adr/ADR-0008-peer-nodes-sync-hub.md` (статус: Прийнято, 2026-09-12) |
| Тип | Поетапний план впровадження (не код) |
| Автор | PM_Agent (контракт від NIKO через Orchestrator) |
| Статус | Чернетка плану, очікує рішення Творця за блокерами §6 |
| Конвенція розміщення | `docs/architecture/` уже містить `refactoring-plan.md`, `analysis-report.md` → план ADR лежить поряд. ADR — у `docs/adr/`. |
| Пріоритет при розбіжності | **ADR-0008 вище за цей план**; розбіжності зафіксовано в §1 |

---

## 0. Мета і результат

Прибрати з моделі мережі концепцію «standby не може писати»: кожен вузол — повний
read-write PostgreSQL, обмін даними — наявним прикладним HTTP-протоколом
(`POST /api/v1/sync/push`, `GET /api/v1/sync/master`), центральний хаб — арбітр
спільних довідників. Трек фізичної реплікації (ЕТАП 15–20, ADR-0007) стає мертвим
і видаляється окремим, ізольованим кроком — **після** доведення нового шляху.

---

## 1. Розбіжності / уточнення до ADR-0008 (пріоритет — ADR)

Технічних помилок, які роблять план неможливим, **не виявлено**. Зафіксовані
уточнення (не суперечності рішенню):

| # | Місце ADR | Уточнення | Наслідок для плану |
|---|---|---|---|
| 1 | §7.3 п.1 «перевикористати як бібліотеку `offline/sync_push.rs`» | У `sync_push.rs:496-560` живе **ґейт Фази 3.8** (`NodeConfig::push_blocked_reason`): якщо вузол позначений як «promote-нутий», HTTP-push НЕ робиться взагалі. Форвардер node→hub буде ним **заблокований**. ADR цього не згадує. | Етап E3 має окремий підпункт: шлях форвардера не проходить через цей ґейт (новий код, не видалення — ґейт ще потрібен живим до E7) |
| 2 | §7.2 п.6 «`DELETE` … `/api/v1/local/promote`, `/api/v1/network-nodes/join`…» | Це **видалення роутів**, а не DELETE-хендлери. Фактичне підключення: `router_v1.rs:816-821` (`promote::admin_router`), `:833` (`write_gate::gate_middleware`), `:842` (`readonly_net::readonly_net_middleware`) | Точки демонтажу зафіксовані в E7 |
| 3 | §7.1-B5 + §5 №33 + §10 №1 | B5/C3 (`store_product_price`) неможливо оцінити до рішення Творця про природу цін | E5 має блокер №1; обсяг E5 — плаваючий |
| 4 | §8 п.6 (`node_config.rs`, 1149 рядків «частково») | Підтверджено 1149 рядків. **Важливо:** видаляти лише `NodeMode`/`RepointPending`/`push_blocked_reason`; `local_port`, `primary_db_url`, `load*`, `resolve_primary_db_url`, `save_to_disk` використовуються і поза реплікацією | E7: часткове видалення за списком, не файлом |
| 5 | §8 п.12 (список тестів на видалення) | Список **неповний**: додатково мертвими стають `tests/standby_dbname_selfheal_e2e.rs` і `tests/cash_operation_standby_e2e.rs` (не згадані) | E7 містить крок «повна інвентаризація за grep», не лише перелік з ADR |
| 6 | §8 vs §2 п.1 | Робота з попереднього контракту — автовизначення імені локальної БД репліки (коміт `31a0e77` «fix(standby): самолікування імені БД репліки…») — після ADR-0008 стає **мертвою** (standby-специфічна) | **УВАГА ТВОРЦЯ:** подальші інвестиції в standby-трек (у т.ч. щойно завершений self-heal) — викинуті. Аргумент не починати нових standby-задач |

---

## 2. Принципи декомпозиції

1. Етап = окремий інкремент, прийнятний **однією командою/тестом**; жодного «великого вибуху».
2. Кожен етап самодостатній: його можна змерджити й залишити системy працюючою.
3. **Спершу новий шлях — потім видалення.** Обґрунтування: поки новий синк (kinds,
   батчі, форвардер) не доведено тестами, старий standby-код — єдиний робочий
   транспорт у проде. Видалення раніше = втрата відкату і сліпі регресії.
4. Етапи, заблоковані §10, позначені явно й **не** оцінюються як «готові до старту».
5. Жодних вигаданих відповідей за Творця.

---

## 3. Таблиця етапів

| Етап | Назва (інкремент) | Залежить від | Критерій прийняття (конкретно) | Блокери §10 |
|---|---|---|---|---|
| **E0** | Базлайн і заморожування рішень | — | Зафіксовано baseline: `cargo test --workspace 2>&1 \| tail -40` (кількість зелених/ігнорованих) у `docs/audit/adr0008-baseline.md`; ADR-0007 переведено в статус `Superseded` | — |
| **E1** | Нові push-kinds батьків: B1 `debtor`, B2 `work_session`, B3 `prro_shift` + ідемпотентність `uq_debtors_client_uuid` | E0 | Новий `tests/sync_missing_parents_e2e.rs::debtor_then_payment_accepted` (сьогодні оплата боржника відхиляється — `sync_receivers.rs:680-692`); регресія зелена: `cargo test -p torgashka-api --test sync_typed_push_e2e --test sync_push_e2e` | — |
| **E1.5** | Виправити дефект: провалений запис чека виглядає як успіх (HTTP 202) | — | `cargo test -p torgashka-api --test receipt_silent_failure_e2e -- --test-threads=1` → `failed_receipt_write_must_not_look_like_success` проходить | — |
| **E2a** | Батчі: `sync_batches` + `sync_log.batch_id` + статус батча | E1 | `tests/sync_batch_e2e.rs::batch_partial_status_and_log_batch_id` (3 агрегати, 1 невалідний → `status='partial'`, у `sync_log` однаковий `batch_id`) | — |
| **E2b** | Порядок і retry: `error_class` (`RETRYABLE_FK`/`VALIDATION`/`CONFLICT`) + топосорт за DAG + `defer` замість `failed` | E2a | `tests/sync_retryable_fk_e2e.rs::child_before_parent_deferred_then_accepted` (дитина в батчі раніше батька → `deferred`, після повтору — прийнято; SQLSTATE 23503) | — |
| **E3** | Форвардер node→hub: задача на вузлі (читає outbox/`sync_log`, POST у хаб, backoff, `hub_forwarded_at/status`) + A1/A3 | E1, E2a | `tests/hub_forwarder_e2e.rs::node_forwards_receipt_to_hub` (два фасади в одному тесті: вузол приймає локальний чек → на хабі `sync_log` має запис із тим самим `batch_id`, `hub_forwarded_at IS NOT NULL`) | — |
| **E4** | E2E нового світу: «хаб недоступний» замість «вузол read-only» | E3 | `tests/hub_outage_e2e.rs::node_writes_offline_hub_down_then_syncs` (хаб лежить → вузол пише локально, `/api/v1/setup/status != 503`, черга росте; хаба піднято → дані на хабі) | — |
| **E5** | Арбітраж спільних сутностей: `catalog_change_requests` + `/api/v1/sync/catalog-proposal` + `/api/v1/admin/sync/conflicts` + pull результату + C1–C6 (довідники без версій/шляху) + D3-маркер + **1 додатковий entity-шлях: `store_product_prices` як спільна сутність** (рішення Творця від 2026-09-12: `server_version` + арбітраж D1, механізм як у `products.price`) + **`users`-шлях (Б2: касир створюється локально, маркер `sync_state`)** | E2a, E3 | `tests/catalog_proposal_e2e.rs::proposal_accepted_and_visible_on_second_node` + `::conflict_visible_in_admin_queue` (дві пропозиції на один рядок → обидві в журналі, `status='conflict'`) + **`::store_price_proposal_accepted_and_visible_on_second_node`** (пропозиція ціни з вузла → хаб присвоює єдиний `server_version` → ціна видима на другому вузлі) + **`tests/users_offline_creation_e2e.rs::{cashier_created_offline_confirmed_after_hub_contact, second_proposal_for_same_user_visible_as_conflict}`** | **№2 — ЗАКРИТО 2026-09-16**; відкриті: №4, №7 |
| **E6** | Моніторинг/гігієна: `/api/v1/sync/status`, ретеншн `sync_log`/`catalog_change_requests`, процедура бекапу хаба | E5 | `tests/sync_status_e2e.rs::status_reports_lag_queue_conflicts` | **№5, №6** |
| **E7** | **ІЗОЛЬОВАНИЙ ТРЕК ВИДАЛЕННЯ** (див. §4) | E1–E5 доведені | `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` + `cargo fmt --check`; grep-доказ: `grep -rn "NodeMode\|write_gate\|readonly_net\|standby_provision\|pg_basebackup\|pg_promote" crates/` → 0 збігів у коді | — |
| **E8** | Документація: ADR-0007 → Superseded, архів `operations/network-two-devices.md`, `setup-two-devices.md`, `disaster-recovery-network.md`, `network-replication-etap15-20.md`; новий ops-док «хаб + вузли» | E7 | Файли в `docs/operations/archive/`; новий `docs/operations/hub-and-nodes.md` існує; ADR-0007 має статус Superseded | №6 (частк.) |
| **E9** | Протокол версій схеми (major-сумісність хаб↔вузол). **Рішення прийнято 2026-09-16: варіант A** (major-версія у `schema_revision`; хаб відхиляє push із чужою major) — реалізація лише тут, у E5-частині B НЕ кодується | E5 | Схема: push від вузла з несумісною major-версією відхиляється з `error_class='VALIDATION'`; `tests/schema_version_guard_e2e.rs` | **№8 — рішення є, лишається реалізація (блокує прод-викатку)** |
| **E10** | Мережевий фіскальний аудит (розширення B3) | E5 | Визначено за рішенням Творця (нічний звіт хаба vs лише push `prro_shifts`) | **№3** |

Порядок: E0 → E1.5 → E1 → E2a → E2b → E3 → E4 → E5 → E6 → E7 → E8 (E9/E10 — паралельно, поза критичним шляхом; **E1.5 виконується паралельно з E2**).

### 3.1 E1.5 — деталі (дефект «провалений чек = 202 успіх»)

**Проблема (доведено живим прогоном 2026-09-12):** `POST /api/v2/receipts/sale` → HTTP 202,
`fiscal_status="queued"`, чек у PG = 0, чек у SQLite = 1, `outbox pending` = 1,
`has_configured_upstream=false`, `PG stock 0→0`, `GET /api/v2/receipts` чека не містить.
Касир бачить успіх, дані недоставні.

**Критерій прийняття:** `cargo test -p torgashka-api --test receipt_silent_failure_e2e -- --test-threads=1`
→ `failed_receipt_write_must_not_look_like_success` проходить.

**Залежності:** немає (незалежний від E2–E7). **Блокери:** немає. **Виконавець:** Dev_Agent.

**ЗАСТЕРЕЖЕННЯ (обов'язкове):** `receipt_silent_failure_e2e.rs` — незакомічена робота іншого
агента; НЕ послабляти, НЕ видаляти, НЕ змінювати асерти (інакше фікс фіктивний). Другий тест
цього файлу (`legacy_sqlx_pos_on_readonly_replica_is_visible_500`) — флейкі, зафіксований як
baseline-шум (не є критерієм E1.5).

**Порядок:** виконується зараз, паралельно з E2.

---

## 4. Ізольований трек видалення (E7) — обґрунтування і склад

### 4.1 Чому ПІСЛЯ E1–E5

1. **Страховка відкату**: поки хаб-протокол не доведено e2e (E4/E5), standby-код —
   єдиний працюючий транспорт у проде. Видалення раніше = нема куди відкотитись.
2. **Джерело регресійного покриття**: 5 файлів `*_standby_outbox_e2e.rs` реально
   виконують сценарії офлайн-черги; E4 замінює їхній смисл («хаб недоступний»), і
   лише після цього старі можна прибрати без втрати покриття.
3. **Розрив зв'язків**: ґейт `push_blocked_reason` (E3) і `write_gate` переплетені з
   push-шляхом; спершу треба мати альтернативний шлях, потім рвати.
4. **Baseline E0**: дає доказ, що кожен видалений тест мав заміну, а не «зник разом із фічею».

### 4.2 Склад (з ADR §8 + уточнення §1.5 цього плану)

| Підкрок | Що видаляється | Доказ обсягу |
|---|---|---|
| E7a | Тести standby-світу (13 файлів: `readonly_net_e2e.rs`, `promote_drain_e2e.rs`, `write_gate_behavior.rs`, `write_gate_guard.rs`, `adr0007_at_contract.rs`, 5× `*_standby_outbox_e2e.rs`, `facade_standby_boot_e2e.rs`, `standby_dbname_selfheal_e2e.rs`, `cash_operation_standby_e2e.rs`) | ~6000 рядків |
| E7b | Код: `standby_provision.rs` (1091), `promote.rs` (606), `write_gate.rs` (850), `readonly_net.rs` (216), `readonly_guard.rs` (389) | ~3150 |
| E7c | Точкові зрізи: `node_config.rs` (`NodeMode`, `RepointPending`, `push_blocked_reason`, `DEFAULT_LOCAL_PG_PORT`, `PRIMARY_CHECK_TIMEOUT`), `standby_heartbeat.rs` (standby-частина), `network_nodes.rs` (`join_node`, `force_resync_node`, `replication_*`), `route_local.rs:392-416` + тест `:957`, `router_v1.rs:816-842` | ~850 |
| E7d | Документація → E8 | ~77 KB |

**Критерій прийняття E7 (міряється, не «на око»):**
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
grep -rn "NodeMode\|write_gate\|readonly_net\|standby_provision\|pg_basebackup\|pg_promote\|readonly_guard" frontend/src-tauri/crates --include=*.rs   # очікується 0
```
**Ризик:** над-видалення (`network_nodes` несе RLS/реєстр вузлів — лишити; `node_config`
потрібен для `local_port`/`primary_db_url`). Мітигація: E7 робити підетапами a→d,
кожен із зеленим `cargo test --workspace`.

---

## 5. Розподіл виконавців (для координатора)

| Етап | Написання | Компіляційна валідація | Приймання |
|---|---|---|---|
| E0 | Dev_Agent (скрипт baseline) | — | QA_Agent |
| E1, E1.5, E2a, E2b | Dev_Agent | Rust_Agent (`cargo check/test/clippy/fmt`) | QA_Agent |
| E3, E4 | Dev_Agent (E3 — +Infrastructure_Master_Agent для задачі-форвардера) | Rust_Agent | QA_Agent |
| E5, E6 | Dev_Agent + System_Architect_Agent (політика арбітражу) | Rust_Agent | QA_Agent |
| E7 | Dev_Agent (**окрема гілка `chore/remove-standby-track`**) | Rust_Agent | QA_Agent + Git_Admin |
| E8 | System_Architect_Agent | — | QA_Agent |

---

## 6. Рішення Творця потрібні ДО старту (блокери)

### БЛОКУЮТЬ старт відповідних етапів

**Б2 (ADR §10 №2) — ЗАКРИТО 2026-09-16** (рішення Творця: локально, гібрид із маркером).
Див. «НЕ блокують старт» нижче — пункт більше не блокує частину E5 (users).

**Б3 (ADR §10 №4) → блокує обсяг E5/E6.** Чи потрібні вузлу чужі операційні дані
(взаємні борги/переміщення між точками)?
- Варіант A: ні («все операційне — вгору, довідники — вниз») — E5 без змін; зведені звіти лише на хабі.
- Варіант B: так — потрібен новий зворотний потік (hub → node) операційних даних, який ADR прямо виключає (§3.2 п.6) → **вимагає правки ADR**.
- Наслідок: варіант B змінює архітектуру, не лише план.

**Б4 (ADR §10 №7) → блокує рядок `audit_log` у матриці.** Мережевий аудит на хабі?
- Варіант A: так (↑) — потрібен kind + приймач.
- Варіант B: ні (локальний, `Л`) — рядок §5 №2 змінюється з ↑ на ⛔, E5 меншає.

### БЛОКУЮТЬ продакшн-викатку (не розробку)

**Б5 (ADR §10 №8) — РІШЕННЯ ПРИЙНЯТО 2026-09-16** (варіант A: major-версія у `schema_revision`,
хаб відхиляє push із чужою major). Див. «НЕ блокують старт» нижче: питання закрито, розробка не
заблокована; **прод-викатка все ще чекає на РЕАЛІЗАЦІЮ протоколу в E9** (рішення ≠ код).

**Б6 (ADR §10 №5) → блокує E6.** Ретеншн `sync_log`/`catalog_change_requests` (термін, партиціювання).
**Б7 (ADR §10 №6) → блокує E8 (ops-частина).** Процедура бекапу/копії хаба.
**Б8 (ADR §10 №3) → блокує E10.** Фіскальний аудит: лише push `prro_shifts` чи нічний звіт хаба.

### НЕ блокують старт (можна починати негайно)
Жодне з 8 питань §10 **не блокує E0–E4** (базлайн, kinds, батчі, форвардер, outage-e2e).

**Б1 (ADR §10 №1) — ЗАКРИТО 2026-09-12 (рішення Творця).** Обрано варіант B: ціна товару в точці
(`store_product_prices`) — атрибут **МЕРЕЖІ**; хаб — авторитет; вузол надсилає пропозицію, хаб
арбітрує єдиний `server_version` і роздає всім вузлам (механізм як у `products.price`, `sync.rs:278-311`).
Деталі: ADR-0008 §10 №1, §5 рядок 33, §7.1-B5. **E5 розблоковано за цим пунктом**; обсяг E5 зростає
на 1 entity-шлях (див. §3, рядок E5). Перенесено в §6.1 «НЕ блокують старт».
**РЕАЛІЗОВАНО 2026-09-30 (E5-частина B):** Alembic `0024_store_prices_sync_state.py`
(`server_version` + tombstone + `trg_store_product_prices_bump` + рядок `sync_meta`), entity у
`ALLOWED_ENTITIES` (pull ↓), арбітраж — наявний `/api/v1/sync/catalog-proposal`
(`ARBITRATED_ENTITIES` + `apply_store_product_prices`); **окремий push-kind `store_product_price`
НЕ потрібен і шкідливий** (push = «це сталося, прийми» в обхід арбітражу → last-write-wins).
Тест: `catalog_proposal_e2e::store_price_proposal_accepted_and_visible_on_second_node`.

**Б2 (ADR §10 №2) — ЗАКРИТО 2026-09-16 (рішення Творця): касир створюється ЛОКАЛЬНО на вузлі
(варіант A), але гібрид із маркером — варіант C лишається доступним одним прапорцем.**
- Локальний маркер §7.1-D3: `users.sync_state CHECK(local|pending_hub|confirmed) DEFAULT 'confirmed'`;
  створення касира НА ВУЗЛІ (є `sync.hub_url` у власній БД) → `pending_hub`; хаб при прийнятті
  пропозиції ставить `confirmed` (він — авторитет §4.2); рядок, отриманий pull'ом, канонічний за
  визначенням (DEFAULT).
- Політика входу: `REQUIRE_HUB_CONFIRM_BEFORE_LOGIN` — **default false** (offline-first, ADR §2.2:
  точка без VPN заводить касира і він одразу працює); `=true|1|yes|on` вмикає варіант C (блок входу
  до підтвердження). Рішення ОБОРОТНЕ: змінюється прапорець, не код.
- **РЕАЛІЗОВАНО 2026-09-30 (E5-частина B):** `users` у `ARBITRATED_ENTITIES` + `apply_users`
  (валідація ролі до SQL; `owner` навмисно поза набором — як у `auth_routes::parse_role`),
  маркер у `repositories/auth.rs::create_user`, політика входу в `login_common`,
  `infrastructure::sync_settings` (спільні ключі `sync.hub_url`/`sync.hub_token`).
- Тести: `users_offline_creation_e2e::{cashier_created_offline_confirmed_after_hub_contact,
  second_proposal_for_same_user_visible_as_conflict, pending_hub_login_blocked_by_flag}`.
- Залишок (клієнтський інкремент, поза крейтом API): пересилка пропозиції з вузла і локальна
  реконсиляція маркера `pending_hub → confirmed` за присвоєною хабом версією.

**Б5 (ADR §10 №8) — РІШЕННЯ ПРИЙНЯТО 2026-09-16: варіант A** — major-версія схеми у
`schema_revision`; хаб ВІДХИЛЯЄ push від вузла з чужою major (клас помилки `VALIDATION`, §4.3).
Питання закрито → **розробка E5/E6 не заблокована**. ⚠ Рішення ≠ реалізація: сам протокол версій
схеми — окремий етап **E9** (`tests/schema_version_guard_e2e.rs`), у E5-частині B він НЕ кодується;
до виконання E9 **прод-викатка мережі лишається заблокованою** (це і був сенс Б5).

---

## 7. Рекомендація: з чого почати завтра

**E1.5 (перший — гроші, критично) → E0 → E1 → E2a/E2b → E3.**

1. **E1.5 — найгостріше.** `POST /api/v2/receipts/sale` повертає 202 «успіх», коли чек фактично
   не записано в PG (чек у PG = 0, у SQLite = 1, outbox pending = 1), і `GET /api/v2/receipts`
   його не містить. Касир бачить продаж, якого немає ні в базі, ні у звітах. Виконується
   **паралельно з E2** (інші ділянки коду), без блокерів; критерій — наявний тест
   `failed_receipt_write_must_not_look_like_success` (деталі й застереження — §3.1).
2. **E0 (baseline) дешево страхує E7**: без нього неможливо довести, що видалення ~6000 рядків
   тестів не прибрало покриття без заміни.
3. **E1** закриває активний прод-дефект, не чекаючи нічого: сьогодні картка боржника, створена
   на вузлі, не доїжджає на хаб → перший же платіж відхиляється (`sync_receivers.rs:680-692`;
   kind `debtor` відсутній у `receiver_table`, `sync.rs:661-682`). B2/B3 — того ж класу.
   Нуль залежностей від §10; нуль конфлікту з треком видалення.
4. **E1.5 та E1 — фундамент E2–E3**: E1 дає e2e-каркас «батько+дитина»
   (`sync_missing_parents_e2e.rs`), E1.5 дає інваріант «202 без рядка в PG = не успіх», на який
   спираються критерії E2a (статус батча) і E3 (форвардер).

**Не починати з E7 (видалення)**: до E4/E5 немає доведеної заміни, а регресійне покриття
офлайн-черги тримається саме на тестах, які планується видалити.

*Оновлено 2026-09-16 (рішення Б2 і Б5) та 2026-09-30 (реалізація E5-частини B).*
