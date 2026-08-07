# План міграції Kasa POS з Python на Rust

> Версія: 1.0 | Дата: 2026-08-07
> Джерела: консультації Rust_Agent, System_Architect_Agent, DB_Admin_Agent, QA_Agent + інвентаризація коду
> Статус: затверджений до виконання (етап 0)

---

## 0. Фактична база (з коду)

| Показник | Значення |
|---|---|
| Python бекенд `backend/app/` | ~41 404 LOC |
| Весь backend (з тестами/alembic) | ~66 301 LOC |
| Alembic міграцій | 51 |
| SQLAlchemy моделей | 21 (`persistence/models/`) |
| Rust-ядро Tauri | 3 916 LOC (`frontend/src-tauri/src/`) |
| Tauri | v2 (tauri = "2", tauri-build = "2") |
| Фронтенд | React + TypeScript + Vite |
| API v1 | 20 роут-модулів |
| API v2 | 7 роут-модулів (вкл. prro) |
| ПРРО-модуль | 4 033 LOC (crypto_signer 710, xml_builder 906, grpc_client 393, iit_sdk 475, key_store 256, offline_queue 211, prro.proto 156) |
| Python тестів | 72 (`tests/unit/{events,repositories,use_cases}` + `backend/tests/`) |
| ПРРО тестові ключі | Є: `certs/prro-test/*.jks` |
| Інтеграція Arm20 | ВІДСУТНЯ (grep по всьому проєкту — 0 збігів) |

---

## 1. Архітектурне рішення

### 1.1 Вибір: **Unified Tauri Application + вбудований axum-фасад (Strangler Fig)**

| Критерій | Unified Tauri | Standalone axum | Вбудований axum у Tauri |
|---|---|---|---|
| Hardware (COM/TCP/принтери) | ✅ один власник | ❌ конкуренція процесів | ✅ один процес |
| Офлайн-режим | ✅ природний | ⚠️ додаткова точка відмови | ✅ |
| Тестування логіки | ⚠️ | ✅ | ✅ через крейти |
| Міграція фронтенду | ❌ перепис invoke | ✅ нульова | ✅ нульова (axios → :8000) |
| Майбутній веб-клієнт | ❌ | ✅ | ✅ фасад виноситься |

**Рішення:** бізнес-логіка — в чистих крейтах (`crates/domain`, `crates/application`), які НЕ залежать від Tauri. Tauri-шар — тонкий адаптер команд. Вбудований axum-фасад біндиться на `127.0.0.1:8000` (той самий порт Python), Python sidecar переїжджає на `:8001`. Фронтенд (axios → `http://localhost:8000/api/v1`) **не змінюється взагалі**.

### 1.2 Гібридний режим

```
┌────────────────────────── Tauri Process ──────────────────────────┐
│ React frontend (axios, БЕЗ ЗМІН)                                  │
│   │  http://127.0.0.1:8000/api/v1/*                               │
│   ▼                                                                │
│ ┌───────────────────────────────────────────────┐                 │
│ │ crates/api — axum facade (embedded, tokio)    │                 │
│ │  route → native Rust handler                  │                 │
│ │  route → REVERSE PROXY → Python :8001         │                 │
│ │  auth: JWT (jsonwebtoken, спільний секрет)    │                 │
│ └───────────────────────────────────────────────┘                 │
│   │ native                    │ proxy                             │
│   ▼                           ▼                                   │
│ crates/application      reqwest → 127.0.0.1:8001 (FastAPI sidecar)│
│ crates/infrastructure                                            │
│   ├─ sqlx → PostgreSQL ◄─────┴── SQLAlchemy → PostgreSQL          │
│   ├─ rusqlite (offline)                                           │
│   └─ hardware (print/devices)                                     │
└───────────────────────────────────────────────────────────────────┘
```

Ключові правила:
- **Маршрутизація per-роутна, статична, в коді** (enum/match), не runtime-конфіг.
- **Схема БД належить ОДНІЙ міграційній системі.** Весь період міграції — Alembic (Python) володіє всіма таблицями; Rust (sqlx) тільки читає/пише без своїх міграцій. Право володіння таблицею передається Rust **тільки після повної міграції модуля**.
- Python — sidecar-процес: Tauri піднімає при старті, вбиває при виході (спершу flush черг).
- Fallback: недоступний Python при проксі-роуті → 503 з чітким повідомленням, не тихий фейл.

---

## 2. Цільова структура Rust workspace

