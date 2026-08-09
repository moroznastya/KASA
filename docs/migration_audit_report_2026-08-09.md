# Аудит повноти міграції Kasa POS: Python → Rust

**Дата:** 2026-08-09
**Аудитор:** QA_Agent (NIKO 4.0.0)
**Об'єкт:** /home/anastasia/Andriy/aegis_v3/Niko/Projects/kasa
**Метод:** статичний аналіз (grep/parse) Rust-коду, Tauri-конфігурації, Docker/CI/автозапуску, порівняння роутів FastAPI vs axum.

---

## 1. Активні Python-виклики з Rust/Tauri — **НІ** ✅

### 1.1 Sidecar-конфігурація в tauri.conf.json — відсутня
`frontend/src-tauri/tauri.conf.json` — **немає** `externalBin`, немає `shell`-секції з sidecar. Плагіни: тільки `updater` (рядок `"plugins": {"updater": {...}}`). Схема у `$schema` веде на nicegui/tauri — косметика, не впливає.

### 1.2 Дозволи shell-плагіна — spawn/sidecar ЗАБОРОНЕНО
- `frontend/src-tauri/Cargo.toml`: `tauri-plugin-shell = "2"` присутній.
- `frontend/src-tauri/capabilities/default.json`: з shell-дозволів **тільки** `"shell:allow-open"`. **Немає** `shell:allow-spawn`, `shell:allow-execute`, `shell:allow-sidecar`. Rust-код `spawn`/`sidecar` не використовує (grep по `src/` + `crates/` — нуль збігів поза коментарями).

### 1.3 Rust-код — жодного виклику python-бінарника
Єдині `Command::new(...)` в Rust:
- `src/main.rs:31` → `pkg-config` (перевірка WebKit версії при запуску);
- `crates/kasa-infrastructure/src/print/mod.rs:110,441`, `print_templates.rs:694`, `devices/mod.rs:724,779` → `lp`, `lpstat`, `scanimage` (друк/сканери).
Викликів `python`, `python3`, `uvicorn`, sidecar-бінарників — **0**.

### 1.4 Fallback-проксі на Python — дезактивовано
- `crates/kasa-api/src/router_v1.rs:540`: `.fallback(proxy::proxy_handler)`.
- `crates/kasa-api/src/proxy.rs` (весь файл): fallback **не проксіює** на 127.0.0.1:8001 — повертає **410 Gone** (`endpoint_deprecated`, "Python sidecar дезактивовано"). Юніт-тести в тому ж файлі підтверджують (unknown_route_returns_410, legacy_route_returns_json_detail).

### 1.5 Коментарі дезактивації в lib.rs
- `frontend/src-tauri/src/lib.rs:199-213`: "Повна дезактивація Python sidecar: фасад биндиться на :8000 і [Python] не запускається".
- `lib.rs:290`: shutdown-hook — "Python sidecar дезактивовано (етап 8) — нема чого зупиняти".
- Фасад слухає `127.0.0.1:8000` (`crates/kasa-api/src/lib.rs:46`, `DEFAULT_FACADE_ADDR`).

### 1.6 Runtime-стан
Python-процеси (uvicorn/fastapi) **не запущені**; порти 8000/8001/8002 не слухаються (Tauri не запущено на момент аудиту).

**Висновок:** активних Python-викликів у продакшн-шляху **немає**. Критичних аномалій не виявлено.

---

## 2. Вмикання Python backend — **НЕ вмикається** ✅

