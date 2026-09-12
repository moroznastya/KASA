# Baseline тестів: E0 + E1 плану ADR-0008

| Поле | Значення |
|---|---|
| Контракт | ADR-0008 §7.1-B (B1/B2/B3) + `docs/architecture/plan-adr0008-peer-nodes.md` §3 (E0, E1), §7 |
| Виконавець | Dev_Agent (contract від NIKO через Orchestrator) |
| Дата | 2026-09-12 |
| HEAD на момент E0 | `e98ba05` (гілка з ADR-0007-треком уже закомічена) |
| Команда baseline | `cd frontend/src-tauri && cargo test --workspace --no-fail-fast` |
| Призначення | зафіксувати «до» ДО будь-яких змін коду і страхувати E7 (видалення ~6000 рядків тестів) |

---

## 1. Baseline (ДО змін E1) — `cargo test --workspace --no-fail-fast`

| Метрика | Значення |
|---|---|
| Цілей (тест-бінаріїв) запущено | 81 |
| `test result:` рядків | 88 (87 `ok` + 1 `FAILED`) |
| **passed** | **662** |
| **failed** | **2** |
| ignored | 0 |

### Падаючі тести baseline (baseline-шум, НЕ лікуються в межах E0/E1)

Обидва — у бінарії `torgashka-api --test receipt_silent_failure_e2e`
(**untracked** файл, створений паралельним агентом; `git status` → `??`, у HEAD
його немає — тобто це чужа незакомічена робота, не мій контракт):

1. `failed_receipt_write_must_not_look_like_success`
   ```
   ПРОВАЛЕНИЙ ЗАПИС ЧЕКА ВИГЛЯДАЄ ЯК УСПІХ: POST → HTTP 202 Accepted
   (fiscal_status="queued"), чеків у PG-джерелі істини = 0, черга доставна = false,
   PG stock 0→0, чек у списку GET /api/v2/receipts = false.
   Касир бачить «успіх», БД не змінена, черга не має куди піти.
   ```
2. `legacy_sqlx_pos_on_readonly_replica_is_visible_500` — падає у baseline
   (`receipt_silent_failure_e2e.rs:487`), **минає** в після-прогоні → тест
   **флейкі** (порядок/стан БД), не пов'язаний з E1. Фіксується як шум.

### ⚠ Проблема середовища, що спотворила перший baseline-прогін (Е0)

Перший `cargo test --workspace` **не дійшов до тестів**: диск був заповнений на
100% (`/ 198G, доступно 0`), лінкер падав ще на етапі збірки:

```
error: linking with `cc` failed: exit status: 1
= note: collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped
error: could not compile `torgashka-infrastructure` (test "pos_crud")
```

Дію: звільнено 16 GB — видалено **кеш інкрементальної компіляції**
(`target/debug/incremental`, 17 GB; `cargo` регенерує його сам, коду/артефактів
релізу це не торкається). Після цього baseline відтворюваний.

Примітка про метод: без `--no-fail-fast` `cargo test` зупиняється на першому
ж падаючому бінарії (перший прогін дав лише 33 бінарії з 81) — тому повна карта
baseline знімалася саме з `--no-fail-fast`.

---

## 2. Доведений дефект (baseline-доказ, ДО фіксу)

Прогін нового тесту ДО правок коду: `cargo test -p torgashka-api --test sync_missing_parents_e2e`
(новий тест, ДО правок коду) → `0 passed; 4 failed`:

```
test debtor_then_payment_accepted ... FAILED
  агрегат не прийнято: status="error" error="тип 'debtor' не підтримується push"
test work_session_pushed_and_idempotent ... FAILED
  агрегат не прийнято: status="error" error="тип 'work_session' не підтримується push"
test prro_shift_pushed_and_idempotent ... FAILED
  агрегат не прийнято: status="error" error="тип 'prro_shift' не підтримується push"
test payment_before_debtor_arrives_is_rejected_then_accepted ... FAILED
  (крок 1 пройдено — див. нижче; падіння на кроці 2: «тип 'debtor' не підтримується push»)
```

Прямий прод-симптом (той самий, що описаний у плані §7.1 і `sync_receivers.rs:680-692`)
— платіж боржника, який «не доїхав» на хаб:

```
[e2e][evidence] платіж по боржнику, якого немає на хабі → status="error"
  error=Боржника 5c3f6e40-dc2b-47b7-ac1c-222123286e58 не знайдено в цій точці — оплату відхилено
```

**Коренева причина = саме відсутність kind'ів** у `receiver_table` (`sync.rs`),
як і стверджував план: жодних інших причин не виявлено; `accept_debtor_payment`
працює коректно й далі (перевірка батька — pre-flight, платіж не пише нічого в БД).

---

## 3. Після E1 — `cargo test --workspace --no-fail-fast`

| Метрика | До (baseline) | Після E1 | Δ |
|---|---|---|---|
| Цілей запущено | 81 | 82 | +1 (новий бінарій) |
| `test result:` рядків | 88 | 89 | +1 (новий бінарій) |
| **passed** | **662** | **667** | **+5** |
| **failed** | **2** | **1** | **−1** |
| ignored | 0 | 0 | 0 |

