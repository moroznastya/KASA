# Помилки ПРРО — код + точний текст для користувача

Статус: РЕАЛІЗОВАНО (2026-08-27)
Стек: Rust (torgashka-prro, torgashka-api) + Python backend (FastAPI) + GUI (React/Tauri)

## Вимога

При будь-якій помилці ПРРО користувач бачить ТОЧНИЙ текст і код (якщо є),
а не загальне «Помилка фіскалізації». Формат: `[КОД] Точний текст`.

## Аудит (знайдені місця «ковтання» коду)

| Шар | Проблема до виправлення |
|-----|------------------------|
| `PrroFiscalizeError` (Rust) | `Display = "{message}"` — код існував у структурі, але губився в `to_string()` |
| `PrroShiftError` (Rust) | те саме |
| `PrroSettingsError` (Rust) | те саме |
| `PrroApiError` (torgashka-api) | `Display` варіантів без кодів; API віддавав `e.to_string()` без `[КОД]` |
| `fiscalize.rs` / `use_case.py` (callsites `on_error`) | `error_message` = тільки текст сервера або «ПРРО: статус N» — код статусу ДПС (-3/-12/...) губився; `DTO.error` / `receipt.fiscal_error` без коду |
| Python `PrroFiscalizeError` / `PrroShiftError` / `PrroSettingsError` | `__str__` не перевизначено → `str(e)` = тільки message |
| `test_connection` (ping) | error без `[КОД]` префікса |
| GUI `prroStore.ts` / `PosPage.tsx` | обгортка «Помилка фіскалізації: {error}» — замінено на показ as-is |

## Рішення

### Rust `torgashka-prro`
- `fiscalize.rs`:
  - `PrroFiscalizeError::Display` → `[{code}] {message}`.
  - Нова `pub fn status_name(i32) -> String`: мапа статусів ДПС
    (1→OK, -1→ERROR_VEREFY, ..., -12→ERROR_BAD_HASH_PREV, -16→ERROR_OFFLINE_ID,
    невідомі → `STATUS_{n}`).
  - Нова `pub fn server_error_text(status, error_message) -> String`:
    `[{status_name}] {текст сервера}`, а якщо текст порожній — людське
    пояснення статусу. Використовується в усіх callsites `on_error`.
  - Мережеві помилки → `[GRPC_ERROR] ...` (без плутанини з ERROR_VEREFY).
- `shift.rs`: `PrroShiftError::Display` → `[{code}] {message}`.
- `settings.rs`: `PrroSettingsError::Display` → `[PRRO_SETTINGS_ERROR] {message}`;
  `test_connection` error тепер з `[{status_name}] ...`.

### Rust `torgashka-api`
- `PrroApiError::Display` варіантів з кодами:
  `[PRRO_SHIFT_ERROR]`, `[PRRO_REPO_ERROR]`, `[GRPC_ERROR]`, `[CRYPTO_ERROR]`,
  `[KEYSTORE_ERROR]`, `[XML_ERROR]`, `[QUEUE_ERROR]`, `[PRRO_SETTINGS_ERROR]`,
  Fiscalize — динамічний `[{code}]` з `PrroFiscalizeError`.
- Всі HTTP-хендлери вже віддають `e.to_string()` → тепер завжди з кодом.

### Python backend (FastAPI)
- `fiscalize_receipt_use_case.py`: `PrroFiscalizeError.__str__` =
  `[{code}] {message}`; нові `status_name()` / `server_error_text()` (1:1 Rust);
  всі callsites `_on_error` формують `[КОД] текст`.
- `shift_use_case.py`: `PrroShiftError.__str__` = `[{code}] {message}`.
- `prro_settings_use_case.py`: `PrroSettingsError.__str__` =
  `[{code}] {message}`; `test_connection` error з `[{status_name}] ...`.
- FastAPI-хендлери віддають `HTTPException(detail=str(e))` → код у detail.

### GUI (React/Tauri)
- `prroStore.ts`: `result.error` (вже `[КОД] Текст`) показується as-is,
  без обгортки «Помилка фіскалізації:».
