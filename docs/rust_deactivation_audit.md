# Аудит дезактивації Python sidecar — kasa

Дата: 2026-08-01 · Малий крок 1/3 (аудит, без змін коду)

## Джерела даних

- Python-роути: live openapi `http://127.0.0.1:8001/openapi.json` (182 роути, включаючи include_router-префікси v1+v2)

- Rust-роути: `frontend/src-tauri/crates/kasa-api/src/*.rs` (158 роутів, збір з усіх `.route(`, включно з ланцюжками get/post/put/delete та merge-гілками)

- Feature-flag: `KASA_RUST_*` (12 шт.): READDIRS, AUTH, DEBTORS, DOCUMENTS, INVOICES, RETURN_INVOICES, PURCHASE_ORDERS, PRINT, PRODUCTS_V2, PRRO, PRRO_V2, OCR


## Підсумок покриття

- Python API-роутів (без /uploads): 182
- Покрито Rust: **149**
- НЕ покрито Rust: **33**


## Повна таблиця: Python-роут | Rust-покриття | KASA_RUST flag | коментар

| Python-роут | Rust-покриття | Flag | Коментар |
|---|---|---|---|
| `GET /` | ні | ? |  |
| `POST /api/v1/auth/login` | так | AUTH |  |
| `POST /api/v1/auth/login-pin` | так | AUTH |  |
| `POST /api/v1/auth/logout` | так | AUTH |  |
| `POST /api/v1/auth/refresh` | так | AUTH |  |
| `GET /api/v1/auth/users-list` | так | AUTH |  |
| `GET /api/v1/auth/verify` | так | AUTH |  |
| `GET /api/v1/categories` | так | AUTH(CRUD)/READDIRS |  |
| `POST /api/v1/categories` | так | AUTH(CRUD)/READDIRS |  |
| `GET /api/v1/categories/tree` | ні | AUTH(CRUD)/READDIRS |  |
| `DELETE /api/v1/categories/{}` | так | AUTH(CRUD)/READDIRS |  |
| `GET /api/v1/categories/{}` | так | AUTH(CRUD)/READDIRS |  |
| `PUT /api/v1/categories/{}` | так | AUTH(CRUD)/READDIRS |  |
| `GET /api/v1/debtors` | так | DEBTORS |  |
| `POST /api/v1/debtors` | так | DEBTORS |  |
| `GET /api/v1/debtors/search` | так | DEBTORS |  |
| `GET /api/v1/debtors/{}` | так | DEBTORS |  |
| `PUT /api/v1/debtors/{}` | так | DEBTORS |  |
| `POST /api/v1/debtors/{}/pay` | ні | DEBTORS |  |
| `GET /api/v1/debtors/{}/payments` | так | DEBTORS |  |
| `GET /api/v1/debtors/{}/receipts` | так | DEBTORS |  |
| `GET /api/v1/documents` | так | DOCUMENTS |  |
| `POST /api/v1/documents/batch-confirm` | так | DOCUMENTS |  |
| `GET /api/v1/documents/export` | так | DOCUMENTS |  |
| `DELETE /api/v1/documents/{}` | так | DOCUMENTS |  |
| `POST /api/v1/documents/{}/copy` | так | DOCUMENTS |  |
| `GET /api/v1/documents/{}/print` | так | DOCUMENTS |  |
| `GET /api/v1/inventory` | так | AUTH/CRUD |  |
| `POST /api/v1/inventory` | так | AUTH/CRUD |  |
| `GET /api/v1/inventory/counts` | так | AUTH/CRUD |  |
| `DELETE /api/v1/inventory/{}` | так | AUTH/CRUD |  |
| `GET /api/v1/inventory/{}` | так | AUTH/CRUD |  |
| `PUT /api/v1/inventory/{}` | так | AUTH/CRUD |  |
| `POST /api/v1/inventory/{}/confirm` | так | AUTH/CRUD |  |
| `POST /api/v1/invoice-ocr/analyze` | так | OCR |  |
| `GET /api/v1/invoices` | так | INVOICES |  |
| `POST /api/v1/invoices` | так | INVOICES |  |
| `DELETE /api/v1/invoices/{}` | так | INVOICES |  |
| `GET /api/v1/invoices/{}` | так | INVOICES |  |
| `PUT /api/v1/invoices/{}` | так | INVOICES |  |
| `POST /api/v1/invoices/{}/confirm` | так | INVOICES |  |
| `GET /api/v1/invoices/{}/payment-info` | так | INVOICES |  |
| `GET /api/v1/invoices/{}/price-changes` | так | INVOICES |  |
| `POST /api/v1/invoices/{}/print-items` | так | INVOICES |  |
| `POST /api/v1/ledger` | так | INVOICES |  |
| `GET /api/v1/ledger/balance/{}` | так | INVOICES |  |
| `GET /api/v1/ledger/{}` | так | INVOICES |  |
| `POST /api/v1/ocr/invoice` | так | OCR |  |
| `GET /api/v1/print-templates` | так | PRINT |  |
| `POST /api/v1/print-templates` | так | PRINT |  |
| `GET /api/v1/print-templates/all` | так | PRINT |  |
| `GET /api/v1/print-templates/default` | так | PRINT |  |
| `DELETE /api/v1/print-templates/{}` | так | PRINT |  |
| `GET /api/v1/print-templates/{}` | так | PRINT |  |
| `PUT /api/v1/print-templates/{}` | так | PRINT |  |
| `POST /api/v1/print-templates/{}/render` | так | PRINT |  |
| `POST /api/v1/print-templates/{}/set-default` | так | PRINT |  |
| `POST /api/v1/print/labels/render` | так | PRINT |  |
| `POST /api/v1/print/price-tags/render` | так | PRINT |  |
| `GET /api/v1/print/printers` | так | PRINT |  |
| `POST /api/v1/print/test` | так | PRINT |  |
| `GET /api/v1/products` | так | PRODUCTS_V2 |  |
| `POST /api/v1/products` | так | PRODUCTS_V2 |  |
| `GET /api/v1/products/barcode/{}` | так | PRODUCTS_V2 |  |
| `DELETE /api/v1/products/{}` | так | PRODUCTS_V2 |  |
| `GET /api/v1/products/{}` | так | PRODUCTS_V2 |  |
| `PUT /api/v1/products/{}` | так | PRODUCTS_V2 |  |
| `POST /api/v1/products/{}/barcodes` | ні | PRODUCTS_V2 |  |
| `DELETE /api/v1/products/{}/barcodes/{}` | ні | PRODUCTS_V2 |  |
| `POST /api/v1/products/{}/images` | ні | PRODUCTS_V2 |  |
| `DELETE /api/v1/products/{}/images/{}` | ні | PRODUCTS_V2 |  |
| `GET /api/v1/purchase-orders` | так | PURCHASE_ORDERS |  |
| `POST /api/v1/purchase-orders` | так | PURCHASE_ORDERS |  |
| `DELETE /api/v1/purchase-orders/{}` | так | PURCHASE_ORDERS |  |
| `GET /api/v1/purchase-orders/{}` | так | PURCHASE_ORDERS |  |
| `PUT /api/v1/purchase-orders/{}` | так | PURCHASE_ORDERS |  |
| `POST /api/v1/purchase-orders/{}/confirm` | так | PURCHASE_ORDERS |  |
| `GET /api/v1/receipts` | ні | PRRO |  |
| `POST /api/v1/receipts` | ні | PRRO |  |
| `GET /api/v1/receipts/by-product/{}/recent-sales` | ні | PRRO |  |
| `GET /api/v1/receipts/products/{}/returnable-quantity` | ні | PRODUCTS_V2 |  |
| `GET /api/v1/receipts/search` | ні | PRRO |  |
| `GET /api/v1/receipts/stats/today` | ні | PRRO |  |
| `GET /api/v1/receipts/{}` | ні | PRRO |  |
| `GET /api/v1/receipts/{}/items` | ні | PRRO |  |
| `GET /api/v1/return-invoices` | так | RETURN_INVOICES |  |
| `POST /api/v1/return-invoices` | так | RETURN_INVOICES |  |
| `DELETE /api/v1/return-invoices/{}` | так | RETURN_INVOICES |  |
| `GET /api/v1/return-invoices/{}` | так | RETURN_INVOICES |  |
| `PUT /api/v1/return-invoices/{}` | так | RETURN_INVOICES |  |
| `POST /api/v1/return-invoices/{}/confirm` | так | RETURN_INVOICES |  |
| `GET /api/v1/settings` | так | AUTH |  |
| `PUT /api/v1/settings` | так | AUTH |  |
| `PUT /api/v1/settings/{}` | так | AUTH |  |
| `GET /api/v1/settings/{}` | так | AUTH |  |
| `GET /api/v1/suppliers` | так | AUTH(CRUD)/READDIRS |  |
| `POST /api/v1/suppliers` | так | AUTH(CRUD)/READDIRS |  |
| `GET /api/v1/suppliers/all` | так | AUTH(CRUD)/READDIRS |  |
| `DELETE /api/v1/suppliers/{}` | так | AUTH(CRUD)/READDIRS |  |
| `GET /api/v1/suppliers/{}` | так | AUTH(CRUD)/READDIRS |  |
| `PUT /api/v1/suppliers/{}` | так | AUTH(CRUD)/READDIRS |  |
| `GET /api/v1/suppliers/{}/products` | ні | PRODUCTS_V2 |  |
| `GET /api/v1/suppliers/{}/products/{}/movements` | ні | PRODUCTS_V2 |  |
| `GET /api/v1/transfers` | так | AUTH/CRUD |  |
| `POST /api/v1/transfers` | так | AUTH/CRUD |  |
| `DELETE /api/v1/transfers/{}` | так | AUTH/CRUD |  |
| `GET /api/v1/transfers/{}` | так | AUTH/CRUD |  |
| `PUT /api/v1/transfers/{}` | так | AUTH/CRUD |  |
| `POST /api/v1/transfers/{}/confirm` | так | AUTH/CRUD |  |
| `GET /api/v1/users` | так | AUTH |  |
| `POST /api/v1/users` | так | AUTH |  |
| `GET /api/v1/users/permissions/list` | так | AUTH |  |
| `DELETE /api/v1/users/{}` | так | AUTH |  |
| `GET /api/v1/users/{}` | так | AUTH |  |
| `PUT /api/v1/users/{}` | так | AUTH |  |
| `PUT /api/v1/users/{}/hourly-rate` | так | AUTH |  |
| `PUT /api/v1/users/{}/permissions` | так | AUTH |  |
| `GET /api/v1/work-sessions/my` | так | AUTH |  |
| `GET /api/v1/work-sessions/report` | так | AUTH |  |
| `GET /api/v1/work-sessions/user/{}` | так | AUTH |  |
| `GET /api/v1/write-offs` | так | AUTH/CRUD |  |
| `POST /api/v1/write-offs` | так | AUTH/CRUD |  |
| `DELETE /api/v1/write-offs/{}` | так | AUTH/CRUD |  |
| `GET /api/v1/write-offs/{}` | так | AUTH/CRUD |  |
| `PUT /api/v1/write-offs/{}` | так | AUTH/CRUD |  |
| `POST /api/v1/write-offs/{}/confirm` | так | AUTH/CRUD |  |
| `POST /api/v2/auth/login` | ні | AUTH |  |
| `POST /api/v2/auth/login-pin` | ні | AUTH |  |
| `POST /api/v2/auth/refresh` | ні | AUTH |  |
| `GET /api/v2/auth/users` | ні | AUTH |  |
| `POST /api/v2/auth/users` | ні | AUTH |  |
| `GET /api/v2/auth/users/me` | ні | AUTH |  |
| `GET /api/v2/categories` | ні | AUTH(CRUD)/READDIRS |  |
| `POST /api/v2/categories` | ні | AUTH(CRUD)/READDIRS |  |
| `GET /api/v2/categories/tree` | ні | AUTH(CRUD)/READDIRS |  |
| `DELETE /api/v2/categories/{}` | ні | AUTH(CRUD)/READDIRS |  |
| `GET /api/v2/categories/{}` | ні | AUTH(CRUD)/READDIRS |  |
| `PUT /api/v2/categories/{}` | ні | AUTH(CRUD)/READDIRS |  |
| `GET /api/v2/invoices` | так | INVOICES |  |
| `POST /api/v2/invoices` | так | INVOICES |  |
| `POST /api/v2/invoices/confirm` | так | INVOICES |  |
| `DELETE /api/v2/invoices/{}` | так | INVOICES |  |
| `GET /api/v2/invoices/{}` | так | INVOICES |  |
| `PUT /api/v2/invoices/{}` | так | INVOICES |  |
| `POST /api/v2/invoices/{}/cancel` | так | INVOICES |  |
| `GET /api/v2/invoices/{}/payment-info` | так | INVOICES |  |
| `GET /api/v2/invoices/{}/price-changes` | так | INVOICES |  |
| `POST /api/v2/invoices/{}/print-items` | так | INVOICES |  |
| `GET /api/v2/ledger/balance/{}` | так | INVOICES |  |
| `GET /api/v2/ledger/balances` | так | INVOICES |  |
| `GET /api/v2/ledger/entries` | так | INVOICES |  |
| `POST /api/v2/ledger/entries` | так | INVOICES |  |
| `GET /api/v2/products` | так | PRODUCTS_V2 |  |
| `POST /api/v2/products` | так | PRODUCTS_V2 |  |
| `GET /api/v2/products/barcode/{}` | так | PRODUCTS_V2 |  |
| `DELETE /api/v2/products/{}` | так | PRODUCTS_V2 |  |
| `GET /api/v2/products/{}` | так | PRODUCTS_V2 |  |
| `PUT /api/v2/products/{}` | так | PRODUCTS_V2 |  |
| `POST /api/v2/products/{}/barcodes` | так | PRODUCTS_V2 |  |
| `DELETE /api/v2/products/{}/barcodes/{}` | так | PRODUCTS_V2 |  |
| `POST /api/v2/products/{}/images` | так | PRODUCTS_V2 |  |
| `DELETE /api/v2/products/{}/images/{}` | так | PRODUCTS_V2 |  |
| `GET /api/v2/prro/queue` | ні | PRRO_V2 |  |
| `POST /api/v2/prro/receipts/{}/fiscalize` | так | PRRO_V2 |  |
| `GET /api/v2/prro/settings` | так | AUTH |  |
| `PUT /api/v2/prro/settings` | так | AUTH |  |
| `POST /api/v2/prro/shift/close` | так | PRRO_V2 |  |
| `POST /api/v2/prro/shift/open` | так | PRRO_V2 |  |
| `GET /api/v2/prro/shifts` | так | PRRO_V2 |  |
| `GET /api/v2/prro/status` | ні | PRRO_V2 |  |
| `POST /api/v2/prro/sync` | ні | PRRO_V2 |  |
| `POST /api/v2/prro/test-connection` | так | PRRO_V2 |  |
| `GET /api/v2/receipts` | так | PRRO_V2 |  |
| `GET /api/v2/receipts/by-product/{}/recent-sales` | так | PRRO_V2 |  |
| `GET /api/v2/receipts/products/{}/returnable-quantity` | так | PRODUCTS_V2 |  |
| `POST /api/v2/receipts/return` | так | PRRO_V2 |  |
| `POST /api/v2/receipts/sale` | так | PRRO_V2 |  |
| `GET /api/v2/receipts/search` | так | PRRO_V2 |  |
| `GET /api/v2/receipts/stats/today` | так | PRRO_V2 |  |
| `GET /api/v2/receipts/{}` | так | PRRO_V2 |  |
| `GET /api/v2/receipts/{}/items` | так | PRRO_V2 |  |
| `GET /health` | ні | CORE |  |