| Канал | Стан | Доказ |
|---|---|---|
| docker-compose | Backend-сервіс **вимкнено** (`profiles: ["legacy"]`) | `docker-compose.yml` рядок `backend:` → `profiles: ["legacy"]`, коментар "⚠️ LEGACY (етап 8: Python sidecar дезактивовано)". `docker compose up -d` піднімає лише `db` |
| .env (корінь) | Тільки DB-змінні; **немає** `KASA_USE_PYTHON` | `.env`: `SECRET_KEY`, `DB_PASSWORD`, `DB_USER`, `DB_NAME` |
| .env (src-tauri) | Тільки ключі Tauri updater | `frontend/src-tauri/.env`: `TAURI_SIGNING_PRIVATE_KEY_PATH/PASSWORD` |
| Env-прапори Rust | `KASA_RUST_*=1` — дефолт у коді, не в .env | `crates/kasa-api/src/lib.rs:84-98` — `DEFAULT_RUST_FLAGS` 12 флагів, усі `"1"` |
| systemd | Немає юнітів | `/etc/systemd/system/` та `~/.config/systemd/user/` — 0 збігів kasa/pos |
| Autostart | Rust-бінарник, але **мертвий шлях** | `~/.config/autostart/"Kasa POS.desktop"` → `Exec=/home/anastasia/nastya/aegis_v3/kasa/frontend/src-tauri/target/debug/kasa-pos --autostart` — шлях **не існує** (старий шлях, `/home/anastasia/nastya/...` замість `/home/anastasia/Andriy/...`) |
| scripts/ (корінь) | 1 файл — dev-інструмент | `scripts/gen_golden_vectors.py` — генерація golden-векторів для Rust-тестів, не runtime |
| CI | Python **АКТИВНИЙ у CI** | `.github/workflows/ci.yml`: `lint-backend` (ruff+mypy), `test-backend` (pytest, 466 unit-тестів, coverage), `build` (Docker image backend). `rust-core.yml` — окремий job (cargo fmt/clippy/test) |

**Висновок:** Python backend не вмикається жодним каналом у runtime. **Єдине активне використання Python — CI** (як еталон тестування та Docker-збірка). Autostart-файл — мертвий артефакт (бітий шлях).

---

## 3. Стан залишків

### 3.1 `backend/` (FastAPI) — 568 МБ, мертвий у runtime, ЖИВИЙ як еталон
- Останній коміт: `71b5dcf feat(rust): повна дезактивація Python sidecar (етап 8)` — backend свідомо переведений у legacy.
- **Використовується:**
  - CI: `pytest` (unit+integration), ruff/mypy, Docker build — `.github/workflows/ci.yml`;
  - docker-compose `backend` (профіль legacy) — для відкату/налагодження;
  - еталон parity для Rust-тестів: численні `1:1 Python` коментарі та golden-фікстури в `crates/kasa-*` (напр. `kasa-prro/tests/golden_xml.rs:3` "Вектори згенеровані з Python-еталона (backend/venv/bin/python...)"), `backend/venv` наявний.
- **Висновок: НЕ видаляємий зараз** — потрібен як еталон для differential-тестів і CI. Видаляємий після повного переведення еталонів на Rust (див. рекомендації).

### 3.2 `ui/` (CustomTkinter) — 76 КБ, мертвий, НЕ використовується
- Останній коміт: `5643363 v3.0.1 — Clean Architecture міграція...` (липень).
- Жодних згадок у: docker-compose, CI, скриптах, Rust-коді, README.
- **Висновок: видаляємий** (або архівувати в окрему гілку/tag). Референсна цінність відсутня — функціонал повністю покритий React-фронтендом.

### 3.3 Документація — застаріла
- `README.md:7` досі описує **Backend: FastAPI** як основний компонент; `README.md:40-64` — інструкції запуску `python -m venv venv`, `:8001`, Swagger; `README.md:80-85` — "docker-compose up -d (Вся система PostgreSQL + Backend + Frontend)" — **брехня після етапу 8** (backend має profile legacy).
- `STRUCTURE.md`, `SYSTEM_STATE.md` — ті самі застарілі описи.

---

## 4. Покриття API-роутів: **157/164 (95.7%)** ✅

Метод: парсинг `@router.<method>("...")` у `backend/app/api/v1/*.py` + `v2/*.py` (з префіксами роутерів) проти `.route("...", method(...))` у `crates/kasa-api/src/*.rs` (з балансом дужок, ураховуючи багатометодні `.get().put().delete()`). Нормалізація параметрів `:id`/`:category_id` → generic.

