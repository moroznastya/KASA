# План реалізації embedded PostgreSQL для Windows-збірки Torgashka

> Версія: 1.1.0
> Дата: 2026-08-21
> Статус: план до виконання
> Автор: PM_Agent (NIKO)
> Зміни v1.1.0: додано ключовий сценарій «створення товару» (POST /api/v1/products)
> у критерії прийняття, тест-план (Linux/Windows) та чекліст (уточнення Творця).

## 0. Фактична база (перевірено 2026-08-21)

| Параметр | Значення |
|---|---|
| Корінь проєкту | `/home/anastasia/Andriy/aegis_v3/Niko/Projects/kasa` |
| Rust-ядро | `frontend/src-tauri/crates/` — workspace: `torgashka-api`, `torgashka-application`, `torgashka-domain`, `torgashka-infrastructure`, `torgashka-ocr`, `torgashka-prro` |
| Робота з БД | `crates/torgashka-infrastructure/src/db.rs` — `resolve_database_url()` (env `DATABASE_URL` → fallback `backend/.env` DB_*), `ensure_schema(&pool)` (рядок 205, схема з `schema.sql`: 34 таблиці) |
| Точка запуску API | `crates/torgashka-api/src/lib.rs:518` — `serve_listener(listener)`; перед listener: `connect_readonly_pool(5)` → `ensure_schema` → `pool.close()` |
| Виклик із Tauri | `frontend/src-tauri/src/lib.rs:267` — єдине місце: `torgashka_api::serve_listener(listener).await` |
| Ендпоінти товарів | `POST /api/v1/products` (crud::create_product), `GET /api/v1/products` (readdirs::list_products); v2: `POST /api/v2/products` — **ключовий сценарій «створення товару»** |
| `backend/.env` | Містить: `DB_NAME`, `DB_USER`, `DB_PASSWORD`, `DB_HOST`, `DB_PORT`, `SECRET_KEY`, `ACCESS_TOKEN_EXPIRE_MINUTES`, `APP_NAME`, `DEBUG`, `CORS_ORIGINS` |
| Локальний PG (Linux) | PostgreSQL **17.6**, бінарники: `/usr/lib/postgresql/17/bin` (`psql`, `pg_ctl`, `initdb`) |
| Windows PG (ціль) | `postgresql-17.6-1-windows-x64-binaries.zip` (EDB, ~330MB) — версія ЗБІГАЄТЬСЯ з локальною (важливо для тестів) |
| API facade | `127.0.0.1:8000` (Rust-ядро, Python sidecar деактивовано) |
| Tauri | v2, `tauri-plugin-process`, `tauri-plugin-shell` у залежностях |

### Ключовий факт для інтеграції

`resolve_database_url()` має пріоритет: **env `DATABASE_URL` → `backend/.env`**. Отже PostgresManager може НЕ чіпати існуючий код db.rs: достатньо перед `serve_listener` встановити `DATABASE_URL`, вказавши на локальний PG. Існуючий `ensure_schema` створить схему сам.

### Зафіксовані унікальні порти/шляхи (SSOT для всього плану)

| Сутність | Значення |
|---|---|
| PG listen | `127.0.0.1:5433` (fallback: 5434…5440) |
| API facade | `127.0.0.1:8000` (не змінюється) |
| pgdata (Windows) | `%APPDATA%\com.torgashka.pos\pgdata` (через `app.path().app_data_dir()`) |
| pgdata (Linux) | `$XDG_DATA_HOME/torgashka/pgdata` → за замовчуванням `~/.local/share/torgashka/pgdata` |
| Бінарники PG (у збірці) | `<resource_dir>/pg/bin`, `<resource_dir>/pg/lib`, `<resource_dir>/pg/share` |
| Логи PG | `<pgdata>/postgres.log` |
| Юзер/БД/пароль | з `backend/.env`: `DB_USER`, `DB_NAME`, `DB_PASSWORD` (єдине джерело) |

---

## 1. Архітектура: PostgresManager

### 1.1. Розміщення

Новий модуль у **`crates/torgashka-infrastructure/src/pg/`** (окрема підсистема, ізольована від `db.rs`):

```
crates/torgashka-infrastructure/src/
├── db.rs              # існуючий: resolve_database_url, ensure_schema (НЕ змінюється)
└── pg/
    ├── mod.rs         # pub use; PGError; константи (порт, шляхи)
    ├── manager.rs     # PostgresManager: initdb, start, stop, status, create_db, ensure_running
    ├── config.rs      # генерація/патч postgresql.conf, шляхи (cross-platform)
    └── process.rs     # spawn/terminate child process, очікування готовності (pg_isready)
```