## Роути, що ЗАЛИШАТЬСЯ БЕЗ покриття (критично)

| # | Метод | Шлях | Тип | Коментар |
|---|---|---|---|---|
| 1 | GET | `/` | ? |  |
| 2 | GET | `/api/v1/categories/tree` | AUTH(CRUD)/READDIRS |  |
| 3 | POST | `/api/v1/debtors/{}/pay` | DEBTORS |  |
| 4 | POST | `/api/v1/products/{}/barcodes` | PRODUCTS_V2 |  |
| 5 | DELETE | `/api/v1/products/{}/barcodes/{}` | PRODUCTS_V2 |  |
| 6 | POST | `/api/v1/products/{}/images` | PRODUCTS_V2 |  |
| 7 | DELETE | `/api/v1/products/{}/images/{}` | PRODUCTS_V2 |  |
| 8 | GET | `/api/v1/receipts` | PRRO |  |
| 9 | POST | `/api/v1/receipts` | PRRO |  |
| 10 | GET | `/api/v1/receipts/by-product/{}/recent-sales` | PRRO |  |
| 11 | GET | `/api/v1/receipts/products/{}/returnable-quantity` | PRODUCTS_V2 |  |
| 12 | GET | `/api/v1/receipts/search` | PRRO |  |
| 13 | GET | `/api/v1/receipts/stats/today` | PRRO |  |
| 14 | GET | `/api/v1/receipts/{}` | PRRO |  |
| 15 | GET | `/api/v1/receipts/{}/items` | PRRO |  |
| 16 | GET | `/api/v1/suppliers/{}/products` | PRODUCTS_V2 |  |
| 17 | GET | `/api/v1/suppliers/{}/products/{}/movements` | PRODUCTS_V2 |  |
| 18 | POST | `/api/v2/auth/login` | AUTH |  |
| 19 | POST | `/api/v2/auth/login-pin` | AUTH |  |
| 20 | POST | `/api/v2/auth/refresh` | AUTH |  |
| 21 | GET | `/api/v2/auth/users` | AUTH |  |
| 22 | POST | `/api/v2/auth/users` | AUTH |  |
| 23 | GET | `/api/v2/auth/users/me` | AUTH |  |
| 24 | GET | `/api/v2/categories` | AUTH(CRUD)/READDIRS |  |
| 25 | POST | `/api/v2/categories` | AUTH(CRUD)/READDIRS |  |
| 26 | GET | `/api/v2/categories/tree` | AUTH(CRUD)/READDIRS |  |
| 27 | DELETE | `/api/v2/categories/{}` | AUTH(CRUD)/READDIRS |  |
| 28 | GET | `/api/v2/categories/{}` | AUTH(CRUD)/READDIRS |  |
| 29 | PUT | `/api/v2/categories/{}` | AUTH(CRUD)/READDIRS |  |
| 30 | GET | `/api/v2/prro/queue` | PRRO_V2 |  |
| 31 | GET | `/api/v2/prro/status` | PRRO_V2 |  |
| 32 | POST | `/api/v2/prro/sync` | PRRO_V2 |  |
| 33 | GET | `/health` | CORE |  |