```
kasa-rs/
├── Cargo.toml                 # [workspace]
├── crates/
│   ├── kasa-domain/           # ЧИСТИЙ шар: serde, chrono, rust_decimal
│   │   ├── money.rs           #   Decimal + валідація ≤2 знаки
│   │   ├── barcode.rs, quantity.rs, tax_rate.rs, rounding.rs
│   │   ├── invoice.rs, receipt.rs, product.rs, supplier.rs,
│   │   │   ledger_entry.rs, category.rs, user.rs
│   │   ├── aggregates/        #   Receipt (open→pay→close), InventoryCount
│   │   ├── events/            #   domain events (ReceiptFiscalized, StockChanged)
│   │   ├── repos/             #   trait ProductRepository, ReceiptRepository...
│   │   └── errors.rs          #   thiserror
│   ├── kasa-application/      # залежить тільки від domain
│   │   ├── ports/             #   ReceiptRepository, Fiscalizer, Printer...
│   │   ├── use_cases/         #   receipt, ledger, auth, invoice, product,
│   │   │                      #   invoice_print, prro/{fiscalize, shift, sync_offline_queue}
│   │   └── dto.rs             #   відповідає Python schemas/ (serde camelCase)
│   ├── kasa-infrastructure/   # реалізації портів
│   │   ├── postgres/          #   sqlx репозиторії (← SQLAlchemy models)
│   │   ├── prro/              #   tonic gRPC (prost з prro.proto!), crypto_signer,
│   │   │                      #   xml_builder, offline_queue, key_store, qr_url
│   │   ├── print/             #   ← print.rs (774) + commands/print.rs (428)
│   │   ├── devices/           #   ← devices.rs (915): serialport, TCP
│   │   ├── terminal/          #   ← pb_protocol.rs (689): Newland N950
│   │   ├── cash_drawer/       #   ← cash_drawer.rs (140)
│   │   └── offline/           #   ← db.rs + offline.rs (SQLite, rusqlite)
│   ├── kasa-api/              # axum facade (embedded HTTP gateway)
│   │   ├── router_v1.rs, router_v2.rs   # маршрути 1:1 з Python
│   │   ├── proxy.rs           #   reverse proxy → Python :8001 (reqwest)
│   │   └── auth.rs            #   JWT validation (jsonwebtoken)
│   └── kasa-tauri-shell/      # тонкий шар: Tauri commands + AppState + events
│       ├── commands/          #   print, devices, offline, system (обгортки)
│       └── lib.rs             #   spawn axum facade, sidecar-менеджмент
└── tests/                     # e2e: обидва стекі + PostgreSQL
```

**Правило:** існуючі 3 916 LOC монолітних Tauri-команд **розщеплюються** (рефакторинг, не перепис): hardware → `infrastructure/*`, у tauri-shell лишаються тонкі команди.

---

## 3. Технічний стек (мапінг Python → Rust)

| Python | Rust | Примітка |
|---|---|---|
| FastAPI/Uvicorn | axum 0.7 (tokio) | тільки для вбудованого фасаду |
| SQLAlchemy | sqlx 0.8 | compile-time SQL, async |
| Alembic | sqlx-cli (SQL-міграції) | baseline з pg_dump, НЕ порт 51 файлу |
| Pydantic | serde + serde_json | `rename_all = "camelCase"` |
| Decimal | rust_decimal | JSON — рядком `"12.34"`, ніколи f64 |
| datetime tz | chrono `DateTime<Utc>` | `Europe/Kyiv` тільки на межі (chrono-tz) |
| Enum(VARCHAR+CHECK) | String на шарі БД + enum у домені | `#[sqlx(type_name)]` при потребі |
| PyJWT | jsonwebtoken | той самий секрет |
| bcrypt | argon2 / bcrypt крейт | зберегти сумісність хешів |
| gRPC (prro_pb2) | tonic + prost | з того ж prro.proto |
| Jinja2 (шаблони друку) | minijinja | 58/76/91мм шаблони |
| escpos-друк | escpos крейти / власний | golden на байти ESC/POS |
| serialport (ваги/принтери) | serialport | вже є в Cargo.toml |
| OCR (Gemini) | НЕ портувати | лишити Python-мікросервіс або зовнішній API |

---

## 4. База даних (рішення DB_Admin_Agent)