`+5` = 4 нових тести `sync_missing_parents_e2e` + 1 флейкі-перехід
`legacy_sqlx_pos_on_readonly_replica_is_visible_500` (baseline: FAILED → після: ok).
Єдине падіння після E1 — той самий baseline-шум
`failed_receipt_write_must_not_look_like_success` (чужий untracked файл).
**Кількість падаючих тестів не зросла (2 → 1).**

### Цільові прогони E1

| Прогін | Результат |
|---|---|
| `sync_missing_parents_e2e` (4 тести) | ✅ 4 passed / 0 failed (було 0/4) |
| `sync_push_e2e` (регресія) | ✅ 2 passed / 0 failed |
| `sync_typed_push_e2e` (регресія) | ✅ 1 passed / 0 failed |
| `sync_edge_cases_e2e` | ✅ 3 passed / 0 failed (тест «невідомий тип» переведено на синтетичний kind — див. §5) |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 попереджень (`Finished`) |
| `cargo fmt --check` | ⚠️ 1 файл — `tests/receipt_silent_failure_e2e.rs` (чужий untracked, див. §5); мої файли чисті |

---

## 4. Реєстр baseline-шуму (не лікується цим контрактом)

| # | Шум | Доказ «це не E1» | Рішення |
|---|---|---|---|
| 1 | `receipt_silent_failure_e2e::failed_receipt_write_must_not_look_like_success` падає до і після | той самий текст паніки у baseline і після; файл untracked (`??`), у моєму `git diff` відсутній | залишено як є; лікування — окремий контракт (трек ADR-0007/E7) |
| 2 | `legacy_sqlx_pos_on_readonly_replica_is_visible_500` флейкі | у baseline FAILED, після E1 ok; файл untracked і поза моїм дифом | зафіксовано як флейкі |
| 3 | `cargo fmt --check` не чистий: 8 діфів у `tests/receipt_silent_failure_e2e.rs` | той самий untracked файл; мої 6 файлів + 2 нових — `fmt`-чисті (перевірено `rustfmt --check` по кожному) | НЕ форматував (чужa незакомічена робота в процесі — конфлікт інтересів) |
| 4 | Перший baseline-прогін не зібрався через 100% диска | `ld ... signal 7 [Bus error]` | звільнено 16 GB кешем incremental; baseline перезапущено |

---

## 5. Що змінено в межах E1 (приріст, без видалення)

| Файл | Зміна |
|---|---|
| `crates/torgashka-api/src/sync.rs` | `receiver_table`: `debtor`→`debtors`, `work_session`→`work_sessions`, `prro_shift`→`prro_shifts`; диспетчер `accept_non_receipt_kind` — 3 нові гілки; розширено діагностичний текст «тип не підтримується» |
| `crates/torgashka-api/src/sync_receivers.rs` | +`accept_debtor` (зберігає UUID вузла — інакше FK оплат не резолвиться), +`accept_work_session` (payload = той самий, що формує вузол у `offline/transactions.rs`), +`accept_prro_shift` (payload = колонки `prro_shifts`, `status` → PG-enum) |
| `backend/alembic/versions/0019_peer_parents_push_kinds.py` | **нова**: `debtors.client_uuid` + `uq_debtors_client_uuid`, `prro_shifts.client_uuid` + `uq_prro_shifts_client_uuid` (патерн DROP IF EXISTS + CREATE, як 0013/0016/0017/0018) |
| `crates/torgashka-infrastructure/src/schema.sql` | дзеркало тієї самої міграції для fresh Rust-БД (ідемпотентно) |
| `crates/torgashka-api/tests/common/sync_schema.rs` | дзеркало для Rust-e2e (той самий DDL-shape, що Alembic) |
| `crates/torgashka-api/tests/sync_missing_parents_e2e.rs` | **новий**: `debtor_then_payment_accepted` (критерій приймання), `payment_before_debtor_arrives_is_rejected_then_accepted` (прямий доказ симптому + retry не отруєний), `work_session_pushed_and_idempotent`, `prro_shift_pushed_and_idempotent` |
| `crates/torgashka-api/tests/sync_edge_cases_e2e.rs` | тест «невідомий тип» переведено з `work_session` на синтетичний `unknown_aggregate_e2e` (після E1 `work_session` — підтримуваний kind; механізм той самий) |
| `docs/adr/ADR-0007-standby-write-routing.md` | статус → **Superseded** (E0), посилання на ADR-0008 + план E7; файл збережено як історичний запис |

**НЕ чіпалось (доведено `git status`):** `write_gate.rs`, `promote.rs`,
`standby_provision.rs`, `readonly_net.rs`, `readonly_guard.rs`, `node_config.rs`
(предмет ізольованого етапу E7).

### Підводний камінь §1.1 плану (ґейт `push_blocked_reason`) — перевірено

Ґейт `offline/sync_push.rs:496-560` спрацьовує **до** відправки і **не залежить
від kind**: `push_pending_batch_with_node` повертає `Ok{sent=0, gated=true}`
(жодного HTTP) на основі стану вузла, а `pending_outbox` фільтрує лише за
`status='pending'` (без списку типів). Отже: нові kinds ґейтом не ламаються —
або весь батч іде (ґейт відкритий), або жоден (вузол promote-нутий). Ґейт
лишається живим (видаляється лише на E7).