- `PosPage.tsx`: `fiscal_error` (з БД, зберігається з кодом) — as-is.

## Приклади фінальних повідомлень

1. З кодом (помилка ДПС, сервер повернув ERROR_SAVE / -3):
   `[ERROR_SAVE] Помилка запису на сервері (ERROR_SAVE). Спробуйте пізніше.`
   — або з текстом сервера: `[ERROR_SAVE] Server rejected receipt`
2. Без коду сервера (мережева помилка):
   `[GRPC_ERROR] gRPC send_chk не вдався: transport error`
3. З числовим кодом ДПС (-12):
   `[ERROR_BAD_HASH_PREV] Невірний хеш попереднього чеку (ERROR_BAD_HASH_PREV).`

## Тести

### Rust
- Юніт (torgashka-prro `fiscalize.rs`): `fiscalize_error_display_includes_code`,
  `status_name_maps_server_codes`, `server_error_text_includes_code_and_server_message`,
  `server_error_text_falls_back_to_human_message_when_empty`,
  `on_error_dto_error_carries_code`.
- Юніт (torgashka-api `prro.rs`): `api_error_fiscalize_display_includes_code`,
  `api_error_shift_display_includes_code`, `api_error_repo_display_includes_code`,
  `api_error_settings_display_includes_code`.
- Інтеграція: `prro_facade.rs` — виправлено сигнатуру `add_document` (B2/B4).

### Python
- `tests/unit/use_cases/test_prro_errors.py` (14 тестів): `__str__` з кодом,
  `status_name`, `server_error_text`, числовий код ДПС.
- Оновлено `test_prro_fiscalize_use_case.py::test_fiscalize_server_error`:
  `error == "[ERROR_UNKNOWN] Unknown error"` (DTO, БД, черга).

## Результати прогонів (все через run_limited.sh)

| Прогін | Результат |
|--------|-----------|
| `cargo test -p torgashka-prro` | 97 passed, 6 ignored, 0 failed |
| `cargo test -p torgashka-api --lib` | 25 passed, 0 failed |
| `cargo test -p torgashka-api --test prro_bugs` | 3 passed, 0 failed |
| `cargo test -p torgashka-api --test prro_facade` | 4 passed, 1 failed (середовищний гейт, нижче) |
| `cargo test -p torgashka-infrastructure --lib` | 96 passed, 0 failed |
| `cargo clippy -p torgashka-prro -p torgashka-api` | нових warning немає (існуючі: devices/pos/setup — не ПРРО) |
| `pytest` (повний backend) | 639 passed, 83 deselected, 0 failed |
| `ps aux \| grep cades_iit` | 0 процесів |

## Аномалії середовища (НЕ регресії, не ПРРО)

1. `status_reports_rust_gate` (torgashka-api): очікує `rust_gate==true`,
   але в спільній PostgreSQL налаштування `false` (Python-гілка активна).
   Тест-гейт залежить від зовнішнього стану БД.
2. `cash_operations_http_roundtrip` (torgashka-api): E2E зі спільною БД —
   накопичені операції попередніх запусків змінюють баланс (550 замість 500).
3. `put_onboarding_completed_persists_in_db` (torgashka-api): E2E зі спільною БД.

Усі три — не торкаються коду помилок ПРРО (змінені файли цієї задачі не
входять у їхній шлях виконання) і падали б і без цих змін. Потребують
ізоляції тестової БД (окрема схема/контейнер) — поза зоною цієї задачі.

---

# Задача 2: КОД + ІМ'Я + ЛЮДСЬКИЙ ОПИС (status=-13 → ERROR_NOT_REGISTERED_RRO)

Статус: РЕАЛІЗОВАНО (2026-08-27)

## Проблеми (підтверджені координатором)
1. `prroService.ts` extractErrorMessage дивився тільки `response.data.detail`;
   Rust-шлюз при помилці віддавав text/plain → fallback «Помилка запиту до ПРРО».