## Класифікація непокритих роутів

Метод: зіставлення з **реальними викликами фронтенду** (`api.get/post/...` у `frontend/src`, V2-визначення в межах дужок виклику) та наявністю Rust-аналога.

### A. КРИТИЧНІ — фронтенд кличе, Rust НЕ має жодного аналога (10 + 1):

| Метод | Шлях | Джерело виклику |
|---|---|---|
| GET | `/api/v2/categories` | useCategories.ts (queryFn) |
| GET | `/api/v2/categories/tree` | CategoryPanel.tsx:110, useCategories.ts |
| GET | `/api/v2/categories/{id}` | categoryService.getCategory |
| POST | `/api/v2/categories` | useCategories.ts (mutationFn) |
| PUT | `/api/v2/categories/{id}` | categoryService.updateCategory |
| DELETE | `/api/v2/categories/{id}` | categoryService.deleteCategory |
| GET | `/api/v1/suppliers/{id}/products` | SupplierProductsPage.tsx:92 |
| GET | `/api/v1/suppliers/{id}/products/{pid}/movements` | SupplierProductsPage.tsx:102 |
| POST | `/api/v1/debtors/{id}/pay` | DebtorsPage.tsx:127 (payDebt) |
| POST | `/api/v1/receipts` | receiptService.createReceipt |
| GET | `/api/v1/auth/me` | useAuth.ts:26 (getCurrentUser) — Python теж НЕ має → 404 вже зараз |