### 1.2. Складові та їхні відповідальності

**`PostgresManager` (manager.rs)** — єдиний публічний API:

```rust
pub struct PostgresManager {
    bin_dir: PathBuf,      // каталог з initdb.exe/pg_ctl.exe/postgres.exe
    data_dir: PathBuf,     // pgdata
    port: u16,             // 5433 (fallback 5434..5440)
    user: String,          // DB_USER з backend/.env
    password: String,      // DB_PASSWORD з backend/.env
    db_name: String,       // DB_NAME з backend/.env
    log_file: PathBuf,     // <pgdata>/postgres.log
}

impl PostgresManager {
    pub fn from_env() -> Result<Self, PGError>;        // читає backend/.env (DB_*, шляхи з app_data_dir)
    pub fn ensure_running(&self) -> Result<String, PGError>; // idempotent: init? start? ready? → повертає DSN (DATABASE_URL)
    pub fn status(&self) -> PGStatus;                  // NotInitialized | Stopped | Running | Unknown
    pub fn shutdown(&self) -> Result<(), PGError>;     // pg_ctl stop -m fast; викликається при exit
    pub fn data_dir(&self) -> &Path;                   // для діагностики/логів
}
```

Поведінка `ensure_running()` (єдиний вхідний виклик з Tauri):

```
1. status():
   - pgdata/  не існує        → initdb (Етап 1)  [10-20 с на Windows]
   - pgdata/  існує, PG down  → pg_ctl start     [2-5 с]
   - pg_isready -p 5433 = OK  → вже працює (іdempotent, нічого не робити)
2. Якщо стартував сам: патч postgresql.conf (port, listen_addresses).
3. create_db(): якщо БД DB_NAME не існує — CREATE DATABASE (через psql до БД postgres).
4. Повертає DSN: postgresql://{user}:{pass}@127.0.0.1:{port}/{db_name}
```

**`config.rs`** — генерація конфігурації:

- Після `initdb` записати/допатчити `<pgdata>/postgresql.conf`:
  ```
  port = 5433
  listen_addresses = '127.0.0.1'
  unix_socket_directories = ''        # Windows: уникнути проблем з довгими шляхами сокетів
  logging_collector = off             # лог у файл через pg_ctl -l, не в каталог log/
  ```
- На Linux (`cfg!(target_os = "linux")`) `unix_socket_directories` лишити за замовчуванням (потрібен для psql/createdb без TCP).

**`process.rs`** — низькорівнева робота:

- `spawn(cmd, args)` → `std::process::Child` (без `Command::new` з конкатенацією рядків — передача args вектором, захист від пробілів у шляхах, особливо `C:\Users\...\AppData\...`).
- `wait_ready(port, timeout_ms)` — цикл `pg_isready -h 127.0.0.1 -p {port}` до 30 с, крок 500 мс.
- `terminate(child)` — на Windows `child.kill()` + очікування; або коректніше `pg_ctl stop -m fast`.

### 1.3. Auth: trust vs scram

**Рішення: `scram-sha-256`** (не trust), бо:
- `resolve_database_url()` будує DSN з `DB_PASSWORD` — пароль уже є в системі;
- локальний single-user POS — але пароль захищає від інших процесів на машині, що слухають 127.0.0.1 (наприклад, malware/інші застосунки юзера);
- нульова зміна конфігурації: initdb з `-A scram-sha-256 --pwfile=<tmp>` і `-U {DB_USER}` дає рівно той юзер/пароль, що в `backend/.env`.

Команда initdb (Windows і Linux — однакова):
```bash
initdb -D <pgdata> -U <DB_USER> -E UTF8 --locale=C -A scram-sha-256 --pwfile=<tmpfile>
```
> `--locale=C` + `-E UTF8` — уникнення проблем з ICU/locale на Windows; кирилиця зберігається як UTF-8 (для POS-даних сортування за байтами прийнятне). Якщо згодом знадобиться українська колація — перейти на `--locale-provider=icu --icu-locale=uk`.

---

## 2. Інтеграція з існуючим `db.rs` / `serve_listener`

### 2.1. Принцип: мінімальна інвазивність

`db.rs` **НЕ змінюється**. `resolve_database_url()` вже має пріоритет env `DATABASE_URL` — це і є точка інтеграції.

### 2.2. Хто і коли запускає PG

Єдине місце зміни в Rust-ядрі: `frontend/src-tauri/src/lib.rs` (блок запуску фасаду, ~рядок 247-267) та обробник завершення.

