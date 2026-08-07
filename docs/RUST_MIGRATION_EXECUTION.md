# Виконавчий план міграції Kasa POS → Rust

> Джерело стратегії: `docs/RUST_MIGRATION_PLAN.md` (v1.0, затверджений)
> Виконавчий контроль: NIKO (координація, моніторинг)
> Створено: 2026-08-07 | Оновлено: 2026-08-07 (етап 6 завершено)

## 0. Статус етапів

| Етап | Назва | Ведучий | Статус | DoD |
|---|---|---|---|---|
| 0 | Фундамент (workspace, axum-фасад, sidecar, CI) | Rust_Agent + Tauri_Agent | ✅ ЗАВЕРШЕНО (0.1–0.5) | workspace збирається; :8000+проксі :8001; фронт без змін; cargo test зелений |
| 1 | Довідники read (products/categories/suppliers GET) | Rust_Agent | ✅ ЗАВЕРШЕНО | Rust==Python 20/20, 50/50, 50/50; feature-flag KASA_RUST_READDIRS; відкат працює |
| 2 | Довідники CRUD + inventory | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E 16/16; конкурентність 2 confirm → stock 104.000; валідація 1:1 |
| 3 | POS: чеки v2, робочі сесії, списання, переміщення, зміни ПРРО (X/Z) | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E POS 43/43; конкурентність 2 sale → stock 86.000; транзакційність (помилка → rollback); X/Z без ПРРО 1:1 |
| 3b | Документи (invoices, purchase_orders, return_invoices) | Python_Backend_Agent + Rust_Agent | ⏳ | статуси 1:1; ledger ідентичний; офлайн-синхронізація |
| 4 | Ledger (журнал взаєморозрахунків) v1+v2 | Rust_Agent | ✅ ЗАВЕРШЕНО | differential 10 100 записів 1:1; 101 сторінка GET 1:1; валідації 404/400/422/500 1:1; конкурентність/транзакційність |
| 5 | Receipts + друк (open→pay→close, офлайн-черга) | Tauri_Agent + Rust_Agent | ✅ ЗАВЕРШЕНО | ESC/POS друк у мок-пристрій (19227 байт, ESC @ + GS v 0 + GS V); офлайн-черга SQLite на диск + персистентність + синхронізація; Python print-роути 410 |
| 6 | Auth / users / settings / RBAC | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E AUTH DIFF 59/59; JWT крос-валідний (Rust↔Python, той самий секрет); RBAC admin/cashier 1:1; feature-flag KASA_RUST_AUTH (відкат перевірено); валідації 401/400/403/404/409/422 1:1 |
| 7 | ПРРО (gRPC/tonic, crypto, xml, offline_queue, shift) | Rust_Agent + apiarm_agent | ⏳ | sandbox-сертифікація; golden parity; shadow-mode; відкат |
| 8 | Дезактивація Python | Tauri_Agent + Git Admin Agent | ⏳ | єдиний бінарник; e2e зелений; updater |

Паралельно з етапом 1: QA_Agent — тестовий контур (differential/golden/proptest).
Паралельно з етапом 5: ПРРО-дослідження (JKS→PKCS12/PEM, FFI vs gRPC).

## 1. Критичний шлях

```
Етап 0 ✅ → 1 ✅ → 2 ✅ → 3 ✅ → 4 ✅ → 5 ✅ → 6 ✅ → 7 → 8
```

## 2. Журнал делегувань (етап 0)