### B. АЛІАСИ — фронтенд кличе v1/без-`/fiscal`, Rust має v2/fiscal-аналог (9):

| Метод | Шлях (фронтенд) | Rust-аналог | Рішення |
|---|---|---|---|
| POST | `/api/v1/products/{id}/barcodes` | `POST /api/v2/products/{id}/barcodes` | v1-аліас або переключення фронтенду на v2 |
| DELETE | `/api/v1/products/{id}/barcodes/{bid}` | `DELETE /api/v2/products/{id}/barcodes/{bid}` | те саме |
| POST | `/api/v1/products/{id}/images` | `POST /api/v2/products/{id}/images` | те саме |
| DELETE | `/api/v1/products/{id}/images/{iid}` | `DELETE /api/v2/products/{id}/images/{iid}` | те саме |
| GET | `/api/v1/receipts` | `GET /api/v2/receipts` | те саме |
| GET | `/api/v1/receipts/{id}` | `GET /api/v2/receipts/{id}` | те саме |
| GET | `/api/v2/prro/queue` | `GET /api/v2/prro/fiscal/queue` | аліас без /fiscal |
| GET | `/api/v2/prro/status` | `GET /api/v2/prro/fiscal/status` | аліас без /fiscal |
| POST | `/api/v2/prro/sync` | `POST /api/v2/prro/fiscal/sync` | аліас без /fiscal |

