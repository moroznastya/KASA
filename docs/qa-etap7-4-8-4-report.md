# QA-звіт: ЕТАП 7.4 (негативний RLS-тест) + ЕТАП 8.4 (тест ідемпотентності)

> Дата: 2026-09-01 (гілка `feat/rust-migration`, репозиторій `Projects/kasa`)
> Виконавець: QA_Agent (за контрактом ЕТАП 7.4 / 8.4 плану `Projects/database-architecture-implementation-plan.md`)
> Статус: ✅ ЗЕЛЕНИЙ — `cargo test --workspace` проходить повністю

---

## 1. Що додано (тільки тести + test-fixtures, продакшн-код НЕ змінено)

| Файл | Призначення |
|---|---|
| `frontend/src-tauri/crates/torgashka-infrastructure/tests/common/mod.rs` | Спільні фікстури: створення тестової БД (`<dbname>_test`), FORCE RLS, тестова роль `torgashka_test_app` (НЕ власник, НЕ superuser, НЕ BYPASSRLS), гранти, set/reset RLS-контексту |
| `frontend/src-tauri/crates/torgashka-infrastructure/tests/rls_security.rs` | ЕТАП 7.4 — 4 тести RLS |
| `frontend/src-tauri/crates/torgashka-infrastructure/tests/receipt_idempotency.rs` | ЕТАП 8.4 — 2 тести ідемпотентності |

Тестова БД: `pos_system_fresh_test` (похідна від `backend/.env` → `DB_NAME=pos_system_fresh` + суфікс `_test`, або `TEST_DATABASE_URL`). Створена як копія `pos_system_ci_test` (містить дані: 1124 чеки — потрібно для доказу «таблиця не порожня»), на неї застосовано FORCE RLS (міграція 0005) та UNIQUE-індекс ідемпотентності (міграція 0006). Роль `torgashka_test_app` — виділена ТЕСТОВА роль, ізольована від продакшн-ролі `torgashka_app`.

## 2. ЕТАП 7.4 — RLS: результати тестів

### 2.1 Негативний тест: `rls_blocks_without_context_not_because_empty` ✅
Доводить, що **0 рядків — саме через RLS, а не через порожню таблицю**:
- **Доказ A (таблиця не порожня):** контрольний `SELECT count(*)` під адміном (postgres, superuser) для `receipts` = **1124** (>0), для `stock` свого тестового рядка = **1**;
- **Доказ B (RLS активний і застосовується до ролі):**
  - `relrowsecurity = t`, `relforcerowsecurity = t` для `receipts` і `stock`;
  - власник таблиці — `postgres`, роль `torgashka_test_app` **НЕ власник** → RLS застосовується (FORCE додатково закриває шлях власника);
  - `has_table_privilege(role, table, 'SELECT') = true` → 0 рядків — фільтр RLS, а не помилка прав;