| # | Контракт | Агент | Коміт | Статус | Верифікація NIKO |
|---|---|---|---|---|---|
| 0.1 | Workspace + рефакторинг 3 916→455 LOC у крейти | Rust_Agent | 1a63dae | ✅ | build/test/clippy чисті; 28 тестів збережено |
| 0.2 | axum-фасад :8000 + проксі :8001 + JWT + diff CLI | Rust_Agent | 87cc1e1, b4778cd | ✅ | health 200; 401 без JWT; 503 при Python down; 200 при Python up |
| 0.3 | Python sidecar → :8001 + CORS | Tauri_Agent | 002e725 | ✅ | /health 200, /docs 200 на :8001 |
| 0.4 | CI rust-core.yml + push гілки | Git Admin Agent | 81e5b12 | ⚠️ ЧАСТКОВО | workflow валідний; гілка в origin; **PR не відкрито — gh не автентифікований** |
| 0.5 | Differential CLI | Rust_Agent | (в 0.2) | ✅ | echo-op працює: {"op":"echo","ok":true} |
| 1.1 | Репозиторії read довідників + роути GET + snapshot-тести | Rust_Agent | c71f97c, f0d6db8, 21d9467 | ✅ | Rust==Python 20/20, 50/50, 50/50; flag KASA_RUST_READDIRS=1; без флага проксі ідентичний; cargo test/clippy/fmt чисті |
| 4.1 | Ledger v1+v2: порти, SQL-репозиторії, роути під flag, E2E differential 10 100 записів | Rust_Agent | 08d0c34, 20bc65d, 24ec542, 0ed4d70, ee365c8, 9ef3931, c35a03e | ✅ | E2E ledger: 10 100 записів (10k Rust + 100 Python), v2 GET entries 101 сторінка 1:1, v1/v2 balance, balances, валідації 404/400/422/500 1:1; конкурентність 2 паралельні POST → 201/201 без втрат; транзакційність: 400 не створив запис; cargo test 58/58, clippy/fmt чисті; тестові дані E4-/E4T- видалені (перевірено psql)
| 5.1 | Друк чеків open→pay→close (ESC/POS → мок-пристрій) + офлайн-черга (SQLite на диск, персистентність, синхронізація) | Tauri_Agent | f009579 | ✅ | e2e_stage5_tauri.sh: друк 19227 байт ESC/POS (ESC @ / GS v 0 / 2×подача / GS V); офлайн: save→count=1→перезапуск процесу count=1 (персистентність)→sync POST sale 201→count=0→чек знайдено в backend; фінальний health 200; тестові дані видалені; fmt виправлено NIKO (494766a) |
| 6.1 | Auth/users/settings/RBAC: порти kasa-domain (AuthService, DTO, валідатор settings), SQL-репозиторій SqlxAuth (login/login-pin/refresh/logout, users CRUD, permissions, hourly-rate, settings), фасад kasa-application, роути kasa-api під KASA_RUST_AUTH, JWT create (HS256, той самий секрет), E2E differential | Rust_Agent | 000d238 | ✅ | E2E AUTH DIFF 59/59 зелений; JWT parity: токени Rust↔Python крос-валідні (verify/refresh обидва напрямки, claims 1:1: access {sub,role,permissions,type,iat,exp}, refresh без permissions); RBAC 401/403 1:1; валідації 401/400/403/404/409/422 1:1; feature-flag: KASA_RUST_AUTH=1 → Rust, =0 → проксі Python (перевірено); cargo test 69/69, clippy/fmt чисті; БД почищена |
| 3.1 | POS: чеки v2 (sale/return/list/detail/items/stats/search/by-product/returnable), робочі сесії, списання, переміщення, зміни ПРРО (X/Z) | Rust_Agent | 72b4e21, fcaeffa, ba695ec, 6e97a5c, 9b0bf39, 435ea36 | ✅ | E2E POS 43/43: чеки (sale/return/список/деталі/статистика/пошук/returnable), робочі сесії, списання (авто-confirm), переміщення (draft→confirm/cancel), ПРРО X/Z; транзакційність: 400 у середині → чек не створено, stock не змінено; конкурентність 2 паралельні sale → stock 86.000, нуль втрат; cargo test 9/9, clippy/fmt чисті |
| 2.1 | Write-порти CRUD+інвентаризації, SQL-репозиторії write, CRUD-роути під flag, E2E differential-скрипт | Rust_Agent | 319d849, c66450c, 04e6edb, adfa79a | ✅ | E2E 16/16: 201/200/204, 404, 409, 400, 422 ідентичні Python; конкурентність 2 паралельні confirm → stock 104.000; БД почищена від тестових даних |