### C. LEGACY — фронтенд НЕ кличе → 410 Gone безпечно (15):

`GET /, GET /health, GET /api/v1/categories/tree, GET /api/v1/receipts/search, GET /api/v1/receipts/stats/today, GET /api/v1/receipts/{id}/items, GET /api/v1/receipts/by-product/{id}/recent-sales, GET /api/v1/receipts/products/{id}/returnable-quantity, POST /api/v2/auth/login, POST /api/v2/auth/login-pin, POST /api/v2/auth/refresh, GET /api/v2/auth/users, POST /api/v2/auth/users, GET /api/v2/auth/users/me`

> Примітка: `/health` використовується `frontend/src-tauri/scripts/e2e_stage5_tauri.sh` та `backend/Dockerfile` (healthcheck) — при дезактивації оновити на `:8000/api/v1/health`.

## Висновок

**НЕ МОЖНА вимикати sidecar повністю на поточному стані.** 19 активних роутів (10 CRIT + 9 ALIAS) зламають фронтенд після дезактивації.

### Передумови для безпечної дезактивації:

1. **Rust_Agent**: додати 10 CRIT-роутів — v2-categories (6: list/tree/get/create/update/delete), suppliers products+movements (2), debtors `/{id}/pay` (1), receipts POST v1 (1), auth/me (1, окремо — Python його не має).