| Метрика | Значення |
|---|---|
| Роутів Python (метод+шлях, v1+v2) | **164** |
| Роутів Rust (з аліасами) | **182** |
| Збігаються 1:1 | **157** |
| Відсутні в Rust | **7** |

### Невідповідні 7 роутів (усі — НЕ критичні)

| Роут Python | Файл | Стан у Rust | Наслідок |
|---|---|---|---|
| `GET /api/v1/categories/tree` | v1/categories.py | Є тільки `GET /api/v2/categories/tree` | v1 deprecated; фронтенд кличе v2 → 410 |
| `POST /api/v2/auth/login` | v2/auth.py | Є `POST /api/v1/auth/login` | v2-аліас; фронтенд (baseURL `/api/v1`) не кличе → 410 |
| `POST /api/v2/auth/login-pin` | v2/auth.py | Є `POST /api/v1/auth/login-pin` | те саме |
| `POST /api/v2/auth/refresh` | v2/auth.py | Є `POST /api/v1/auth/refresh` | те саме |
| `GET /api/v2/auth/users` | v2/auth.py | Є `GET /api/v1/users` | те саме |
| `GET /api/v2/auth/users/me` | v2/auth.py | Фронтенд кличе `GET /api/v1/auth/verify` (`authStore.ts:132`) | те саме |
| `POST /api/v2/auth/users` | v2/auth.py | Є `POST /api/v1/users` | те саме |

**Ключові підтвердження покриття:**
- `router_v1.rs:331`: `.route("/api/v1/debtors", get(debtors::list).post(debtors::create))` — CRUD боржників повністю;
- `router_v1.rs:307-310`: `/api/v1/users` get+post; `302-306`: `/:user_id` get+put+delete; `292-301`: permissions/hourly-rate;
- `router_v1.rs:311-324`: `PUT /api/v1/settings` (batch) — фронтенд `SettingsPage.tsx:1029` `api.put('/settings', ...)` — **покрито**;
- `router_v1.rs:364-436`: інвойси v1+v2 (list/create/get/update/delete/confirm/payment-info/price-changes/print-items/cancel);
- `router_v1.rs:249-253`: `PUT /api/v2/prro/settings` — **покрито**;
- `router_v1.rs:490-544`: товари v2 + v1-аліаси (images/barcodes/uploads);
- `return_invoices.rs:280-288`: повернення (list/create/confirm);
- `proxy.rs`: невідомі шляхи → 410 (явна деприкація, не тихий 502).

**Висновок:** функціональних пропусків у покритті **немає**. 7 відсутніх — виключно невикористовувані v2-аліаси auth та v1-deprecated tree, які фронтенд не викликає (baseURL `VITE_API_BASE_URL || http://localhost:8000/api/v1`, `src/services/api.ts`; Vite proxy `/api → localhost:8000`, `vite.config.ts`).

---

## 5. E2E-скрипти у frontend/src-tauri/scripts/ — переважно артефакти ⚠️

| Група | Файли | Стан |
|---|---|---|
| Differential (етапи 1-7) | `e2e_auth_diff.sh`, `e2e_categories_v2_diff.sh`, `e2e_crud_diff.sh`, `e2e_debtors_diff.sh`, `e2e_documents_diff.sh`, `e2e_invoices_diff.sh`, `e2e_ledger_diff.sh`, `e2e_ocr_diff.sh`, `e2e_pos_diff.sh`, `e2e_print_diff.sh`, `e2e_products_v2_diff.sh`, `e2e_prro_v2_diff.sh`, `e2e_purchase_orders_diff.sh`, `e2e_return_invoices_diff.sh`, `e2e_alias_diff.py`, `e2e_receipts_post_diff.py`, `e2e_suppliers_products_diff.py` | **НЕАКТУАЛЬНІ.** Усі вимагають Python :8001 + Rust :8002 одночасно (напр. `e2e_alias_diff.py:25-26`: `PY = "http://127.0.0.1:8001"`, `RS = "http://127.0.0.1:8002"`; `e2e_auth_diff.sh:3`: "Rust-фасад (:8002, KASA_RUST_AUTH=1) vs Python (:8001)"). Python дезактивовано → скрипти не можуть виконатися. Виконувалися лише під час міграції |
| Rust-фасад (етап 5) | `e2e_stage5_tauri.sh` | **АКТУАЛЬНИЙ формат** — тестує Rust-фасад :8000 + офлайн-чергу (SQLite), не потребує Python |