**DoD етапу 6 (Auth/users/settings/RBAC):**
- [x] auth: POST /login (пароль), /login-pin (PIN, 401 без PIN-коду), /refresh (400 без
      токена, 401 невалідний, 401 неіснуючий/деактивований), /logout (закриття сесії,
      duration_hours), GET /verify (публічний, optional), GET /users-list (публічний)
- [x] users: list (page/size + 422 int/ge/le), get (404), create (201, авто-логін
      транслітерацією, 409 дублікат), update (exclude_unset, хешування пароля/PIN),
      permissions (400 невідоме право), hourly-rate (422 float/gt), delete (204/404/409),
      permissions/list (групи+іконки 1:1)
- [x] settings: GET всі (модулі), GET /{module} (404), PUT /{key} (upsert: module/value_type/
      label авто; валідації 422: int-діапазони, bool true/false/1/0, whitelist barcode_type),
      PUT batch (тільки існуючі ключі, ігнор невідомих; нормалізація значень), 403 для cashier
- [x] RBAC: 401 без токена ("Відсутній заголовок авторизації"/"Невірний формат токена..."),
      403 cashier (users/settings), деактивація → 403; ролі admin/cashier (v1 — manager немає)
- [x] JWT parity: той самий секрет (KASA_JWT_SECRET / backend/.env SECRET_KEY); claims 1:1
      (access: sub/role/permissions/type/iat/exp; refresh: без permissions); крос-валідація
      Rust↔Python: verify обидва напрямки valid=true, refresh обидва напрямки 200
- [x] feature-flag KASA_RUST_AUTH: =1 → Rust-гілка; =0 → проксі Python (відкат перевірено)
- [x] E2E differential-скрипт scripts/e2e_auth_diff.sh — 59/59 зелений
- [x] cargo test --workspace 69 passed, clippy 0 warnings, fmt чистий
- [x] БД почищена: тестові юзери видалені, settings відновлені (print_copies=1,
      auto_cut_paper=false, barcode_type=code128, upsert-ключі видалені)

**DoD етапу 4 (Ledger):**
- [x] ledger v1+v2 (7 ендпойнтів): POST /ledger, GET /{supplier_id}, GET /balance/{id} (v1);
      GET/POST /entries, GET /balance/{id}, GET /balances (v2) — 1:1 Python
- [x] differential 10 100 записів (10 000 через Rust v2 POST + 100 через Python v1 POST):
      GET v2 entries 101 сторінка × 100 — Rust==Python 1:1; v1 history, v1/v2 balance, v2 balances
- [x] валідації 1:1: 404 (v1×3, v2 balance), 400 (тип/supplier), 422 (decimal_max_places
      з ctx, missing з input=body, enum з ctx.expected), 500 ValueError (v2 entries)
- [x] конкурентність: 2 паралельні POST → 201/201, записів 2 (жоден не втрачено)
- [x] транзакційність: 400 (невалідний тип) не створює запис (count до/після рівні)
- [x] E2E differential-скрипт scripts/e2e_ledger_diff.sh — повністю зелений (25/25)
- [x] cargo test --workspace зелений (63 passed, 0 failed), clippy 0, fmt чистий
- [x] БД почищена: тестові E4-/LEDGER- дані count=0; реальні дані не чіпались

