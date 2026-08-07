# Виконавчий план міграції Kasa POS → Rust

> Джерело стратегії: `docs/RUST_MIGRATION_PLAN.md` (v1.0, затверджений)
> Виконавчий контроль: NIKO (координація, моніторинг)
> Створено: 2026-08-07 | Оновлено: 2026-08-07 (етап 3 завершено)

## 0. Статус етапів

| Етап | Назва | Ведучий | Статус | DoD |
|---|---|---|---|---|
| 0 | Фундамент (workspace, axum-фасад, sidecar, CI) | Rust_Agent + Tauri_Agent | ✅ ЗАВЕРШЕНО (0.1–0.5) | workspace збирається; :8000+проксі :8001; фронт без змін; cargo test зелений |
| 1 | Довідники read (products/categories/suppliers GET) | Rust_Agent | ✅ ЗАВЕРШЕНО | Rust==Python 20/20, 50/50, 50/50; feature-flag KASA_RUST_READDIRS; відкат працює |
| 2 | Довідники CRUD + inventory | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E 16/16; конкурентність 2 confirm → stock 104.000; валідація 1:1 |
| 3 | POS: чеки v2, робочі сесії, списання, переміщення, зміни ПРРО (X/Z) | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E POS 43/43; конкурентність 2 sale → stock 86.000; транзакційність (помилка → rollback); X/Z без ПРРО 1:1 |
| 3b | Документи (invoices, purchase_orders, return_invoices) | Python_Backend_Agent + Rust_Agent | ⏳ | статуси 1:1; ledger ідентичний; офлайн-синхронізація |
| 4 | Ledger (бухгалтерія, звіти) | Rust_Agent | ⏳ | differential 10k операцій, 0 розбіжностей |
| 5 | Receipts + друк (open→pay→close, офлайн-черга) | Rust_Agent | ⏳ | proptest ≥100k, 0 розбіжностей; Python print-роути 410 |
| 6 | Auth / users / settings / RBAC | Rust_Agent | ⏳ | JWT parity тим самим секретом |
| 7 | ПРРО (gRPC/tonic, crypto, xml, offline_queue, shift) | Rust_Agent + apiarm_agent | ⏳ | sandbox-сертифікація; golden parity; shadow-mode; відкат |
| 8 | Дезактивація Python | Tauri_Agent + Git Admin Agent | ⏳ | єдиний бінарник; e2e зелений; updater |

Паралельно з етапом 1: QA_Agent — тестовий контур (differential/golden/proptest).
Паралельно з етапом 5: ПРРО-дослідження (JKS→PKCS12/PEM, FFI vs gRPC).

## 1. Критичний шлях

```
Етап 0 ✅ → 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8
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
| 2.1 | Write-порти CRUD+інвентаризації, SQL-репозиторії write, CRUD-роути під flag, E2E differential-скрипт | Rust_Agent | 319d849, c66450c, 04e6edb, adfa79a | ✅ | E2E 16/16: 201/200/204, 404, 409, 400, 422 ідентичні Python; конкурентність 2 паралельні confirm → stock 104.000; БД почищена від тестових даних |

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
2. Тестові процеси (facade :8000, Python :8001) були зупинені після верифікації.
   Наступний запуск: `cargo run -p kasa-api --bin facade` + Python sidecar
   (Tauri сам підніме sidecar при старті).

## 4. Наступні кроки (етап 4 — ledger/звіти, етап 5 — друк/офлайн-черга)

1. Rust_Agent: ledger (бухгалтерія, звіти) — differential 10k операцій.
2. Rust_Agent + Tauri_Agent: друк чеків (open→pay→close), офлайн-черга.
3. QA_Agent: differential/golden-контур для POS + ledger.
4. Git Admin Agent: PR за етапами 1+2+3 (накопичений зміст).