- **Доказ C (контекст відсутній):** `current_setting('app.store_id', true)` = NULL (свіже з'єднання) / `''` (після reset) — контексту немає;
- **Доказ D (результат):** під роллю додатка БЕЗ `set_config('app.store_id')` → `SELECT` з `receipts` і `stock` повертає **0 рядків**.

**Чутливість доведена:** при `DISABLE ROW LEVEL SECURITY` тест **падає** (0→побачив рядки), після включення RLS — проходить (критерій «падає ДО фіксу, проходить ПІСЛЯ» виконано).

### 2.2 Позитивний тест: `rls_with_context_shows_only_own_store_and_owner_all_stores` ✅
- cashier1 (user_stores = {A}), контекст `app.store_id = A` → бачить **1** чек (свою точку A), чек точки B **не видимий**;
- owner1 (user_stores = {A, B}), контекст `app.store_id = A` → бачить **2** чеки (обидві свої точки через `user_stores`) — друга гілка політики;
- політика `stores_access` (за `user_id`): cashier бачить **1** точку, owner — **2**;
- зафіксовано фактичну семантику політики 0004: RLS = `ctx-точка ∪ user_stores` (branch 1 ∪ branch 2); авторизація user→store (який контекст можна ставити) — обов'язок middleware (X-Store-Id + JWT), RLS гарантує ізоляцію РЯДКІВ за точкою.

### 2.3 WITH CHECK: `rls_with_check_rejects_insert_into_foreign_store` ✅
- INSERT у власну точку (у контексті своєї) — проходить;
- INSERT з `store_id` чужої точки → **42501 insufficient_privilege** (RLS WITH CHECK), рядок не створено (контроль під адміном: count = 0).

### 2.4 Схема: `force_rls_applied_to_all_store_tables` ✅
FORCE RLS застосовано до **всіх 23** бізнес-таблиць (список 0005) + `stores`/`user_stores`.

## 3. ЕТАП 8.4 — ідемпотентність: результати тестів

### 3.1 Дублікат: `duplicate_client_receipt_uuid_creates_single_row_and_single_stock_deduction` ✅
Сценарій «обрив після успішного запису» (під роллю додатка, RLS-контекст точки):
1. **Перша відправка:** `INSERT receipts (client_receipt_uuid = U)` + `UPDATE stock SET quantity = quantity - 1` в одній транзакції → COMMIT;
2. **Обрив:** відповідь не отримана (симуляція — нічого не робимо);
3. **Друга відправка того самого U:** `23505 unique_violation` (UNIQUE-індекс `uq_receipts_client_uuid`) → сервер трактує як **200/409** (результат першого запису), транзакція відкочена → **НЕ 500**;
4. **Підсумок:** `COUNT(receipts WHERE client_receipt_uuid = U)` = **1** (другий запис НЕ створено), `stock.quantity` = **9** (списано ОДИН раз, а не 8);
5. Шлях 200/409 напряму: `INSERT ... ON CONFLICT (client_receipt_uuid) WHERE client_receipt_uuid IS NOT NULL DO NOTHING` → **0 рядків зачеплено**, запис і залишок не змінюються.

**Чутливість доведена:** при видаленні UNIQUE-індексу тест **падає** (повторна вставка проходить → дублікат), після відновлення індексу — проходить.

### 3.2 Схема: `unique_index_on_client_receipt_uuid_exists_and_is_partial` ✅
- індекс `uq_receipts_client_uuid` існує, `indisunique = t`, partial (`indpred IS NOT NULL` → NULL дозволений для старих чеків);
- колонка `receipts.client_receipt_uuid` існує (міграція 0006).

> Клієнтська частина (генерація `client_receipt_uuid` при `save_receipt_offline`, стабільність у черзі) вже покрита тестом `offline_store_id.rs::receipt_queue_generates_client_uuid` — не дублювалась.

## 4. Запуск: `cargo test --workspace` (frontend/src-tauri)

```
40 тестових цілей, 0 FAILED, 262 тести пройдено
```
Включно з новими:
- `tests/rls_security.rs` — 4 passed
- `tests/receipt_idempotency.rs` — 2 passed

Повторність: повний сьют `torgashka-infrastructure` (15 цілей) — 3 запуски поспіль без фейлів (перевірка паралельних гонок).

## 5. Поведінка без PostgreSQL (CI без сервісу БД)

Усі 6 нових тестів коректно пропускаються (SKIP) з повідомленням у stderr:
`[rls_security] SKIP: PostgreSQL недоступний/не налаштований — <причина>` — тест проходить як `ok` без падіння (патерн існуючих тестів не ламає CI).

## 6. Примітки та аномалії

1. **Тестова БД створена як fixture:** `pos_system_fresh_test` (копія `pos_system_ci_test` + FORCE RLS + індекс 0006 + роль `torgashka_test_app`). `common/mod.rs::ensure_fixture()` ідемпотентно відтворює цей стан на чистому CI (CREATE DATABASE ... TEMPLATE torgashka_template → DDL 0006 → FORCE RLS → роль/гранти).
2. **PostgreSQL-квірк (документовано в тесті):** `current_setting('app.store_id', true)` = NULL на свіжому з'єднанні, `''` після set+reset — обидва стани = «контексту немає» (споживачі через `NULLIF`).
3. **Інфраструктурна аномалія (поза скоупом):** під час роботи диск був заповнений на 100% — пошкоджений бінарник `cash_operations_e2e` (SIGSEGV) у `torgashka-api`. Виправлено перекомпіляцією тесту; фінальний workspace-прогін повністю зелений. Кліпі-помилки у `tests/write_crud.rs` (незакомічені зміни інших агентів) — поза цим контрактом.
4. **Семантика політики 0004 (зафіксовано тестом, а не змінено):** RLS ізолює рядки за `ctx-точка ∪ user_stores`; авторизація «який X-Store-Id може ставити користувач» — відповідальність middleware. Якщо потрібен додатковий контур «касир не бачить чужу точку навіть за її X-Store-Id» — це зміна політики 0004 (поза скоупом тестів, потребує окремого рішення).
5. **Продакшн-код не змінювався** — `git status`: лише 3 нові файли тестів (untracked).

## 7. Файли

```
frontend/src-tauri/crates/torgashka-infrastructure/tests/common/mod.rs
frontend/src-tauri/crates/torgashka-infrastructure/tests/rls_security.rs
frontend/src-tauri/crates/torgashka-infrastructure/tests/receipt_idempotency.rs
```