```rust
// ПОТОЧНИЙ код (спрощено):
// ... torgashka_infrastructure::devices::init_auto_connect(...)
// ... spawn serve_listener(listener)

// НОВИЙ код:
let pg = torgashka_infrastructure::pg::PostgresManager::from_env()?;
match pg.ensure_running() {
    Ok(dsn) => {
        // КЛЮЧОВЕ: DATABASE_URL у env → resolve_database_url() підхопить його
        // без змін у db.rs
        std::env::set_var("DATABASE_URL", &dsn);
        eprintln!("[torgashka-pg] embedded PostgreSQL ready: {dsn}");
    }
    Err(e) => {
        eprintln!("[torgashka-pg] ПОМИЛКА запуску embedded PG: {e}");
        // fallback: спробувати зовнішній PG (розробка) — НЕ блокувати запуск фасаду,
        // serve_listener сам залогує помилку підключення
    }
}
// ... далі serve_listener(listener) — ensure_schema виконається проти embedded PG
```

**Порядок гарантований:** `ensure_schema` викликається всередині `serve_listener` (api lib.rs:530-535) — отже PG вже запущено до цього моменту. Після застосування схеми роути `/api/v1/products` та `/api/v1/users` стають робочими проти embedded PG.

### 2.3. Створення БД (нова відповідальність)

`ensure_schema` створює **таблиці**, але не саму БД. На Linux розробника БД існує вручну. Для embedded PG `PostgresManager::ensure_running()` має створити `DB_NAME`, якщо її немає:

```bash
psql -h 127.0.0.1 -p 5433 -U <DB_USER> -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname='<DB_NAME>'" | grep -q 1 \
  || psql -h 127.0.0.1 -p 5433 -U <DB_USER> -d postgres -c "CREATE DATABASE \"<DB_NAME>\""
```
(у Rust — через `process.rs` з env `PGPASSWORD=<DB_PASSWORD>`)

### 2.4. Env-флаг для розробки

```rust
// Якщо TORGASHKA_EMBEDDED_PG=0 — пропустити embedded PG (зовнішній PG на 5432)
if std::env::var_os("TORGASHKA_EMBEDDED_PG").map(|v| v == "0").unwrap_or(false) {
    // старий шлях: покладатись на зовнішній PG через backend/.env
} else {
    // embedded PG (за замовчуванням у release-збірках)
}
```
У dev-режимі (`tauri dev`, Linux) за замовчуванням — embedded PG також (бо код той самий), з флагом `TORGASHKA_EMBEDDED_PG=0` для запуску проти розробницького PG.

---

## 3. Розміщення бінарників PostgreSQL

### 3.1. Джерело та slim-набір

**Джерело:** `postgresql-17.6-1-windows-x64-binaries.zip` (EDB, zip-збірка без інсталятора — саме те, що треба для embedding). Розпаковується в `frontend/src-tauri/resources/pg/`.

**Повний розпакований zip ≈ 1.2 GB — потрібен slim-набір.** Мінімальний склад (валідувати на Windows, див. §3.3):

```
resources/pg/
├── bin/
│   ├── initdb.exe          # ініціалізація pgdata
│   ├── pg_ctl.exe          # start/stop/status
│   ├── postgres.exe        # сам сервер
│   ├── pg_isready.exe      # перевірка готовності
│   ├── psql.exe            # create_db, діагностика
│   ├── libpq.dll           # клієнтська бібліотека (для psql/initdb)
│   ├── libssl-3-x64.dll    # TLS (потрібен навіть без TLS — лінкується)
│   ├── libcrypto-3-x64.dll
│   ├── libintl-9.dll
│   ├── libicu*.dll         # icudt/icuuc/icuin (якщо initdb скомпільований з ICU)
│   ├── libxml2.dll         # (якщо є залежність)
│   ├── liblz4.dll / libzstd.dll / zlib1.dll
│   └── vcruntime140.dll / vcruntime140_1.dll / msvcp140.dll  # VC++ runtime (якщо не системний)
├── lib/
│   └── (DLL, що в EDB-збірці лежать у lib/ — перевірити структуру zip)
└── share/
    ├── postgresql.conf.sample   # initdb копіює звідси базовий конфіг
    └── timezone/                # обов'язково для initdb
```

**Чого НЕ включати:** `pgAdmin*`, `stackbuilder*`, `psql.exe`-alternatives, `pg_upgrade`, `pg_dump` (опційно залишити pg_dump/pg_restore для бекапів — рішення на етапі 3), `doc/`, `include/`, `pgxs/`, більшість `share/extension/` (залишити лише ті, що потрібні схемі — перевірити `CREATE EXTENSION` у schema.sql: зазвичай `plpgsql` вбудований).