2. `pos.rs:3544-3571` — ЗАГЛУШКИ open_shift/close_shift з хардкодом «status=-13».
3. `shift.rs` / `shift_use_case.py` — error_msg = `status={n}` без імені/опису.
4. `prro.rs test_connection` — Err без JSON `detail` (несумісно з FastAPI-контрактом).

## Активна гілка (підтверджено)
**Rust-шлюз axum на 127.0.0.1:8000** (дезактивація Python):
- Tauri `lib.rs:253-267` біндить :8000 (`DEFAULT_FACADE_ADDR`);
- DEV Vite-проксі → `http://localhost:8000`, production API_ROOT → `http://127.0.0.1:8000`;
- `DEFAULT_RUST_FLAGS`: TORGASHKA_RUST_PRRO=1, TORGASHKA_RUST_PRRO_V2=1 → всі
  ПРРО-маршрути GUI (`/shift/open`, `/test-connection`, `/settings`, `/fiscalize`)
  виконує Rust (torgashka-api + torgashka-prro).

## Рішення
1. **Спільна таблиця кодів ДПС** (-1..-16 + OK):
   - Rust: `torgashka-prro/src/prro/status_codes.rs` — `DPS_STATUS_CODES`,
     `status_name`, `status_description_uk`, `status_error_text`;
   - Python: `backend/app/application/use_cases/prro/status_codes.py` (1:1);
   - `fiscalize.rs` / `fiscalize_receipt_use_case.py` тепер використовують
     спільне джерело (прибрано дублювання _STATUS_NAMES).
2. **shift.rs / shift_use_case.py**: `status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО
   не зареєстровано)`; текст сервера — ПОВНІСТЮ + статус: `"текст | status=..."`.
   Винесено в тестований helper `shift_status_error_text` (Rust).
3. **pos.rs (infrastructure)**: хардкод-заглушки прибрано → чесна помилка
   «Фіскальну гілку ПРРО не підключено (TORGASHKA_RUST_PRRO=0)».
   **pos.rs (api)**: open_shift/close_shift при наявності Rust-фасаду
   делегують на реальну реалізацію torgashka-prro (PrroShiftUseCase).
4. **prro.rs (api)**: ВСІ Err-гілки → FastAPI-сумісний JSON `{"detail": "..."}`
   (helper `api_err`), включно з test-connection (було text/plain).
5. **prroService.ts**: extractErrorMessage винесено в `prroErrors.ts`,
   порядок: detail (string/array/object) → error (string/object) → message →
   data-string → Error.message → fallback. testConnection: `ok===false` →
   `data.error` як message (не ховається).
6. **test_connection (Rust+Python)**: додано `status_error_text(status)` →
   числовий код + ім'я + опис у відповіді.

## Приклади фінальних повідомлень GUI
- **open_shift, status=-13 без тексту сервера**:
  `Не вдалося відкрити зміну: status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)`
- **open_shift з текстом сервера**:
  `Не вдалося відкрити зміну: RRO not registered | status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)`
- **test-connection (HTTP 400, Rust-шлюз)** — JSON `{"detail": "..."}` з кодом/текстом
  (більше не fallback «Помилка запиту до ПРРО»).

## Тести
| Прогін | Результат |
|--------|-----------|
| `cargo test -p torgashka-prro` | 107 passed, 0 failed |
| `cargo test -p torgashka-api --lib` | 27 passed (+2 api_err JSON) |
| `cargo test -p torgashka-api --test prro_bugs` | 3 passed |
| `cargo test -p torgashka-infrastructure --lib` | 96 passed |
| `pytest` (повний backend) | 647 passed (+8 status_codes), 0 failed |
| TS `prroErrors.test.mjs` (esbuild+node:assert) | ALL TESTS PASSED (9 сценаріїв) |
| `cargo clippy -p torgashka-prro` | 0 warning (новий код чистий) |
| `ps aux \| grep cades_iit` | 0 |

## Відомі середовищні флейки (НЕ регресії)
- `prro_facade::status_reports_rust_gate` — очікує `configured==false`, але в
  спільній PostgreSQL ПРРО вже налаштований (рядок 36, падав і до змін).