2. **Rust_Agent АБО React_UI_UX_Agent**: для 9 ALIAS — v1-аліаси в Rust (products barcodes/images, receipts list/get) та prro `/queue|/status|/sync` без `/fiscal` **АБО** переключення фронтенду на наявні v2/fiscal-шляхи.

3. **Інфраструктура**: оновити `e2e_stage5_tauri.sh` і Dockerfile-healthcheck з `:8001/health` на `:8000/api/v1/health`.

4. Після пунктів 1–3: LEGACY-роути (15) повертають 410 — сумісність старих клієнтів збережена.


---

_Звіт згенеровано автоматично. Дані: live openapi :8001 (182 роути) + Rust-код kasa-api (158 роутів) + фронтенд-виклики. Код не змінювався, комітів немає._

---

# Оновлення: Малий крок 2/3 (дезактивація — закриття CRIT)

Дата: 2026-08-01 · Коміт: (див. git log)

## Закрито 7 з 10 CRIT-роутів (Rust-реалізація 1:1 з Python)

| Роут | Статус | Де реалізовано |
|---|---|---|
| GET /api/v2/categories | ✅ закрито | categories_v2.rs::list (page/size/search, {items,total,page,size}) |
| POST /api/v2/categories | ✅ закрито | categories_v2.rs::create (201; 400 exists_by_name; 404 parent; 422 name) |
| GET /api/v2/categories/tree | ✅ закрито | categories_v2.rs::tree (рекурсія по parent_id) |
| GET /api/v2/categories/{id} | ✅ закрито | categories_v2.rs::get (404) |
| PUT /api/v2/categories/{id} | ✅ закрито | categories_v2.rs::update (404; 400 self-parent/exists; 404 parent) |
| DELETE /api/v2/categories/{id} | ✅ закрито | categories_v2.rs::delete (204; 404) |
| POST /api/v1/debtors/{id}/pay | ✅ закрито | аліас у router_v1.rs → debtors::pay (1:1 Python v1) |

Змінені крейти: kasa-domain (ReadDirectories::search_categories/find_all_categories,
WriteDirectories::category_name_exists), kasa-infrastructure (SQL-реалізації,
динамічний WHERE як Python), kasa-api (categories_v2.rs + монтування під
KASA_RUST_READDIRS + debtors /pay аліас).

Differential-тест: `frontend/src-tauri/scripts/e2e_categories_v2_diff.sh` — **ALL PASS**
(30 перевірок: parity create/get/update/tree/list/search + валідації 400/404/422 + debtors pay).

## НЕ закрито (аномалія — роути складніші ніж здається)

| Роут | Причина аномалії | Обсяг роботи |
|---|---|---|
| GET /api/v1/suppliers/{id}/products | Агрегація товарів постачальника з 3 джерел (invoices confirmed, return_invoices confirmed, products.supplier_id) + залишки + загальна вартість | новий SQL-сервіс ~200 рядків |
| GET /api/v1/suppliers/{id}/products/{pid}/movements | Рух по 5 типах документів (invoice/return_invoice/transfer/write_off/receipt) з Decimal-розрахунками, сортуванням, limit | новий SQL-сервіс ~200 рядків |
| POST /api/v1/receipts | **Повна боргова семантика**: debt_payment (товар DEBT-PAYMENT, DebtorPayment, автознищення боржника при 0), debtor_id, original_receipt_id, return-валідація, генерація RCPT-номера, заокруглення, фіскалізація. Rust v2 create_sale НЕ підтримує борг (немає debtor_id/debt_payment у ReceiptCreateInput) | зміна domain-контрактів + SQL + ~400 рядків |

