# Виконавчий план міграції Kasa POS → Rust

> Джерело стратегії: `docs/RUST_MIGRATION_PLAN.md` (v1.0, затверджений)
> Виконавчий контроль: NIKO (координація, моніторинг)
> Створено: 2026-08-07

## 0. Статус етапів

| Етап | Назва | Ведучий | Статус | DoD |
|---|---|---|---|---|
| 0 | Фундамент (workspace, axum-фасад, sidecar, CI) | Rust_Agent + Tauri_Agent | 🔄 В РОБОТІ | workspace збирається; :8000+проксі на :8001; фронт без змін; cargo test зелений |
| 1 | Довідники read (products/categories/suppliers GET) | Rust_Agent | ⏳ | snapshot-тести ідентичні Python; feature-flag; відкат |
| 2 | Довідники CRUD + inventory | Rust_Agent | ⏳ | валідація ідентична; конкурентність двох терміналів |
| 3 | Документи (invoices, purchase_orders, return_invoices) | Python_Backend_Agent + Rust_Agent | ⏳ | статуси 1:1; ledger ідентичний; офлайн-синхронізація |
| 4 | Ledger (бухгалтерія, звіти) | Rust_Agent | ⏳ | differential 10k операцій, 0 розбіжностей |
| 5 | Receipts + друк (open→pay→close, офлайн-черга) | Rust_Agent | ⏳ | proptest ≥100k, 0 розбіжностей; Python print-роути 410 |
| 6 | Auth / users / settings / RBAC | Rust_Agent | ⏳ | JWT parity тим самим секретом |
| 7 | ПРРО (gRPC/tonic, crypto, xml, offline_queue, shift) | Rust_Agent + apiarm_agent | ⏳ | sandbox-сертифікація; golden parity; shadow-mode; відкат |
| 8 | Дезактивація Python | Tauri_Agent + Git Admin Agent | ⏳ | єдиний бінарник; e2e зелений; updater |

Паралельно: QA_Agent — тестовий контур (differential/golden/proptest) з етапу 1.
Паралельно з етапом 5: ПРРО-дослідження (JKS→PKCS12/PEM, FFI vs gRPC).

## 1. Критичний шлях

```
Етап 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8
```

## 2. Декомпозиція ЕТАПУ 0 (активний)

**DoD етапу 0** (з RUST_MIGRATION_PLAN.md §5):
- [ ] cargo workspace збирається; Tauri-команди працюють через крейти
- [ ] фасад :8000 приймає запити, проксі на Python :8001 працює
- [ ] фронтенд працює БЕЗ змін (axios → :8000)
- [ ] `cargo test` зелений; differential-міст (Rust CLI) зібраний

### Задачі (contracts)

| # | Задача | Виконавець | Критерій прийняття | Статус |
|---|---|---|---|---|
| 0.1 | Створити kasa-rs workspace + рефакторинг існуючих 3 916 LOC у крейти (domain/application/infrastructure/api/tauri-shell) | Rust_Agent | `cargo build` чисто; `cargo test` зелений; Tauri-команди працюють через крейти | ⏳ |
| 0.2 | axum-фасад :8000 (JWT-валідація, reverse-proxy → Python :8001, health) | Rust_Agent | `curl :8000/api/v1/health` → 200; проксі на :8001 працює | ⏳ |
| 0.3 | Python sidecar: переїзд на :8001, spawn/kill з flush черг | Tauri_Agent | Python стартує на :8001; Tauri піднімає/вбиває коректно | ⏳ |
| 0.4 | CI-скелет (fmt/clippy/test, sqlx prepare) + гілка/PR | Git Admin Agent | GitHub Actions зелений; PR відкрито | ⏳ |
| 0.5 | Differential-міст (Rust CLI для порівняння) | Rust_Agent | CLI збирається, приймає JSON, повертає normalized результат | ⏳ |

### Правила етапу 0
- Жодного big-bang: кожен крок — окремий PR з feature-flag.
- Схема БД належить Alembic (Python) до хендоффу модуля.
- Гроші — тільки Decimal (rust_decimal), f64 у фінансах = fail.
- ПРРО не чіпати до етапу 7.

## 3. Журнал делегувань

| Дата | Контракт | Агент | Результат |
|---|---|---|---|
| 2026-08-07 | 0.1 workspace+рефакторинг | Rust_Agent | ⏳ очікує |
| 2026-08-07 | 0.2 axum-фасад | Rust_Agent | ⏳ очікує |
| 2026-08-07 | 0.3 sidecar :8001 | Tauri_Agent | ⏳ очікує |
| 2026-08-07 | 0.4 CI+PR | Git Admin Agent | ⏳ очікує |
| 2026-08-07 | 0.5 differential CLI | Rust_Agent | ⏳ очікує |
