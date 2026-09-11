# Rust_Agent — Звіт: ЕТАП 13 (SQLCipher SPIKE), ЕТАП 15.1 (offline-міграції), ЕТАП 15.2 (RBAC/OAuth)

Дата: 2026-09-02 · Гілка: `feat/rust-migration` · Виконавець: Rust_Agent (делегування: Dev_Agent)

---

## 1. ЕТАП 15.1 — Версійовані міграції offline.db (sqlx-альтернатива, stability_first)

### Рішення
Поточна схема офлайн-БД — на **rusqlite** (SQLite, WAL), перехід на `sqlx::migrate!`
вимагав би повної заміни драйвера та `links="sqlite3"`-конфлікту зі spike-крейтом.
За принципом stability_first обрано **версійований механізм через `PRAGMA user_version`**
з SQL-скриптами у `migrations/offline/*.sql` — те, що дозволяє контракт задачі.

### Що зроблено
| Файл | Зміна |
|---|---|
| `crates/torgashka-infrastructure/migrations/offline/0001_initial.sql` | **новий**: базова схема (products/receipts/settings + індекси, весь DDL — `IF NOT EXISTS`) |
| `crates/torgashka-infrastructure/src/offline/db.rs` | `initialize_tables()` → `apply_migrations()`: раннер читає `PRAGMA user_version`, застосовує pending-міграції (SQL + legacy-хуки `ensure_column`), виставляє `user_version`; `SCHEMA_VERSION = 1`, реєстр `MIGRATIONS` через `include_str!` |
| `ensure_column()` | тепер пропускає неіснуючі таблиці (свіжа БД) — дозволяє запускати хуки ДО SQL-скрипта, щоб індекси на `store_id` знаходили колонки в legacy-БД |

### Як працює
1. `PRAGMA user_version` = 0 (свіжа/legacy) → застосувати міграцію 0001 + legacy-ALTERs → `user_version = 1`.
2. Повторний старт: `user_version >= SCHEMA_VERSION` → no-op (ідемпотентність).
3. Майбутні зміни: новий `0002_*.sql` + `SCHEMA_VERSION = 2` (раннер застосує лише pending).

### Тести (4 unit-тести в `offline/db.rs`, всі green)
- `fresh_db_gets_schema_v1` — свіжа БД: таблиці + колонки + `user_version=1`, схема робоча.
- `reopen_is_idempotent` — повторний старт: no-op, дані цілі.
- `legacy_db_upgraded_with_columns` — БД до Етапу 5/8.1 (без store_id/client_receipt_uuid): мігрується, дані не втрачені, індекси створені.
- `reset_version_rerun_is_safe` — скинутий `user_version` на мігрованій БД: повторна міграція безпечна.

Існуючі інтеграційні тести `offline_store_id.rs` (6) та `stage5_tauri.rs` (2) — green (зворотна сумісність з legacy-БД збережена).

---

## 2. ЕТАП 15.2 — OAuth/RBAC

### Стан до роботи (Etap 6, commit 000d238)
- JWT HS256 з claims `{sub, role, permissions, type, iat, exp}` — вже був.
- Ролі в Rust: **тільки admin|cashier**; Python-домен мав owner/viewer, але Rust `UserRole::parse` їх не знав → **owner деградував до cashier-permissions** (баг).
- `require_admin` приймав admin|owner; таблиці roles не було.

### Що зроблено (Rust-шар)
| Файл | Зміна |
|---|---|
| `crates/torgashka-domain/src/auth.rs` | `UserRole` → **Owner, Admin, Cashier, Viewer**; `as_str`/`parse` для всіх 4; `VIEWER_PERMISSIONS` (read-only, 1:1 Python `VIEWER={"read","reports"}`); `default_permissions`: Owner/Admin → ALL, Cashier → CASHIER, Viewer → VIEWER |
| `crates/torgashka-api/src/auth_routes.rs` | `parse_role`: приймає owner/viewer (422-повідомлення оновлено); дефолт при створенні юзера без ролі — cashier (Python-parity) |
| `crates/torgashka-api/src/auth.rs` | Нова `viewer_write_blocked(role, method)`: viewer → 403 на POST/PUT/PATCH/DELETE бізнес-роутів (read-only); meta-роути лишаються під `require_admin` (viewer → 403 на admin), `/auth/refresh` (POST) не блокується — viewer може оновлювати токен |

### Міграція БД (делеговано DB_Admin_Agent, зроблено)
`backend/alembic/versions/0010_roles_rbac.py` (revision `0010_roles_rbac`, down_revision `0009_transfers_store_fk`):
- `ALTER TYPE user_role ADD VALUE IF NOT EXISTS 'viewer'`;
- `roles` (id, code UNIQUE, name, description, created_at) + seed owner/admin/cashier/viewer;
- `users.role_id INTEGER NULL REFERENCES roles(id)` — nullable, колонка `role` НЕ чіпається (сумісність Rust/Python parity-коду);
- backfill `role_id` з `users.role` (через `::text`);
- downgrade: DROP FK/COLUMN/TABLE (enum-значення не видаляються — PG-обмеження, як у 0002a).

**`alembic heads` → рівно одна голова: `0010_roles_rbac`.** ✅

### OAuth-рішення (задокументовано)
- **OAuth2-провайдера немає** (немає публічного IdP, немає вимог плану до конкретного провайдера; `auth_routes.rs` не містить PKCE-флоу — лише basic login/login-pin).
- **Рішення: basic-auth + RBAC зараз, OAuth2 — v-next.** Обґрунтування: (1) система on-premise/офлайн-first, OAuth2 вимагає зовнішнього сервісу токенів — суперечить архітектурі (офлайн-каса не може покладатись на IdP); (2) JWT вже крос-валідний Rust↔Python з role-claim — RBAC-потреба закрита; (3) додавання PKCE без провайдера = мертвий код (zero_bloat).