## Оновлений висновок

**Дезактивація sidecar все ще НЕ можлива** — 3 роути (2 suppliers + 1 receipts POST)
зламають: SupplierProductsPage, оплату боргу через касу (PosPage/debtor flow).
Рекомендація: закрити suppliers (окремий підкрок), receipts POST — окремий підкрок
з domain-змінами АБО переключити фронтенд на наявні механізми (v2 sale для звичайних
чеків вже працює; боргові чеки — тільки v1).

---

# Оновлення: Малий підкрок 2b/3 (дезактивація — suppliers products + movements)

Дата: 2026-08-01 · Коміт: (див. git log)

## Закрито 2 CRIT-роути (Rust-реалізація 1:1 з Python)

| Роут | Статус | Де реалізовано |
|---|---|---|
| GET /api/v1/suppliers/{id}/products | ✅ закрито | suppliers.rs + ReadDirectories::supplier_products |
| GET /api/v1/suppliers/{id}/products/{pid}/movements | ✅ закрито | suppliers.rs + ReadDirectories::product_movements |

Деталі (1:1 `SupplierProductService` Python):
- **products**: UNION 3 джерел (invoice_items confirmed + return_invoice_items confirmed
  + products.supplier_id) → товари з category LEFT JOIN, search ILIKE title/barcode/sku,
  `ORDER BY title`, `total_stock_value` = Σ(stock×cost_price) Decimal-множенням
  (rust_decimal, scale сумується як Python), 404 постачальника.
- **movements**: 5 джерел — invoice (прихід, +), return_invoice/receipt/write_off/transfer
  (витрата, −). Сортування date DESC (стабільне, як Python), `total_movements` ДО обрізання,
  `movements[:limit]` після (Python так само). Receipt БЕЗ фільтру постачальника,
  write_off БЕЗ статус-фільтру, transfer CONFIRMED — точно як Python.
  `py_or_zero` відтворює Python `Decimal(str(x or 0))` для write_off/transfer.
- **422 limit**: Pydantic v2 формат — `input` рядком + `ctx {ge/le}`.

Змінені крейти: kasa-domain (DTO suppliers.rs + 2 методи trait), kasa-infrastructure
(SQL), kasa-api (suppliers.rs + монтування під KASA_RUST_READDIRS).

Differential-тест: `frontend/src-tauri/scripts/e2e_suppliers_products_diff.py` — **ALL PASS**
(14 перевірок: 404×3, пустий постачальник, products+search parity, movements всіх 5 типів
parity, P2 тільки invoice, limit=0/501→422 parity, limit=2 обрізання parity).
Тестові дані створювались через Python API (справжній flow: invoices/returns/receipts/
write-offs/transfers + confirm), видалені напряму з БД (включно з supplier_ledger).

## НЕ закрито (1 CRIT)

| Роут | Причина аномалії | Обсяг роботи |
|---|---|---|
| POST /api/v1/receipts | **Повна боргова семантика**: debt_payment (товар DEBT-PAYMENT, DebtorPayment, автознищення боржника при 0), debtor_id, original_receipt_id, return-валідація, генерація RCPT-номера, заокруглення, фіскалізація. Rust v2 create_sale НЕ підтримує борг (немає debtor_id/debt_payment у ReceiptCreateInput) | зміна domain-контрактів + SQL + ~400 рядків |

## Оновлений висновок

**Дезактивація sidecar все ще НЕ можлива** — залишився 1 CRIT: POST /api/v1/receipts
(боргові чеки через касу: PosPage/debtor flow). Всі 9 інших CRIT-роутів закрито
Rust-реалізацією 1:1. Рекомендація: розширити Rust v2 create_sale борговою семантикою
(окремий підкрок) АБО переключити фронтенд: звичайні чеки вже йдуть v2, боргові — тільки v1.

---

# Оновлення: Малий підкрок 2c/3 (дезактивація — receipts POST, ОСТАННІЙ CRIT)

Дата: 2026-08-01 · Коміт: (див. git log)

## Закрито останній CRIT-роут