1. **sqlx 0.8** (не Diesel): async-нативний, сирий SQL, Postgres+SQLite одним драйвером, compile-time перевірка, `rust_decimal` фіча.
2. **Одна baseline-міграція** замість 51 файлу:
   ```
   docker compose up -d db
   pg_dump -h localhost -U kasa --schema-only --no-owner --no-privileges kasa > baseline.sql
   sqlx migrate add initial_schema   # копіювати baseline.sql, прибрати OWNER TO, alembic_version
   sqlx migrate add seed             # admin, ролі
   ```
   Для існуючих prod-БД: застосувати baseline напряму + ручний INSERT у `_sqlx_migrations` з SHA-384.
3. **Типи:** `Numeric(12,2)` → `rust_decimal::Decimal`; `Numeric(10,3)` → Decimal; `timestamptz` → `DateTime<Utc>`; JSONB → `serde_json::Value`/типізовані struct; UUID → `uuid::Uuid`.
4. **Енуми вже VARCHAR+CHECK** (create_constraint=True) → тривіальний мапінг, сумісність з SQLite.
5. **Гроші:** `rust_decimal` end-to-end; у JSON — рядком; округлення явно (`MidpointAwayFromZero` = Python HALF_UP).
6. **Ролі/права — НЕ в міграціях:** `docker-entrypoint-initdb.d/01_roles.sql` (kasa_owner, kasa_app, kasa_test CREATEDB) + `02_grants.sql` з `ALTER DEFAULT PRIVILEGES`.
7. **Офлайн:** SQLite через sqlx, гроші TEXT, outbox + client-UUID + idempotent UPSERT (`ON CONFLICT (id) DO NOTHING`). Читання: снапшоти з version/etag. Не синхронізувати похідні таблиці (ledger, balances).

---

## 5. Фази міграції (Strangler Fig)

Принцип сортування: **ризик (фіскальний/регуляторний) → ізольованість → read before write → hardware.**

### Етап 0. Фундамент (готовий на ~80%)
**Зміст:** рефакторинг наявних 3 916 LOC Rust у крейти workspace; axum-фасад на :8000; Python sidecar на :8001; JWT-валідація в Rust; CI-скелет (cargo fmt/clippy/test, sqlx prepare).
**DoD:**
- [ ] cargo workspace збирається; Tauri-команди працюють через крейти
- [ ] фасад :8000 приймає запити, проксі на Python :8001 працює
- [ ] фронтенд працює БЕЗ змін (axios → :8000)
- [ ] `cargo test` зелений; differential-міст (Rust CLI) зібраний
- **Тривалість:** 5–7 робочих днів

### Етап 1. Довідники read (нульовий фіскальний ризик)
**Зміст:** `GET` products, categories, suppliers у Rust + sqlx.
**DoD:**
- [ ] snapshot-тести: відповіді GET ідентичні Python
- [ ] feature-flag on; відкат перевірено
- **Тривалість:** 4–5 днів

### Етап 2. Довідники CRUD + inventory
**Зміст:** CRUD довідників + transfers, write_offs (бізнес-правила, без фіскалізації).
**DoD:**
- [ ] валідаційні помилки ідентичні Python
- [ ] stock-рухи збігаються; тест двох терміналів (конкурентність) зелений
- **Тривалість:** 6–8 днів

### Етап 3. Документи (не фіскальні)
**Зміст:** invoices (внутрішні), documents, purchase_orders, return_invoices (нефіскальна частина).
**DoD:**
- [ ] статусні переходи 1:1; ledger-постінги ідентичні
- [ ] офлайн-створення + синхронізація працює
- **Тривалість:** 7–10 днів

### Етап 4. Ledger (бухгалтерія)
**Зміст:** ledger_entries, supplier_ledger, debtor_balances, звіти.
**DoD:**
- [ ] постінги байт-ідентичні Python для 10k випадкових операцій (differential)
- [ ] звіти збігаються
- **Тривалість:** 6–8 днів

### Етап 5. Receipts (чеки) + друк
**Зміст:** життєвий цикл open→pay→close, оплати, знижки, повернення/обмін; друк чеків через Rust; офлайн-черга чеків — єдиний власник Rust.
**DoD:**
- [ ] parity життєвого циклу; proptest округлення (≥100k операцій, 0 розбіжностей)
- [ ] друк через Rust; Python print-роути 410
- [ ] офлайн-черга чеків належить Rust
- **Тривалість:** 10–14 днів