### 3.2. Куди класти і як Rust знаходить у рантаймі

**Бандлінг (Tauri bundle resources)** — `tauri.conf.json`:
```jsonc
"bundle": {
  "resources": {
    "resources/pg/bin/*.exe":   "pg/bin/",
    "resources/pg/bin/*.dll":   "pg/bin/",
    "resources/pg/lib/*":       "pg/lib/",
    "resources/pg/share/**/*":  "pg/share/"
  }
}
```
Результат: у встановленому застосунку файли лежать у `<install_dir>/resources/pg/...`.

**Рантайм-пошук (НЕ `env!("CARGO_MANIFEST_DIR")` — це compile-time шлях розробника, у продакшені не існує):**

```rust
// torgashka-infrastructure/src/pg/config.rs
pub fn resolve_pg_bin_dir(app: Option<&tauri::AppHandle>) -> Result<PathBuf, PGError> {
    if let Ok(p) = std::env::var("TORGASHKA_PG_BIN") {   // override для тестів/dev
        return Ok(PathBuf::from(p));
    }
    #[cfg(debug_assertions)]
    {
        // dev: системний PG на Linux / поруч із репо на Windows
        return Ok(PathBuf::from("/usr/lib/postgresql/17/bin"));
    }
    #[cfg(not(debug_assertions))]
    {
        // release: ресурси Tauri
        let res = app.expect("AppHandle у release-режимі")
            .path().resource_dir()?;              // <install_dir>/resources
        Ok(res.join("pg").join("bin"))
    }
}
```

Правила:
1. **Release:** `resource_dir()/pg/bin` — єдиний правильний шлях у встановленому застосунку.
2. **Debug/тести:** env `TORGASHKA_PG_BIN` → інакше системний `/usr/lib/postgresql/17/bin` на Linux (той самий 17.6).
3. Усі виклики `initdb/pg_ctl/postgres/psql/pg_isready` — через абсолютні шляхи з `bin_dir` (НЕ через `PATH`).

### 3.3. Валідація slim-набору (Windows)

Метод: **видаляй, поки не зламається** — на Windows-машині/раннері:
1. Розпакувати повний zip у тимчасовий каталог → переконатися, що `initdb` + `pg_ctl start` + `psql -c "SELECT version()"` працюють.
2. Поетапно видаляти непотрібні файли (список з §3.1) → після кожного кроку повторювати smoke: initdb → start → SELECT 1.
3. Перевірити, які DLL реально завантажуються (ProcMon або помилки "The code execution cannot proceed..."), додати відсутні.
4. Зафіксувати фінальний slim-набір у `resources/pg/` і розмір (ціль: **≤ 200 MB розпакованого, ~80-120 MB у NSIS**).

---

## 4. Життєвий цикл PG

### 4.1. Запуск (idempotent)

```
ensure_running():
  pgdata існує? ── ні ──→ initdb (таймаут 60 с, лог у stderr + postgres.log)
       │ так
       ▼
  pg_isready -p 5433? ── так ──→ (вже запущено інстансом; перевірити, що це НАШ: спроба auth DB_USER)
       │ ні
       ▼
  pg_ctl -D <pgdata> -l <pgdata>/postgres.log start
  wait_ready(port, 30_000)
  → create_db (якщо треба) → DSN
```

**Ідемпотентність гарантована:** повторний виклик `ensure_running()` на працюючому PG нічого не перезапускає (pg_isready = OK → повертає DSN).

### 4.2. Зупинка (cleanup при виході)

У Tauri `src/lib.rs` — `Builder::build().run()` з обробником:

```rust
.run(|app_handle, event| match event {
    RunEvent::Exit => {
        // коректна зупинка: pg_ctl stop -m fast (швидше, ніж kill, і без risk corruption)
        let _ = PostgresManager::from_env().and_then(|pg| pg.shutdown());
    }
    _ => {}
})
```

- `shutdown()` = `pg_ctl -D <pgdata> -m fast stop` (таймаут 15 с → потім `postgres.exe` kill по PID з `<pgdata>/postmaster.pid`).
- Захист від panic/аварійного завершення: PG сам переживає `kill` (WAL), при наступному старті `pg_ctl start` підніме після crash-recovery. Тому жорсткий `child.kill()` у Drop — допустимий fallback, але штатний шлях — `pg_ctl stop`.

### 4.3. Логування