**DoD етапу 3 (POS):**
- [x] чеки v2 (sale/return/list/detail/items/stats/search/by-product/returnable) — 1:1 Python
- [x] робочі сесії (/my, /report, /user/{id}) — 1:1, 'Z'-формат часу
- [x] списання (CRUD+confirm, авто-confirm при create) — 1:1, вхідна scale create / scale БД GET
- [x] переміщення (CRUD+confirm/cancel, тільки чернетки редагуються) — 1:1
- [x] зміни ПРРО: list з БД; open/close без ПРРО → 400 з текстом Python
- [x] конкурентність: 2 паралельні sale → stock 86.000, нуль втрат (FOR UPDATE)
- [x] транзакційність: помилка на 2-й позиції sale → rollback (stock/чек/номер не спалено)
- [x] E2E differential-скрипт scripts/e2e_pos_diff.sh — повністю зелений
- [x] cargo test/clippy/fmt чисті; БД почищена від тестових даних (реальні 24.07 не чіпались)

**DoD етапу 2:**
- [x] write-репозиторії + транзакції для products/categories/suppliers/inventory
- [x] CRUD-роути POST/PUT/PATCH/DELETE під KASA_RUST_READDIRS (без флага — проксі)
- [x] валідація/статуси/помилки 1:1 з Python (201/200/204, 404, 409, 400, 422)
- [x] конкурентність: 2 паралельні confirm → stock 104.000, нуль втрат
- [x] E2E differential-скрипт scripts/e2e_crud_diff.sh — 16/16 пройдено
- [x] БД почищена від тестових даних, цілісність збережена

**DoD етапу 1:**
- [x] репозиторії sqlx read-only + сервіси + DTO для products/categories/suppliers
- [x] роути GET під feature-flag KASA_RUST_READDIRS (без флага — проксі на Python, відкат)
- [x] snapshot-тести: Rust-відповідь == Python-еталон (нормалізований JSON)
- [x] cargo build/clippy/test/fmt чисті

**DoD етапу 0:**
- [x] cargo workspace збирається; Tauri-команди працюють через крейти
- [x] фасад :8000 приймає запити, проксі на Python :8001 працює
- [x] фронтенд працює БЕЗ змін (axios → :8000, тепер це Rust-фасад)
- [x] `cargo test` зелений; differential-міст (Rust CLI) зібраний

## 3. Аномалії та відкриті питання

1. **PR не відкрито**: `gh` CLI не автентифікований на цій машині. Гілка
   `feat/rust-migration` запушена в origin (moroznastya/KASA, 6 комітів).
   → Потрібен ручний PR через GitHub web UI або `gh auth login`.
3. **Python-баг v2 POST /ledger/entries → 500 UnmappedInstanceError** (етап 4):
   Rust v2 POST створює запис (201), Python падає з 500. Rust==Python на GET-стороні
   (101 сторінка 1:1); аномалія зафіксована скриптом e2e_ledger_diff.sh як «аномалія Python».
   → Виправити у Python-бекенді або задокументувати як відомий баг при дезактивації (етап 8).
3. **Python-баг delete_user → 500 IntegrityError** (етап 6):
   Python `DELETE /users/{id}` падає з 500 IntegrityError на користувачах, які мають
   робочі сесії (login): SQLAlchemy relationship `user.work_sessions` без cascade +
   БД FK ON DELETE CASCADE + nullable=False → ORM намагається встановити user_id=NULL
   → порушення NOT NULL. Юзер без сесій видаляється (204). Rust `SqlxAuth::delete_user`
   робить правильно (204, CASCADE) — це відхилення від Python у КРАЩИЙ бік, зафіксовано.
   → Виправити у Python-бекенді при дезактивації (етап 8): додати cascade у relationship.
4. Тестові процеси (facade :8000, Python :8001) зупиняються після верифікації.
   Наступний запуск: `cargo run -p kasa-api --bin facade` + Python sidecar
   (Tauri сам підніме sidecar при старті).

## 4. Наступні кроки (етап 7 — ПРРО)

1. Rust_Agent: ПРРО gRPC/tonic (crypto, xml, offline_queue, shift) — sandbox-сертифікація,
   golden parity з Python, shadow-mode, відкат.
2. QA_Agent: розширити differential-контур на auth/users/settings (e2e_auth_diff.sh).
3. Git Admin Agent: PR за етапами 1–6 (накопичений зміст).
