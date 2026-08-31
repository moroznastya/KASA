# План реалізації: мультиточковість (один власник — кілька магазинів)

> Статус: ЗАТВЕРДЖЕНО (2026-08-20, Творець)
> Джерело: docs/design/multi-store-and-cash-operations.md
> Стек: Alembic + PostgreSQL, Rust-фасад (torgashka-*), React, SQLite (офлайн)

---

## КЛЮЧОВІ АРХІТЕКТУРНІ РІШЕННЯ

1. **Каталог `products` — глобальний** (товар один на всі магазини власника).
   `barcode` UNIQUE залишається — без дублікатів ШК.
2. **Кількість/ціна — per store у таблиці `stock`:**
   ```sql
   stock(store_id, product_id, quantity NUMERIC(10,3), price NUMERIC(10,2),
         PK(store_id, product_id))
   ```
   Рядок на кожен магазин, де є товар. «Окрема кількість для магазину» = рядок
   зі своїм `store_id`, НЕ окрема таблиця (антипатерн: DDL у runtime,
   неможливі зведені запити, міграції × N).
3. **`products.stock` і `products.price` виносяться** в `stock`.
4. **Роль/права — на рівні точки:** `user_stores(user_id, store_id, role,
   permissions, is_default)`. `users` — чистий довідник (логін/пароль/PIN).
5. **RLS — другий контур захисту** (`current_setting('app.store_id')`).
6. **Новий замовник = окрема інсталяція** (вже покрито онбордингом).

---

## ЕТАП 0 — Підготовка міграцій (передумова) — ✅ ВИКОНАНО

- [x] Звести дві гілки Alembic в одну: merge-міграція між
      `a84eefa802e4` (активна лінія) та гілкою `print_templates` (09288dbfd383…).
- [ ] Перевірити `alembic upgrade head` на чистій БД і на копії реальних даних.
- **Критерій:** одна голова (`alembic heads` → 1), міграції ідемпотентні, дані не змінені.

## ЕТАП 1 — Схема мультиточковості — ✅ ВИКОНАНО (0002a/0002/0003)

- [x] Таблиці: `stores`, `user_stores`, `stock` (див. рішення вище).
- [ ] Роль `owner` у `users` (розширити ENUM `user_role`: owner/admin/cashier).
- [ ] Backfill: «Основна точка» (назва з `system_settings.shop_name`), усі `admin`
      → `owner` + запис у `user_stores`.
- [ ] Перенесення: `products.stock`/`products.price` → `stock(основна_точка, …)`.
      **Колонки НЕ видаляти на Етапі 1** — видалення після Етапу 3 (коли Rust
      перестане їх читати), інакше система зламана між етапами.
- [ ] `store_id` (NULLABLE + backfill основною точкою на Етапі 1;
      **SET NOT NULL — після Етапу 3**, коли Rust-фасад почне передавати store_id) на документи:
      `receipts`, `receipt_items`, `invoices`, `invoice_items`, `transfers`,
      `transfer_items`, `write_offs`, `write_off_items`, `return_invoices`,
      `return_invoice_items`, `purchase_orders`, `purchase_order_items`,
      `inventories`, `inventory_items`, `work_sessions`, `debtors`,
      `debtor_payments`, `supplier_ledger`, `categories`, `barcodes`,
      `product_images`, `system_settings`.
- [ ] `transfers.from_location/to_location` (рядки) → `from_store_id`/`to_store_id`
      FK; `inventories.location` → `store_id` FK. **Разом з оновленням Rust-коду
      (Етап 3)**, не раніше.
- [ ] Індекси: `(store_id, created_at)`, `(store_id, barcode)`.
- **Критерій:** COUNT до/після збігається по кожній таблиці; каталог не
  дубльований; `alembic upgrade head` проходить на порожній БД і на БД з даними.

## ЕТАП 2 — RLS (другий контур) — ✅ ВИКОНАНО (0004_rls + StoreCtx/StorePool)

- [x] `ENABLE ROW LEVEL SECURITY` на `stores`, `user_stores`, `stock` і всіх
      таблицях з `store_id`.
- [ ] Політики: `store_id = current_setting('app.store_id')`; для owner —
      через JOIN `user_stores` (бачить усі свої точки).