| Канал | Що пише |
|---|---|
| `<pgdata>/postgres.log` | серверні логи PG (`pg_ctl -l`) |
| stderr застосунку | `[torgashka-pg] ...` — initdb прогрес, старт/стоп, помилки (стиль, узгоджений з існуючим `eprintln!` у src/lib.rs) |
| `PostgresManager::status()` | для команди `pg-status` (діагностика через Tauri invoke, опційно) |

`postgres.log` — ротація не потрібна на першому етапі (POS-лог невеликий); при перевищенні 50 MB — обнуляти при старті.

---

## 5. Ризики та мітігації

| # | Ризик | Ймовірність | Мітігація |
|---|---|---|---|
| 5.1 | **Порт 5433 зайнятий** іншим PG/сервісом | середня | `ensure_running()`: pg_isready → якщо відповідає НАШ (auth з DB_USER/DB_PASSWORD успішний) — використовувати; інакше fallback 5434…5440 (перший вільний), DSN з фактичним портом. Діапазон зафіксовано: **5433-5440** |
| 5.2 | **Firewall** блокує PG | низька | `listen_addresses='127.0.0.1'` — loopback не блокується Windows Firewall. Правило НЕ потрібне. Якщо колись зміниться біндинг — додати правило (див. windows-build-plan.md §4.2) |
| 5.3 | **Антивірус** фолз-позитив на postgres.exe/initdb.exe | середня | Код-сігнінг інсталятора (див. windows-build-plan.md §6.3); мінімізація набору файлів; тест на Defender; при потребі — submit до Microsoft |
| 5.4 | **Права на %APPDATA%** | низька | pgdata у `app_data_dir()` = `%APPDATA%\com.torgashka.pos\pgdata` — завжди writable для поточного юзера. НЕ класти pgdata поруч із бінарником (Program Files — read-only) |
| 5.5 | **Перший запуск повільний** (initdb 10-20 с на Windows) | гарантовано | UX: екран «Ініціалізація БД…» (спінер) — Rust-команда `init-embedded-pg` викликається через Tauri invoke, фронтенд показує прогрес; НЕ блокувати головне вікно модально без індикатора. Прогрес-події: `pg-init-progress` (emit у фронтенд) |
| 5.6 | **Конфлікт зі встановленим PG** (розробник має свій на 5432) | низька | Порт 5433 відмінний від 5432; `TORGASHKA_EMBEDDED_PG=0` для dev-режиму |
| 5.7 | **Втрата даних при оновленні застосунку** | середня | pgdata поза інсталятором (%APPDATA%) — оновлення/перевстановлення не чіпає дані. Тест: оновлення v1.0.0→v1.0.1 зі збереженням даних (чекліст §9, тест W5) |
| 5.8 | **Коректність slim-набору** (забули DLL) | середня | Валідація §3.3 на windows-latest раннері в CI; smoke-тест: initdb→start→SELECT 1 на чистій Windows |
| 5.9 | **Panic при exit** → PG не зупинено | низька | `pg_ctl stop` у `RunEvent::Exit` + fallback kill по postmaster.pid; crash-recovery PG на наступному старті |
| 5.10 | **ICU/locale проблеми з кирилицею** | низька | `-E UTF8 --locale=C` (фіксовано в §1.3); валідація: вставка/читання кириличних назв у тесті (включно з назвою товару, §7) |
| 5.11 | **Розмір інсталятора** (330MB zip → NSIS) | гарантовано | Slim-набір (ціль ≤200MB) + NSIS solid compression (LZMA) → ~80-120MB. Прийнятно для POS-термінала |
| 5.12 | **Створення товару не працює після встановлення** (регресія ключового сценарію) | низька | Обов'язковий тест W9 (§7.2): форма створення → POST /api/v1/products → товар у списку; включено в критерії прийняття Етапу 2 та 5 |

---

## 6. Етапи реалізації з критеріями прийняття

### Етап 0 — Підготовка артефактів PG (Tauri_Agent, паралельно з Етапом 1)
**Задача:** завантажити/розпакувати Windows zip, сформувати slim-набір.
- [ ] `postgresql-17.6-1-windows-x64-binaries.zip` завантажено, розпаковано
- [ ] Slim-набір у `frontend/src-tauri/resources/pg/` зібрано за §3.1
- [ ] (Linux) **Неможливо** валідувати Windows-бінарники на Linux — валідація на Windows/раннері (Етап 3)
- **Критерій прийняття:** структура `resources/pg/{bin,lib,share}` існує, розмір ≤ 200 MB, файли зафіксовані в git (або окремий LFS/архів — рішення на етапі; gitignore-виключення якщо великі)