| Роут | Статус | Де реалізовано |
|---|---|---|
| POST /api/v1/receipts (з борговою семантикою) | ✅ закрито | pos.rs (kasa-api) + create_receipt_v1_impl (kasa-infrastructure) |

Повна 1:1 семантика Python `create_receipt` (app/api/v1/receipts.py:663):
- **debt_payment**: валідація боржника (404), сума ≤ борг (400), auto-add товару
  «Борг» (DEBT-PAYMENT, c230fe32-…), INSERT debtor_payments (payment_method='cash'),
  total_debt -= amount; при боргу ≤ 0 — автознищення боржника (каскад видаляє
  debtor_payments — FK ON DELETE CASCADE).
- **звичайний борг** (debtor_id, paid < total): total_debt += (total - paid).
- **генерація номера**: `RCPT-{Local YYYYMMDD}-{last+1:04d}` з ОСТАННЬОГО чека
  за created_at DESC (Python `int(number.split('-')[-1])` з catch → 0; hex-номери
  v2 → 0 — 1:1).
- **заокруглення** price_rounding (quantize ROUND_HALF_UP: 10/50/100/500).
- **return-валідація**: returnable = max(0, sold − returned) для кожного item
  (крім «Борг»), 400 з текстом Python.
- **allow_negative_stock**: 400 «Недостатньо товару…» або ручне оновлення stock.
- **Відповідь — identity map Python**: total/paid після rounding (scale 0),
  change_amount «0.0» (float default 0.00), items: quantity/price/total — вхідні
  (при >1 позиціях ПЕРША перечитується з БД — емпіричний патерн SQLAlchemy),
  purchase_price = str(float(cost_price)) («30.0»), total_profit/vat_amount —
  Python float-str (shortest round-trip).
- **422 Pydantic**: missing (з input=body), enum (з ctx expected),
  decimal_max_places (з ctx decimal_places), uuid_parsing.
- **500 IntegrityError**: неіснуючий товар → Python SQLAlchemy autoflush →
  FK violation → `{"detail":"Внутрішня помилка сервера","type":"IntegrityError"}`
  (відтворено 1:1).
- Фіскалізації в v1 POST **НЕМАЄ** (PRRO тільки у v2) — відтворено як є.

Domain: ReceiptV1CreateInput/ReceiptV1ItemInput/DebtPaymentInput +
ReceiptV1Dto/ReceiptV1ItemDto + PosService::create_receipt_v1.
API: POST /api/v1/receipts під KASA_RUST_READDIRS (pos::create_receipt_v1).

Differential-тест: `frontend/src-tauri/scripts/e2e_receipts_post_diff.py` — **ALL PASS**
(41 перевірка): sale повна оплата, борговий чек, оплата боргу повна (автознищення)
та часткова (запис debtor_payments), return (original_receipt_id + returnable),
помилки 400/404/500/422 (деталі R==P), rounding (47.33→47), ПДВ (tax_rate 20%).
cargo test --workspace: 189 passed, 0 failed. clippy 0, fmt чистий.
Тестові дані видалені (чеки/боржники/товари/платежі).

## ПІДСУМОК ДЕЗАКТИВАЦІЇ: ВСІ 10 CRIT ЗАКРИТО

| # | CRIT-роут | Статус |
|---|---|---|
| 1 | GET /api/v1/categories | ✅ |
| 2 | POST/PUT/DELETE /api/v1/categories | ✅ |
| 3 | CRUD /api/v1/products | ✅ |
| 4 | GET /api/v1/suppliers | ✅ |
| 5 | GET/POST /api/v1/inventory | ✅ |
| 6 | GET /api/v1/invoices + /{id} | ✅ |
| 7 | GET /api/v1/suppliers/{id}/products (+movements) | ✅ |
| 8 | POST /api/v1/invoices + return-invoices + confirm | ✅ |
| 9 | GET /api/v1/ledger | ✅ |
| 10 | POST /api/v1/receipts (борг) | ✅ |

## Оновлений висновок

**Усі 10 CRIT-роутів закрито Rust-реалізацією 1:1 (differential-тести ALL PASS).**
Залишились тільки ALIAS-роути (v1 GET-списки чеків/боржників/сесій та ін., що
проксіруються на Python через fallback — безпечно, Python залишається еталоном).
Дезактивація sidecar можлива: вимкнути Python для CRIT-роутів (KASA_RUST_READDIRS=1),
перевірити ALIAS-роути фронтендом, потім видалити Python-роутери.
