# Panic-Fix Звіт — R1/R2 (rust_audit_report_2026-08-09.md)

Дата: 2026-08-09
Виконавець: Rust_Agent (NIKO)
Проєкт: Kasa POS (`frontend/src-tauri`)
Статус: ✅ ВИКОНАНО, всі критерії прийняття задоволені

---

## 1. R1 — `try_get().expect()` → типізована помилка (`?`)

**Проблема:** 22× `r.try_get("col").expect("...")` у `ledger.rs` — panic у runtime при зміні схеми/мапінгу.

**Рішення:** усі замінені на `r.try_get("col").le()?` через наявний локальний конвертер
`SqlxResultExt::le()` (sqlx::Error → `LedgerError::Infrastructure`) → повертається **Err (500)**, не panic.

Трансформація: 3 `.map(|r| Dto {...}).collect()` → `.map(|r| -> Result<Dto, LedgerError> { Ok(Dto {...}) }).collect::<Result<Vec<_>, _>>()?`

| Файл | Замінено |
|---|---|
| `crates/kasa-infrastructure/src/repositories/ledger.rs` | **22** (18 однорядкових + 4 багаторядкові з `try_get::<Option<T>, _>(...)`) |

Залишок `try_get` + `.expect` у production: **0** (перевірено включно з багаторядковими).

---

## 2. R2 — `Mutex::lock().unwrap()` → `lock().expect("lock poisoned: <контекст>")`

**Проблема:** 47× (38 однорядкових + 9 багаторядкових) — отруєний м'ютекс → panic без діагностики.

**Рішення:** заміна на `lock().expect("lock poisoned: <field>")` з іменем поля-м'ютекса
(shifts/queue/settings/receipts/products/responses/calls/response/status/connections).
Мінімальна зміна — логіка недоторкана.

| Файл | Замінено |
|---|---|
| `crates/kasa-prro/src/prro/repository.rs` | **32** (23 + 9 багаторядкових) |
| `crates/kasa-infrastructure/src/devices/mod.rs` | **8** |
| `crates/kasa-prro/src/prro/chk_sender.rs` | **7** |

Залишок `lock().unwrap(` у production: **0** (перевірено включно з багаторядковими).
Всього `lock poisoned` expect: **47**.

---

## 3. Результати верифікації

| Перевірка | Результат |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ exit 0, чисто |
| `cargo test --workspace` | ✅ **186 passed / 0 failed / 0 ignored** (31 бінарник, включно з 22 тестами kasa-ocr) |
| `cargo build --workspace` | ✅ 0 warnings |
| `cargo fmt` | ✅ застосовано |

> Примітка: очікування "208+ passed" не підтвердилось — 186 вже включає 22 тести kasa-ocr
> (перевірено окремим запуском `cargo test -p kasa-ocr` → 22 passed). 0 failed — критерій виконано.

---

## 4. Залишені легітимні unwrap/expect

**90** у production-коді (поза `#[cfg(test)]`, без `unwrap_or*` та `lock poisoned`).
Вибірково перевірено — це fail-fast контексти (init/setup/константи):
- `serde_json::to_value(...).unwrap()` — серіалізація derive-DTO (гарантована)
- `Regex::new(...).unwrap()` — статичні regex-патерни
- парсинг конфігів/env при старті

**НЕ чіпались** — поза межами задачі (R1/R2), згідно з інструкцією.

---

## 5. Змінені файли (підсумок)

1. `crates/kasa-infrastructure/src/repositories/ledger.rs` — 22× expect → `?`
2. `crates/kasa-prro/src/prro/repository.rs` — 32× unwrap → expect("lock poisoned: ...")
3. `crates/kasa-infrastructure/src/devices/mod.rs` — 8× unwrap → expect("lock poisoned: ...")
4. `crates/kasa-prro/src/prro/chk_sender.rs` — 7× unwrap → expect("lock poisoned: ...")

Поведінка (SQL-запити, роути, DTO) — **недоторкана**. Git-коміт — на Git Admin Agent.