### Етап 6. Auth / users / settings
**Зміст:** видача JWT у Rust, RBAC, settings, users, work_sessions.
**DoD:**
- [ ] Rust валідує JWT тим самим секретом; RBAC parity
- **Тривалість:** 4–6 днів

### Етап 7. ПРРО — ОСТАННІМ (найвищий ризик)
**Зміст:** tonic gRPC-клієнт (prost з prro.proto), crypto_signer (ДСТУ), xml_builder, key_store, offline_queue, shift/Z-звіт.
**Критичні обмеження:**
- **НЕ чіпати сертифікований криптошар IIT SDK** — не переписувати ДСТУ 4145 на чистий Rust. Використовувати IIT SDK через FFI (bindgen) АБО через gRPC (рекомендовано — tonic поверх наявного prro.proto).
- Python-реалізацію заморозити як еталон (sandbox certs/prro-test, smoke_test.py).
**DoD:**
- [ ] sandbox-сертифікація: фіскалізація, зміни, Z-звіт parity
- [ ] golden-XML/підпис байт-ідентичний Python
- [ ] gRPC до IIT sandbox збігається; replay офлайн-фіскальної черги ідентичний
- [ ] shadow-режим: N реальних чеків (Rust рахує, Python виконує)
- [ ] відкат без втрати фіскального стану
- **Тривалість:** 20–30 днів

### Етап 8. Дезактивація Python
**Зміст:** Python sidecar видалено; Alembic передав усі таблиці sqlx; старі роути 410.
**DoD:**
- [ ] єдиний Tauri-бінарник; e2e зелений; tauri-updater працює
- **Тривалість:** 3–5 днів

### Критичний шлях
```
Етап 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8
Паралельно з Етапом 1: тестова інфраструктура (differential, golden, proptest)
Паралельно з Етапом 3: друк (підготовка шаблонів minijinja)
Паралельно з Етапом 5: ПРРО-дослідження (FFI vs gRPC, формат JKS-ключа)
```

---

## 6. Тестування (рішення QA_Agent)

**Принцип:** тести = контракт поведінки. Модуль мігрований, коли Rust проходить ті самі контракти, що й Python. 0 розбіжностей differential — gate міграції.

1. **Тест-вектори** — спільний формат `tests/vectors/<module>/<case>.json`; Python: pytest+parametrize, Rust: rstest.
2. **Golden files** — Python: syrupy, Rust: insta. Канонізувати вивід (дати, ID, PDF).
3. **Property-based** — Python: hypothesis, Rust: proptest. Однакові інваріанти (round_trip, суми, ідемпотентність).
4. **Differential testing** — Python-driven: hypothesis генерує → Python рахує A → Rust CLI рахує B → normalize(A)==normalize(B). CLI (не pyo3): нуль залежностей. Розбіжності → shrink → регресійний вектор.
5. **Округлення:** Python `ROUND_HALF_EVEN` vs `ROUND_HALF_UP` — зафіксувати режими у векторах ПЕРШИМ. `#[deny(clippy::float_arithmetic)]` у фінансових модулях; заборона f64.
6. **ПРРО:** port-adapter `trait FiscalProvider` + `FakeFiscalProvider`; contract tests (вектори request→signature→response); wiremock для HTTP; тестові ключі з `certs/prro-test/*.jks` (вирішити формат: JKS → PKCS12/PEM). Що НЕ мокається: реальний staging-стенд ПРРО поза CI.
7. **Tauri-команди:** функції `fn cmd(state: State<AppState>, args)` + mock State (`Box<dyn Trait>` + InMemoryDb/MockPrinter/FakeFiscalProvider); `tauri::test::mock_builder()`.
8. **Периферія:** golden на байти ESC/POS (hex); PDF — структура документа, не сирі байти; сокет-емуляція (TCP-сервер, що імітує принтер/термінал; Newland N950 — RST/SO_LINGER кейс).
9. **CI:**
   ```
   rust-core:   cargo fmt --check → clippy -D warnings → cargo test
   rust-tauri:  cargo test (tauri::test)
   python-etalon: pytest -m "not differential"
   differential: cargo build → pytest -m differential      ← gate міграції
   golden:      cargo insta test --check + pytest snapshot
   lints:       ruff+mypy; grep: жодного f64 у фінансах
   integration: contract ПРРО (wiremock), сокет-емуляції, staging (ручний)
   ```

---

## 7. Ризики та мітигація