### Етап 1 — PostgresManager core (Rust_Agent, Linux-first)
**Задача:** модуль `pg/` у `torgashka-infrastructure`, робота зі **системним** PG 17.6 на Linux.
- [ ] `pg::{manager,config,process}` реалізовано; `from_env()` читає `backend/.env`
- [ ] Unit-тести: `initdb` у тимчасовий каталог → `start` → `create_db` → `stop` → повторний `start` (ідемпотентність)
- [ ] `TORGASHKA_PG_BIN` env працює (override шляху до бінарників)
- **Критерій:** `cargo test -p torgashka-infrastructure pg::` — зелений на Linux; цикл init→start→query→stop→start проходить без ручного втручання

### Етап 2 — Інтеграція в Tauri startup (Rust_Agent)
**Задача:** запуск PG перед `serve_listener`, зупинка при exit.
- [ ] `src/lib.rs`: `ensure_running()` → `set_var("DATABASE_URL", dsn)` перед serve_listener
- [ ] `RunEvent::Exit` → `shutdown()`
- [ ] Env-флаг `TORGASHKA_EMBEDDED_PG=0` для dev-режиму
- [ ] Логування `[torgashka-pg]` у stderr
- **Критерій:** на Linux: `npm run tauri:dev` (або release-бінарник) → API на 8000 відповідає, `users-list` працює, **створення товару через `POST /api/v1/products` успішне (товар з'являється у `GET /api/v1/products`)**, дані збережено після рестарту застосунку, при exit PG зупинено (`pg_isready` = no response)

### Етап 3 — Windows slim-набір + bundle resources (Tauri_Agent)
**Задача:** бандлінг PG в інсталятор, рантайм-пошук бінарників.
- [ ] `tauri.conf.json`: `bundle.resources` за §3.2
- [ ] `resolve_pg_bin_dir()`: release → `resource_dir()/pg/bin`
- [ ] Smoke-тест на `windows-latest` runner: `tauri build --bundles nsis` → встановити → initdb→start→SELECT 1
- **Критерій:** інсталятор містить `resources/pg/bin/postgres.exe`; на чистій Windows (без встановленого PG) застосунок піднімає БД; розмір NSIS у межах ~80-120 MB

### Етап 4 — UX та edge-cases (Rust_Agent + React_UI_UX_Agent)
**Задача:** індикатор ініціалізації, конфлікт порту, прогрес.
- [ ] Tauri command `init-embedded-pg` + події `pg-init-progress` (emit)
- [ ] Фронтенд: екран «Ініціалізація БД…» зі спінером і відсотками (initdb → start → schema)
- [ ] Fallback порту 5434…5440 з логуванням фактичного порту
- **Критерій:** на холодному старті (порожній %APPDATA%) користувач бачить індикатор, а не «зависле» вікно; час від запуску до робочого API ≤ 60 с

### Етап 5 — Валідація та приймальні тести (QA_Agent + Rust_Agent)
**Задача:** повний тест-план §7 на Linux і Windows, **включно з ключовим сценарієм «створення товару» (POST /api/v1/products, W9) та users-list (L3, W10)**.
- **Критерій:** усі пункти тест-плану пройдено; результати задокументовано в `docs/embedded-pg-test-report.md`

---

## 7. Тест-план

### 7.1. Linux (той самий код PostgresManager, системний PG 17.6)

| # | Тест | Команда / крок | Очікування |
|---|---|---|---|
| L1 | init → start → query | `cargo test -p torgashka-infrastructure pg::` | зелений |
| L2 | Ручний запуск фасаду | `TORGASHKA_EMBEDDED_PG=1 npm run tauri:dev` (або release bin) | API :8000 відповідає; `pg_isready -p 5433` = accepting |
| L3 | users-list | `curl http://127.0.0.1:8000/api/v1/users` (або через UI) | 200 + дані |
| L4 | Ідемпотентність | рестарт застосунку без зупинки PG вручну | PG не переініціалізується, дані на місці |
| L5 | Зупинка при exit | закрити застосунок → `pg_isready -p 5433` | no response |
| L6 | Кирилиця | INSERT/UPDATE назви з кирилицею через API | збереження/читання коректне |
| L7 | Порт зайнятий | підняти dummy на 5433 → запуск | fallback 5434, API працює |
| L8 | `TORGASHKA_EMBEDDED_PG=0` | запуск з флагом | використовується зовнішній PG (5432), embedded не стартує |
| L9 | **Створення товару (ключовий сценарій, API)** | `curl -X POST http://127.0.0.1:8000/api/v1/products -H "Content-Type: application/json" -H "Authorization: Bearer <token>" -d '{"name":"Тест-товар","price":100,"quantity":10}'` → `curl http://127.0.0.1:8000/api/v1/products` | 201/200; товар присутній у списку; назва кирилицею збережена коректно |
| L10 | Створення товару після рестарту | створити товар (L9) → рестарт застосунку → `GET /api/v1/products` | товар на місці (дані в embedded pgdata) |

### 7.2. Windows (windows-latest runner або нативна VM)

| # | Тест | Крок | Очікування |
|---|---|---|---|
| W1 | Slim-набір | smoke: initdb → pg_ctl start → `psql -c "SELECT version()"` | 17.6, без помилок DLL |
| W2 | Інсталяція | встановити NSIS .exe на чисту Windows 10/11 | успішно, `resources/pg/` присутній |
| W3 | Перший запуск | запуск → індикатор «Ініціалізація БД…» | ≤ 60 с до робочого API |
| W4 | Дані після рестарту | створити записи → перезапуск | дані збережено (`%APPDATA%\com.torgashka.pos\pgdata`) |
| W5 | Оновлення застосунку | v1.0.0 → v1.0.1 через updater | дані збережено, PG піднято знову |
| W6 | Порт 5433 зайнятий | сторонній сервіс на 5433 | fallback 5434, API працює |
| W7 | Антивірус | Windows Defender Full Scan після встановлення | без quarantine; при фолз-позитиві — submit |
| W8 | Відсутній WebView2/чиста VM | встановлення на мінімальну VM | bootstrapper WebView2 спрацював (перетин з windows-build-plan) |
| W9 | **Створення товару (ключовий сценарій, UI)** | відкрити форму створення товару → заповнити назву (кирилицею), ціну, кількість → «Зберегти» → відкрити список товарів | товар з'явився у списку; після перезапуску застосунку товар на місці (перевірка embedded PG) |
| W10 | users-list | відкрити екран користувачів (або `GET /api/v1/users`) | список відображається з даними embedded PG |

---

## 8. Контракти для агентів

### Контракт 1: Rust_Agent

```
ЗАДАЧА:           Реалізувати embedded PostgreSQL: модуль PostgresManager
                  у crates/torgashka-infrastructure/src/pg/ та інтеграцію
                  запуску/зупинки у frontend/src-tauri/src/lib.rs
ВХІД:             crates/torgashka-infrastructure/src/db.rs (resolve_database_url,
                  ensure_schema — НЕ змінювати), crates/torgashka-api/src/lib.rs
                  (serve_listener), frontend/src-tauri/src/lib.rs (точка запуску
                  фасаду ~рядки 247-267, обробник RunEvent::Exit),
                  backend/.env (DB_USER/DB_PASSWORD/DB_NAME/DB_HOST/DB_PORT),
                  системний PostgreSQL 17.6 на Linux (/usr/lib/postgresql/17/bin)
                  для тестів
ВИХІД:            crates/torgashka-infrastructure/src/pg/{mod.rs,manager.rs,
                  config.rs,process.rs}; зміни в frontend/src-tauri/src/lib.rs;
                  unit-тести (crates/torgashka-infrastructure/tests/pg_*.rs);
                  env-флаг TORGASHKA_EMBEDDED_PG; документація модуля (rustdoc)
КРИТЕРІЙ:         (1) cargo test -p torgashka-infrastructure pg:: — зелений
                  на Linux; (2) цикл init→start→create_db→query→stop→start
                  ідемпотентний; (3) на Linux з TORGASHKA_EMBEDDED_PG=1 API
                  :8000 відповідає і users-list працює; (4) створення товару
                  через POST /api/v1/products успішне — товар з'являється
                  у GET /api/v1/products (ключовий сценарій); (5) при виході
                  застосунку pg_isready -p 5433 = no response; (6) db.rs
                  не змінено (інтеграція тільки через env DATABASE_URL)
ДЕДЛАЙН:          Етапи 1-2 до кінця поточного спринту
КАНАЛ АНОМАЛІЙ:   PM_Agent — якщо ensure_schema вимагає зміни (наприклад,
                  CREATE EXTENSION з відсутнім extension-файлом у slim-наборі)
```

### Контракт 2: Tauri_Agent

```
ЗАДАЧА:           Забандлити slim PostgreSQL 17.6 (Windows x64) у Tauri
                  bundle та налаштувати інсталятор NSIS
ВХІД:             postgresql-17.6-1-windows-x64-binaries.zip (завантажений),
                  slim-набір за §3.1 у frontend/src-tauri/resources/pg/,
                  frontend/src-tauri/tauri.conf.json
ВИХІД:            tauri.conf.json: bundle.resources (pg/bin,pg/lib,pg/share)
                  + налаштування NSIS (compression, solid); оновлений
                  windows-build workflow (windows-build.yml) з кроком
                  валідації slim-набору (initdb→start→SELECT 1);
                  інструкція рантайм-шляху resolve_pg_bin_dir (resource_dir)
КРИТЕРІЙ:         (1) tauri build --bundles nsis на windows-latest проходить;
                  (2) встановлений застосунок містить resources/pg/bin/
                  postgres.exe та всі DLL (smoke: initdb→start→SELECT 1);
                  (3) розмір NSIS ~80-120 MB; (4) чиста Windows без PG —
                  застосунок піднімає embedded БД
ДЕДЛАЙН:          Етапи 0, 3 (паралельно з Rust_Agent Етап 1)
КАНАЛ АНОМАЛІЙ:   PM_Agent — якщо EDB zip має іншу структуру каталогів,
                  ніж очікувано (bin/lib/share), або розмір перевищує бюджет
```

### Контракт 3 (пізніше, після Етапу 2): React_UI_UX_Agent

```
ЗАДАЧА:           Екран «Ініціалізація БД…» з індикатором прогресу
ВХІД:             Tauri commands/events: invoke('init_embedded_pg'),
                  listen('pg-init-progress', {stage, percent})
ВИХІД:            компонент у frontend/src (екран очікування при холодному
                  старті: initdb → start → schema; повідомлення про помилку
                  з кнопкою «Повторити»)
КРИТЕРІЙ:         на порожньому %APPDATA% користувач бачить прогрес,
                  а не порожнє вікно; при помилці — зрозуміле повідомлення;
                  після готовності БД форма створення товару працює
                  (ключовий сценарій W9)
```

---

## 9. Чекліст перед релізом Windows-збірки з embedded PG

- [ ] `resources/pg/` slim-набір зафіксовано, розмір ≤ 200 MB (W1 пройдено)
- [ ] `tauri.conf.json` resources налаштовано; NSIS збирається (W2)
- [ ] Перший запуск ≤ 60 с з індикатором (W3)
- [ ] Дані зберігаються між рестартами і між оновленнями застосунку (W4, W5)
- [ ] Fallback порту 5434-5440 працює (W6)
- [ ] Антивірус: Defender не карантинить (W7)
- [ ] **Ключовий сценарій: створення товару в UI (форма → збереження → товар у списку) пройдено на Windows (W9)**
- [ ] **users-list відображається з даними embedded PG (W10)**
- [ ] Updater endpoints збігаються з remote (з windows-build-plan.md §6.1)
- [ ] Код-сігнінг: підписано або задокументовано сценарій SmartScreen
- [ ] Тест-план §7: L1-L10, W1-W10 пройдено, звіт у docs/embedded-pg-test-report.md
- [ ] Документація: README розділ «Встановлення на Windows» (що PG вбудований, де дані)
- [ ] Ретроспектива: фактичний розмір інсталятора, час першого запуску, фолз-позитиви антивірусів

---

## Додаток A. Послідовність запуску (Windows, перший холодний старт)

```
Torgashka.exe (Rust, Tauri)
├── 1. PostgresManager::from_env()        → читає backend/.env (запакований у ресурси
│                                            або згенерований конфіг) → bin_dir=resource_dir/pg/bin
├── 2. ensure_running():
│       ├── pgdata немає → initdb -D %APPDATA%\com.torgashka.pos\pgdata
│       │                  -U <DB_USER> -E UTF8 --locale=C -A scram-sha-256 --pwfile=<tmp>
│       ├── патч postgresql.conf: port=5433, listen_addresses='127.0.0.1'
│       ├── pg_ctl -D ... -l postgres.log start → wait pg_isready (до 30 с)
│       ├── create_db <DB_NAME> (якщо немає)
│       └── повертає DSN postgresql://user:pass@127.0.0.1:5433/db
├── 3. set_var("DATABASE_URL", dsn)
├── 4. serve_listener(listener)  → connect_readonly_pool → ensure_schema (34 таблиці)
│                                    → роути змонтовано (включно POST /api/v1/products,
│                                      GET /api/v1/users) → API :8000 готовий
├── 4a. Користувач: форма створення товару → POST /api/v1/products
│        → товар у GET /api/v1/products (список) ✅ КЛЮЧОВИЙ СЦЕНАРІЙ
└── 5. RunEvent::Exit → pg_ctl stop -m fast
```