**Висновок:** 16 із 17 e2e-скриптів — артефакти міграції (diff Python↔Rust). `e2e_stage5_tauri.sh` — зразок актуального підходу.

---

## 6. Загальний висновок

### Міграція **ПОВНА** ✅

Продакшн-шлях **100% Rust**: React (Vite proxy) → Tauri-оболонка → вбудований axum-фасад `:8000` (`crates/kasa-api`) → PostgreSQL. Python **не викликається** (0 викликів у Rust-коді, sidecar-дозволи відсутні, fallback → 410), **не вмикається** (docker-compose legacy-profile, немає systemd/autostart, немає env-прапорів активації), **не входить у runtime** (жодного python-процесу).

### Недоліки / артефакти (не блокуючі)
1. **CI досі збирає і тестує Python backend** (`ci.yml`: lint-backend, test-backend 466 тестів, Docker build) — Python живе в пайплайні як еталон. Не помилка, але подвійне обслуговування.
2. **7 роутів** (v2-auth аліаси + v1 tree) не мігровані — повертають 410. Функціонал не втрачено, але parity неповна на 100%.
3. **Autostart .desktop** вказує на неіснуючий шлях `/home/anastasia/nastya/...` — автозапуск фактично зламаний.
4. **README.md/STRUCTURE.md/SYSTEM_STATE.md** застарілі — описують Python-бекенд як основний.
5. **16 e2e-diff-скриптів** — неактуальні артефакти.
6. `ui/` (CustomTkinter) — мертвий код без жодних посилань.

---

## 7. Рекомендації

| # | Дія | Пріоритет |
|---|---|---|
| 1 | **Видалити `ui/`** (або зафіксувати архівним tag `legacy/ui-customtkinter-v3.0.1`) — мертвий код, 0 посилань | Високий |
| 2 | **Оновити autostart** `~/.config/autostart/Kasa POS.desktop` → актуальний шлях бінарника (або перейти на Tauri autostart-плагін, який уже зареєстрований у `lib.rs:105-112`) | Високий |
| 3 | **Оновити README.md/STRUCTURE.md/SYSTEM_STATE.md**: прибрати інструкції запуску Python (`venv`, `:8001`, `docker-compose up -d` = вся система), описати Rust-фасад `:8000` | Середній |
| 4 | **Видалити 16 неактуальних e2e_*_diff скриптів** (або перенести в `docs/scr/` як історію міграції); лишити/розвивати `e2e_stage5_tauri.sh` як e2e-стандарт | Середній |
| 5 | **`backend/` (568 МБ)**: зберегти, поки CI та golden-тести залежать від Python-еталона. Коли Rust-тести стануть самодостатніми — видалити `backend/`, `backend/venv`, `backend/htmlcov`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache` і прибрати Python-jobs з CI | Пізніше |
| 6 | **docker-compose.yml**: залишити `backend` з `profiles: ["legacy"]` (відкат), але оновити healthcheck — він використовує python-urllib лише всередині legacy-контейнера; винести у окремий файл `docker-compose.legacy.yml` | Низький |
| 7 | **Документувати 7 непокритих роутів** як свідомо деприкейтнуті (410 Gone) у `docs/RUST_MIGRATION_EXECUTION.md` | Низький |
| 8 | **CI**: додати `rust-core` у `needs` summary-джоба; вирішити долю `lint-backend`/`test-backend` (етап 5 після повного переходу еталонів) | Низький |

---

*Звіт сформовано статичним аналізом; runtime-перевірку (запуск Tauri + smoke :8000) див. у `docs/rust_deactivation_audit.md` та `docs/RUST_MIGRATION_EXECUTION.md` (етап 8, cargo test 186/186).*