### Тести (всі green)
- domain `role_tests` (6): parse-раундтріп усіх 4 ролей; owner/admin → ALL; viewer — read-only, підмножина ALL; cashier незмінний.
- api `viewer_write_blocked_rules` + `owner_write_blocked_rules` (в auth.rs, 11 тестів auth).
- Повний `cargo test -p torgashka-api`: 24 lib + onboarding_e2e + cash_operations_e2e + prro_facade(5) — **0 failed** (auth-тести не зламані).

---

## 3. ЕТАП 13 — SPIKE SQLCipher (реальний код, реальна збірка)

### Результат: **SQLCipher ЖИТТЄЗДАТНИЙ** для offline.db

### Докази (реальна збірка, не імітація)
Збірка: `rusqlite 0.32` + feature **`bundled-sqlcipher-vendored-openssl`** (cmake/perl/make/gcc/openssl — усі присутні в середовищі; збірка 1m58s, успішна).

```
[OK] SQLCipher активовано: cipher_version = 4.5.7 community
[OK] journal_mode = wal (очікувалось wal)
[OK] запис/читання під ключем: products=1
[OK] без ключа дані не читаються: file is not a database
[OK] міграція plain→encrypted (ATTACH + sqlcipher_export): receipts=1

ВИСНОВОК SPIKE: SQLCipher життєздатний для offline.db
```

### Перевірки за контрактом
1. **Збірка**: так, збирається в цьому оточенні (bundled-sqlcipher-vendored-openssl). Помилок збірки немає.
2. **Відкриття незашифрованої існуючої БД після включення SQLCipher**: міграція через `ATTACH ... KEY + sqlcipher_export` — працює (стандартний шлях SQLCipher; дані переносяться, читаються під ключем).
3. **WAL/PRAGMA-сумісність**: `journal_mode=wal` увімкнувся під SQLCipher — сумісність підтверджена.
4. **Критерій ЕТАП 13**: файл БЕЗ ключа → `file is not a database` (не читається). ✅
5. **key_check**: на community-збірці 4.5.7 повертає no rows (не реалізований як row-pragma) — НЕ критично: функціональний тест (без ключа не читається / з ключем читається) є визначальним.

### Артефакти
- `crates/torgashka-infrastructure/examples/sqlcipher_spike.rs` — **існує**, компілюється на stock-збірці (діагностика: «offline.db ЗАРАЗ НЕ ЗАШИФРОВАНА»); під sqlcipher-збіркою виконує повний набір перевірок.
- `crates/sqlcipher-spike/` (excluded з workspace, `links="sqlite3"` конфлікт) — ізольований відтворюваний крейт: `cargo run --example sqlcipher_spike` → повний успіх (підтверджено реальним запуском).

### Де зберігати ключ — рекомендація
**`config.toml` (chmod 0600)** — для Torgashka, а не системний keyring:
- офлайн-каса = headless-пристрій, keyring (gnome-keyring/kwallet) вимагає графічної сесії та розблокованого брелока — на автозапуску каси ключ буде недоступний;
- 0600-файл поруч із конфігом: простий, детермінований, бекапиться разом із системою;
- keyring — опція для майбутнього (якщо з'явиться десктопна сесія з брелоком), але не блокер.

### План мінімальної реалізації (v-next, якщо Творець підтвердить)
1. Винести SQLCipher-збірку rusqlite в окремий крейт (або фічу `sqlcipher` у torgashka-infrastructure — перевірити `links`-конфлікт);
2. `PRAGMA key` при відкритті + генерація ключа (32B, `getrandom`) при першому запуску → `config.toml` 0600;
3. Міграція існуючих offline.db: `ATTACH + sqlcipher_export` (одноразовий раннер на старті);
4. `PRAGMA cipher_memory_security = ON` (обнулення ключа в пам'яті);
5. QA: `sqlite3 offline.db` без ключа → garbage/error; `cargo test` зелений.

**Ризик реалізації — низький**: spike довів збірку, WAL, DML і міграцію. Продакшн-код `offline/` НЕ змінювався (контракт ЕТАП 13 дотримано).

---

## 4. Верифікація (критерії прийняття)

| Критерій | Статус | Доказ |
|---|---|---|
| `cargo build --workspace` | ✅ | `Finished dev profile in 28.91s` (з усіма змінами) |
| `cargo test --workspace` | ✅ | **282 passed, 0 failed** (повний workspace) |
| `alembic heads` → одна голова | ✅ | `0010_roles_rbac (head)` |
| JWT містить role claim | ✅ | існував (`Claims.role`), тепер парсить owner/viewer; owner отримує ALL замість cashier-деградації |
| cashier не викликає admin-ендпоінти (403) | ✅ | `require_admin` (admin|owner), тести green |
| offline-міграції ідемпотентно при старті | ✅ | 4 unit-тести + legacy-compat тести |
| Тести auth не зламані | ✅ | api lib 24/24, E2E onboarding/cash/prro green, workspace 282/0 |
| OAuth2 без провайдера | ✅ задокументовано | basic+RBAC зараз, OAuth — v-next (обґрунтування вище) |

## 5. Аномалії / нотатки
- `PRAGMA key_check` на SQLCipher community 4.5.7 повертає no rows — зафіксовано як особливість збірки, функціональні перевірки визначальні.
- У середовищі раніше спостерігався дефіцит диску (ENOSPC) — під час фінальної верифікації диск звільнився (16–23G), повний workspace-прогін пройшов.
- `crates/sqlcipher-spike/` створено попередньою (перерваною) спробою ЕТАП 13 — синхронізовано з фінальною версією spike, excluded з workspace.