| # | Ризик | Мітигація |
|---|-------|-----------|
| 1 | ПРРО: помилка порту крипто/XML/gRPC = регуляторне порушення | ПРРО останнім; golden-XML/підпис; sandbox; shadow-mode; feature-flag з миттєвим відкатом; НЕ переписувати ДСТУ на чистий Rust |
| 2 | Офлайн-черги: два власники → дублі/втрата чеків | Єдиний власник з етапу 5; client-UUID; idempotent sync; append-only |
| 3 | Консистентність даних (Python+Rust пишуть одні таблиці) | Один власник міграцій (Alembic) до хендоффу; атомарна маршрутизація per-агрегат |
| 4 | JSON-контракти (snake_case vs camelCase) | serde camelCase; snapshot-тести контрактів |
| 5 | Грошове округлення (HALF_UP vs HALF_EVEN, rounding allocation) | Money value-object; proptest 100k+; differential як gate |
| 6 | Lifecycle: axum в Tauri, shutdown (flush черг) | `tauri::async_runtime::spawn`; shutdown-hook: stop → flush → kill sidecar |
| 7 | Hardware-конкуренція (Python vs Rust на принтери) | Після міграції print — весь друк через Rust; Python print-роути 410 |
| 8 | Обсяг: Rust у 3–5× багатослівніший | Пріоритет parity, не краса; оцінка: domain+application 15–25K LOC, інфраструктура 15–20K, ПРРО 5–8K |
| 9 | N+1 і продуктивність sqlx vs SQLAlchemy | Контрактні тести p95 < 2× Python; індексний аудит |
| 10 | JKS-ключ ПРРО у Rust | Вирішити формат (JKS→PKCS12/PEM) ДО етапу 7 |
| 11 | OCR (Gemini) | НЕ портувати — лишити Python-мікросервіс/зовнішній API |

---

## 8. Оцінка трудозатрат

| Етап | Тривалість (роб. днів) | LOC Rust (оцінка) |
|---|---|---|
| 0. Фундамент | 5–7 | 2 000–3 000 |
| 1. Довідники read | 4–5 | 2 000 |
| 2. Довідники CRUD + inventory | 6–8 | 3 000–4 000 |
| 3. Документи | 7–10 | 4 000–5 000 |
| 4. Ledger | 6–8 | 3 000–4 000 |
| 5. Receipts + друк | 10–14 | 5 000–7 000 |
| 6. Auth/settings | 4–6 | 2 000 |
| 7. ПРРО | 20–30 | 5 000–8 000 |
| 8. Дезактивація | 3–5 | 500 |
| **Разом** | **65–93 (≈3–4 місяці)** | **26 000–36 000** |

За умови 1 Rust-розробника (або 2 паралельно: domain/application і infrastructure). Python-розробник тримає еталонні тести та differential.

---

## 9. Розподіл роботи по агентах

| Етап | Ведучий агент | Підтримка |
|---|---|---|
| 0. Фундамент | Rust_Agent + Tauri_Agent | System_Architect_Agent |
| 1–2. Довідники | Rust_Agent | DB_Admin_Agent (sqlx), QA_Agent |
| 3–4. Документи/Ledger | Python_Backend_Agent (еталон) + Rust_Agent | QA_Agent (differential) |
| 5. Receipts + друк | Rust_Agent | QA_Agent (proptest), Desktop_UI_Agent |
| 6. Auth | Rust_Agent | QA_Agent |
| 7. ПРРО | Rust_Agent + apiarm_agent(консультація gRPC) | QA_Agent (contract tests), DB_Admin_Agent |
| 8. Дезактивація | Tauri_Agent + Git Admin Agent | PM_Agent (реліз) |

Постійно: PM_Agent — план/ризики, QA_Agent — тестовий контур, Git Admin Agent — гілки/PR.

---

## 10. Правила дисципліни (обов'язкові)

1. **Жодного big-bang.** Кожен етап — окремий PR, feature-flag, перевірений відкат.
2. **Схема належить одному.** Alembic володіє, поки модуль не мігрований повністю.
3. **Гроші — тільки Decimal**, у JSON — рядок. f64 у фінансах = CI fail.
4. **ПРРО — останнім**, з golden + shadow + sandbox. IIT SDK через gRPC/FFI, не перепис.
5. **Differential = gate.** Без 0 розбіжностей модуль не вважається мігрованим.
6. **Python-тести модуля видаляються разом з Python-кодом** після міграції (правило двох еталонів) — вектори стають Rust-тестами.
7. **OCR не чіпаємо.**