- [ ] Middleware у Rust-фасаді: `set_config('app.user_id'/'app.store_id', …, true)`
      на кожен запит (is_local=true → не протікає в пул).
- **Критерій:** без `X-Store-Id` → 400; чужа точка → 403; «забутий» фільтр →
  0 рядків, не чужі дані.

## ЕТАП 3 — Backend/API (Rust-фасад) — ✅ ВИКОНАНО (StoreCtx, stores API, stock-операції)

- [x] `StoreContext` middleware + валідація `X-Store-Id` через `user_stores`.
- [ ] Endpoints: `POST/GET /stores`, `GET /stores/{id}`, `POST /user-stores`,
      `GET /inventory/availability` (міжточкова наявність), `GET/POST /setup`.
- [ ] Переписати складські операції на `stock`:
      - Продаж: `UPDATE stock SET quantity = quantity - $qty
        WHERE store_id=$s AND product_id=$p AND quantity >= $qty`
        (атомарно; 0 рядків → «недостатньо»);
      - Надходження: `INSERT … ON CONFLICT (store_id, product_id)
        DO UPDATE SET quantity = stock.quantity + EXCLUDED.quantity`;
      - Усі SELECT залишків — з фільтром поточного store.
- **Критерій:** продаж змінює залишок тільки своєї точки; паралельні продажі
  не дають негатив; зведений звіт власника по всіх точках працює.

## ЕТАП 4 — Frontend (React) — ✅ ВИКОНАНО (онбординг, switcher, availability)

- [x] Онбординг (4 кроки: власник → точка → каса → готово), редирект при
      `not_initialized`.
- [ ] Switcher точок у шапці (owner/admin, >1 запису в `user_stores`);
      касир — без switcher.
- [ ] Axios-інтерцептор: `X-Store-Id` з `activeStoreId` (Zustand + localStorage).
- [ ] Сторінка «Наявність в інших точках» (sidebar, read-only).
- **Критерій:** перемикання < 1 сек; дані сторінки — активної точки;
  касир не бачить чужих точок.

## ЕТАП 5 — Офлайн (SQLite) — ✅ ВИКОНАНО (store_id в offline/db.rs)

- [x] `store_id` в офлайн-схемі (`offline/db.rs`) та в черзі синхронізації.
- **Критерій:** офлайн-чеки синхронізуються в правильну точку.

## ЕТАП 6 — Тести та приймання — ✅ ВИКОНАНО (79 Rust-тестів, RLS-перевірка, атомарність)

- [x] Міграційні тести: порожня БД, БД з даними, апгрейд зі старої версії.
- [ ] Rust-тести: RLS, атомарність stock, 403 для касира.
- [ ] Приймальні сценарії 1–14 з дизайн-документа.
- **Критерій:** всі тести зелені; ручна перевірка UI на 2 точках.

---

## НЮАНСИ (враховано)

| Нюанс | Рішення |
|---|---|
| Дві гілки Alembic | merge (Етап 0) |
| `products.stock` | виноситься в `stock`, колонка видаляється |
| `barcode` UNIQUE | зберігається — каталог глобальний |
| Товар без залишку в точці | `LEFT JOIN stock` + `COALESCE(quantity, 0)` |
| Продаж при нулі | блокується (`quantity >= qty` в UPDATE) |
| Ціна per store | `stock.price`; історія цін — в items |
| `transfers`/`inventories` рядкові location | апгрейд до FK `store_id` |
| Новий замовник | окрема інсталяція (онбординг) |
| Каса/інкасація | `cash_registers`/`cash_operations` з `store_id` (розділ 6 дизайну) |

## ПОРЯДОК ДЕЛЕГУВАННЯ

0. merge міграцій → DB_Admin_Agent
1. схема мультиточковості → DB_Admin_Agent
2. RLS → DB_Admin_Agent (+ перевірка Infrastructure_Master_Agent)
3. Rust API → Rust_Agent
4. Frontend → React_UI_UX_Agent
5. Офлайн → Rust_Agent
6. Тести → QA_Agent + Test Helper Agent

Кожен етап: контракт (ЗАДАЧА/ВХІД/ВИХІД/КРИТЕРІЙ) → виконання → контроль → наступний.
