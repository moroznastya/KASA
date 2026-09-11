# Аудит витоків сирого тексту БД у тіла HTTP-відповідей

- **Дата:** 2026-09-12
- **Виконавець:** QA_Agent (аудит лише читав код; `.rs` не редагувалися; `docs/adr/**` не чіпалися)
- **Корінь:** `/home/anastasia/Andriy/aegis_v3/Niko/Projects/Torgashka/frontend/src-tauri`
- **Обсяг:** усі `crates/torgashka-api/src/*.rs` (27 `impl IntoResponse` + `api_err` + `#[from]`-enum'и)

## Метод (машинно-підтверджений)

1. Зібрано **всі** блоки `impl IntoResponse` (скрипт із балансуванням `{}`) → карта `варіант помилки → тіло JSON`.
2. Прогнано 3 обов'язкові джерела (точні команди — «Джерела» нижче), кожен кандидат простежено до місця запису в тіло.
3. Пройдено ланцюг «сирий текст» → `IntoResponse`: `sqlx::Error`/`PrroRepoError`/`DbError` → `X::Infrastructure(e.to_string())` → arm `IntoResponse` → `{"detail": …}`.
4. Для `readonly_net` перевірено **обидві** сигнатури та умови втручання (`crates/torgashka-api/src/readonly_net.rs:82-205`, `crates/torgashka-infrastructure/src/readonly_guard.rs:43`).
5. Класифікація: **A** — сирий текст PG/SQLx потрапляє в тіло (дефект); **B** — текст іде лише в stderr/журнал, у тілі нейтральний literal; **C** — у тіло людський текст, причина — у лог (санація/`pg_log`).

## Таблиця знахідок

| файл:рядок | тип помилки | як конвертується (IntoResponse/api_err) | у тіло? | клас | доказ (фрагмент коду) |
|---|---|---|---|---|---|
| `categories_v2.rs:82-86` | `WriteError` (catch-all `Service(e)`, ловить `Infrastructure`) | `(500, Json(json!({"detail": e.to_string()})))` | так | A | `CatV2Error::Service(e) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(serde_json::json!({"detail": e.to_string()})),\n)` |
| `crud.rs:138-140` | `DirectoryError::Infrastructure` | `(500, {"detail": format!("Помилка БД довідників: {msg}")})` | так | A | `ServiceError::Directory(torgashka_domain::DirectoryError::Infrastructure(msg)) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(serde_json::json!({"detail": format!("Помилка БД довідників: {msg}")})),\n)` |
| `readdirs.rs:104-107` | `DirectoryError::Infrastructure` | `(500, {"detail": format!("Помилка БД довідників: {msg}")})` | так | A | `DirectoryError::Infrastructure(msg),\n)) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(serde_json::json!({"detail": format!("Помилка БД довідників: {msg}")})),\n)` |
| `print_templates.rs:71-74` | `PrintError::Infrastructure` | `(500, {"detail": msg})` | так | A | `PrintErr::Service(PrintError::Infrastructure(msg)) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(json!({"detail": msg})),\n)` |
| `products_v2.rs:86-89` | `ProductsV2Error::Infrastructure` | `(500, {"detail": msg})` | так | A | `ProductsV2Error::Infrastructure(msg) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(json!({"detail": msg})),\n)` |
| `documents.rs:74-77` | `DocumentsError::Infrastructure` | `(500, {"detail": msg})` | так | A | `DocErr::Service(DocumentsError::Infrastructure(msg)) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(json!({"detail": msg})),\n)` |
| `setup.rs:86-91` | `SetupError::Infrastructure` | `(500, {"detail": msg})` | так | A | `SetupError::Infrastructure(msg) => {\n eprintln!("[torgashka-api] setup infrastructure error: {msg}");\n (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(serde_json::json!({"detail": msg})),\n)` |
| `suppliers.rs:78-81` (джерело `:43`) | `SupplierError::Internal` ← `other.to_string()` (`DirectoryError::Infrastructure`) | `(500, {"detail": msg})` | так | A | `other => SupplierError::Internal(other.to_string()),` → `SupplierError::Internal(msg) => (\n StatusCode::INTERNAL_SERVER_ERROR,\n Json(serde_json::json!({"detail": msg})),\n)` |
| `route_local.rs:202` (джерела `:216`,`:222`,`:228`,`:651`) | `LocalErr::Read` ← `DirectoryError/PosError/WriteError.to_string()`, `format!("… {e}")` | `(500, {"detail": m.clone()})` | так | A | `LocalErr::Read(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),` … `(code, Json(json!({"detail": msg}))).into_response()` |
| `promote.rs:257` | `LocalErr` (з `drain_local_outbox`) | `json!({ "error": e.to_string() })` → поле `outbox_drain.error` у **200** | так | A | `Err(e) => {\n eprintln!("[promote] увага: drain черги не виконано ({e}) …");\n json!({ "error": e.to_string() })\n }` → `Ok(Json(json!({ … "outbox_drain": drain … })))` |
| `sync.rs:472` + `:555` (джерела `:720`,`:840`,`:966`,`:804`,`:820`,`:922`) | `sqlx::Error` (`e.to_string()` / `e`) | `PushItemResult{status:"error", error: Some(msg)}` → `Json<Vec<PushItemResult>>` у **200** | так | A | `pub struct PushItemResult { … pub error: Option<String> }` … `let msg = e.to_string();` … `PushItemResult::error(item.client_uuid, e)` … `Ok(Json(results))` |
| `admin_db_sources.rs:632-634` (джерело `:378`,`:382`) | `sqlx::Error` з `ping_source` (`map_err(|e| e.to_string())`) | `DbSrcErr::BadRequest(format!("Джерело '{id}' недосяжне: {e}"))` → `{"detail": m}` у **400** | так | A | `.map_err(\|e\| e.to_string())?;` … `Err(e) => Err(DbSrcErr::BadRequest(format!(\n "Джерело '{id}' недосяжне: {e}"` |
| `admin_db_sources.rs:659-661` (джерело `:378`) | `sqlx::Error` з `ping_source` | `BadRequest(format!("Джерело '{id}' недосяжне ({e}); активним НЕ зроблено …"))` → **400** | так | A | `if let Err(e) = ping_source(&url).await {\n return Err(DbSrcErr::BadRequest(format!(\n "Джерело '{id}' недосяжне ({e}); активним НЕ зроблено — перевірте host/port/пароль"` |
| `admin_db_sources.rs:698-700` (джерело `:378`) | `sqlx::Error` з `ping_source` | `BadRequest(format!("Джерело '{source_id}' недосяжне ({e}); дамп не створено"))` → **400** | так | A | `return Err(DbSrcErr::BadRequest(format!(\n "Джерело '{source_id}' недосяжне ({e}); дамп не створено"` |
| `admin_db_sources.rs:814-818` (джерело `:378`) | `sqlx::Error` з `ping_source` | `BadRequest(format!("Джерело-приймач '{}' недосяжне ({e}); імпорт не виконано", …))` → **400** | так | A | `return Err(DbSrcErr::BadRequest(format!(\n "Джерело-приймач '{}' недосяжне ({e}); імпорт не виконано",\n body.source_id` |
| `admin_db_sources.rs:84-91` | `ProvisionError::{Connect{reason},PsqlFailed,StartFailed,Failed}` (несе psql/PG stderr) | `DbSrcErr::BadRequest(other.to_string())` → `{"detail": m}` у **400** | так | A | `other => DbSrcErr::BadRequest(other.to_string()),` ; `#[error("Не вдалося підключитись до {host}:{port} суперкористувачем: {reason}")]` (`provision.rs:61`) |
| `admin.rs:92-96` | `AdminErr::Db(sqlx::Error)` | `eprintln!` + `(500, {"detail": "Внутрішня помилка сервера"})` | ні | B | `eprintln!("[torgashka-api] admin: помилка БД: {e}");` + literal |
| `admin_audit.rs:70-74` | `AdminAuditErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] admin_audit: помилка БД: {e}");` + literal |
| `admin_db_sources.rs:103-108` | `DbSrcErr::Internal(String)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] db-sources: помилка: {m}");` + `"Внутрішня помилка сервера"` |
| `admin_migrate.rs:67-71` | `MigrateErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] admin_migrate: помилка БД: {e}");` + literal |
| `admin_network_config.rs:97-100` | `NetCfgErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] network-config: помилка БД: {e}");` + literal |
| `admin_network_config.rs:104-109` | `NetCfgErr::DbSources(DbSourcesError)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] network-config: db_sources: {e}");` + `"Конфігурація джерел даних недоступна"` |
| `admin_network_config.rs:111-116` | `NetCfgErr::Internal(String)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] network-config: помилка: {m}");` + literal |
| `admin_prro.rs:95-99` | `AdminPrroErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] admin_prro: помилка БД: {e}");` + literal |
| `admin_reports.rs:90-94` | `AdminReportsErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] admin_reports: помилка БД: {e}");` + literal |
| `network.rs:106-112` | `NetworkErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] network: помилка БД: {e}");` + literal |
| `network_nodes.rs:110-116` | `NodeErr::Db(sqlx::Error)` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] network_nodes: помилка БД: {e}");` + literal |
| `sync.rs:104-108` | `SyncError::Db(#[from] sqlx::Error)` | `eprintln!` + `{"detail": "помилка бази даних"}` 500 | ні | B | `eprintln!` + `Json(json!({"detail": "помилка бази даних"}))` |
| `stores.rs:63-68` | `StoreError::Infrastructure` | `eprintln!` + literal 500 | ні | B | `eprintln!("[torgashka-api] stores: {msg}");` + literal |
| `auth_routes.rs:79-84` | `AuthError::Infrastructure` | `eprintln!` + `(500, {"detail": "Помилка БД"})` | ні | C | `eprintln!("[torgashka-api] auth infrastructure error: {m}"); (StatusCode::INTERNAL_SERVER_ERROR, "Помилка БД".to_string())` |
| `crud.rs:106-119` | `WriteError::Infrastructure` | `pg_log(...)` + `(500, {"detail": "Не вдалося зберегти зміну, спробуйте ще раз"})` | ні | C | `torgashka_infrastructure::embedded_pg::pg_log("ERROR", &format!("[crud] WriteError::Infrastructure: {msg}"));` + human |
| `readdirs.rs:145-160` | `WriteError::Infrastructure` | `pg_log(...)` + human 500 | ні | C | `pg_log("ERROR", &format!("[readdirs] WriteError::Infrastructure: {msg}"));` + human |
| `pos.rs:104-121` | `PosError::Infrastructure` | `pg_log("...{msg}")` + human 500 | ні | C | `torgashka_infrastructure::embedded_pg::pg_log("ERROR", &format!("[pos] PosError::Infrastructure: {msg}"));` + human |
| `debtors.rs:79-92` | `DebtorError::Infrastructure` | `pg_log(...)` + human 500 | ні | C | `pg_log("ERROR", &format!("[debtors] DebtorError::Infrastructure: {msg}"));` + human |
| `invoices.rs:68-80` | `InvoicesError::Infrastructure` | `pg_log(...)` + human 500 | ні | C | `pg_log("ERROR", &format!("[invoices] InvoicesError::Infrastructure: {msg}"));` + human |
| `ledger.rs:122-137` | `LedgerError::Infrastructure` | `pg_log(...)` + human 500 | ні | C | `pg_log("ERROR", &format!("[ledger] LedgerError::Infrastructure: {msg}"));` + human |
| `purchase_orders.rs:61-72` | `PurchaseOrdersError::Infrastructure` | `pg_log(...)` + human 500 | ні | C | `pg_log("ERROR", &format!("[purchase_orders] PurchaseOrdersError::Infrastructure: {msg}"));` + human |
| `return_invoices.rs:56-67` | `ReturnInvoicesError::Infrastructure` | `pg_log(...)` + human 500 | ні | C | `pg_log("ERROR", &format!("[return_invoices] ReturnInvoicesError::Infrastructure: {msg}"));` + human |

### Джерела сирого тексту (усі класу A)

| джерело `e.to_string()` | тримач | споживачі (рядки таблиці) |
|---|---|---|
| `directories.rs:1068` `db_err(e) = DirectoryError::Infrastructure(e.to_string())` | `DirectoryError` | crud.rs:140, readdirs.rs:107, suppliers.rs:43, route_local.rs:216 |
| `write.rs:35` `WriteError::Infrastructure(e.to_string())` | `WriteError` (Display = `"помилка БД: {0}"`) | categories_v2.rs:85, route_local.rs:228 |
| `print_templates.rs:481/535/602` `PrintError::Infrastructure(e.to_string())` | `PrintError` | print_templates.rs:74 |
| `products_v2.rs:688` `ProductsV2Error::Infrastructure(e.to_string())` | `ProductsV2Error` | products_v2.rs:89 |
| `documents.rs:20` `de(e.to_string())` (+97/131/174/212/253) | `DocumentsError` | documents.rs:77 |
| `setup.rs:62/87/89/220` `SetupError::Infrastructure(e.to_string())` | `SetupError` | setup.rs:90 |
| `route_local.rs:651` `format!("репліка (stock {product_id}): {e}")`, `:216/:222/:228` `e.to_string()` | `LocalErr::Read` | route_local.rs:202 |
| `sync.rs:720/840/966` `let msg = e.to_string()` (`sqlx::Error`) | `PushItemResult::error` | sync.rs:555 |
| `admin_db_sources.rs:378/382` `map_err(\|e\| e.to_string())` (`sqlx::Error` з `ping_source`) | `DbSrcErr::BadRequest` | admin_db_sources.rs:632/659/698/814 |

## Джерела (обов'язкові grep-и) і скільки дав кожен

1. `grep -rn "to_string()" crates/torgashka-api/src/*.rs` → **479** входжень. Простежено ті, що ведуть у тіло/`api_err`: **16 → клас A**, **8 → клас C** (санація), **3 → не-продакшн/тести** (`prro.rs:883`, `auth_routes.rs:1062/1072`). Ключові рядки-кандидати: `categories_v2.rs:85`, `documents.rs:107/248/264/269/281/285`, `invoices.rs:105`, `ledger.rs:163`, `pos.rs:187`, `print_templates.rs:98`, `purchase_orders.rs:99`, `return_invoices.rs:94`, `promote.rs:257`, `sync.rs:720/840/966`, `admin_db_sources.rs:78/88/378/382`, `suppliers.rs:43`, `route_local.rs:216/222/228/651`.
2. `grep -rn "api_err(" crates/torgashka-api/src/*.rs` → **17** входжень, **усі в `prro.rs`**. Прод-викликів — 15; аргумент — або `e.public_message()` (санація: `is_db_backed()` → лог + «помилка бази даних ПРРО…»), або літерал. **Клас A: 0.** 2 входження — у `#[cfg(test)]` (`prro.rs:864`, `:886`).
3. `grep -rn "#\[from\]" crates/torgashka-api/src/*.rs` → **13** входжень: `prro.rs` ×10 (`Repo(#[from] PrroRepoError)`, `Grpc/Crypto/Key/Xml/Queue/Settings/Fiscalize`) — **клас дефекту ПРРО, уже закритий** `public_message()`; `readdirs.rs:54` `Service(#[from] application::ServiceError)` → ланцюг до `DirectoryError::Infrastructure` → **1 A-ланцюг**; `sync.rs:87` `Db(#[from] sqlx::Error)` → **B-арм**; `auth.rs:26/28` (`jsonwebtoken`, `std::io`) — не DB.

## Покриття новим шаром `readonly_net`

Умови втручання (`readonly_net.rs:93-110`): **ЛИШЕ write-методи** (`write_gate::is_write_method`) **І** (`state.node_config.is_standby()` **АБО** `guard::hits() > 0`). Тіло мусить мати точний `size_hint ≤ 64 KiB`.

Дві сигнатури:
- `guard::MARKER` = `"[READ_ONLY_REPLICA]"` (`readonly_guard.rs:43`) → **503 §4** (`readonly_net.rs:131-138`);
- `SQLX_DB_ERROR_PREFIX` = `"error returned from database: "` (`readonly_net.rs:63`, `Display` sqlx-core) → **заміна тіла** на `{"detail": …}` + заголовок `x-torgashka-sanitized`, статус збережено (`readonly_net.rs:139-152`).

| файл з (A) | покриває `readonly_net`? | причина |
|---|---|---|
| `categories_v2.rs` | частково | write ✓, префікс у тілі є — але лише на standby/`hits>0`; GET-список не інспектується |
| `crud.rs` | частково | префікс лише для `sqlx::Error::Database`; `PoolTimedOut`/`Io`/`RowNotFound` префікса не мають |
| `readdirs.rs` | частково | GET-роути не інспектуються взагалі |
| `print_templates.rs` | частково | тіло = `sqlx` Display (префікс є), але лише standby/write |
| `products_v2.rs` | частково | так само |
| `documents.rs` | частково | так само |
| `setup.rs` | частково | так само (POST) |
| `suppliers.rs` | частково | GET — не інспектується; префікс присутній лише як підрядок |
| `route_local.rs` | частково | GET `/local/products` не інспектується; POST — лише standby/write |
| `promote.rs` | частково | POST ✓, префікс у `error` полі ✓, але лише standby/write |
| `sync.rs` | частково | POST ✓, префікс ✓ (у `error` per-item), але лише standby/write |
| `admin_db_sources.rs` | **ні** | 400 з `ping_source`: `sqlx` **connect/IO**-помилка не починається з `"error returned from database: "` → сигнатура відсутня; `ProvisionError` несе psql/PG **stderr** — теж без префікса |

**Повністю покритих — 0. Частково — 11. Не покрито — 1 (`admin_db_sources.rs`)**, плюс будь-який (A)-шлях, доступний через **GET** (`readdirs.rs`, `suppliers.rs`, `route_local.rs`) або на вузлі `primary` (шар не втручається взагалі).

## ПІДСУМОК

- **Клас A (дефект): 16 знахідок** у **12 файлах**: `categories_v2.rs`, `crud.rs`, `readdirs.rs`, `print_templates.rs`, `products_v2.rs`, `documents.rs`, `setup.rs`, `suppliers.rs`, `route_local.rs`, `promote.rs`, `sync.rs`, `admin_db_sources.rs`.
- **Клас B (лише лог): 13 знахідок** — `admin.rs`, `admin_audit.rs`, `admin_db_sources.rs`, `admin_migrate.rs`, `admin_network_config.rs` (×3), `admin_prro.rs`, `admin_reports.rs`, `network.rs`, `network_nodes.rs`, `sync.rs`, `stores.rs`.
- **Клас C (санація: людині): 9 знахідок** — `auth_routes.rs`, `crud.rs`, `readdirs.rs`, `pos.rs`, `debtors.rs`, `invoices.rs`, `ledger.rs`, `purchase_orders.rs`, `return_invoices.rs`.
- **Разом рядків таблиці: 16 + 13 + 9 = 38.**

### Топ-5 за ризиком (користувач гарантовано бачить сирий PG)

1. **`sync.rs:555`** (`+`:720/840/966/804/820/922) — `POST /api/v1/sync/push`, **200 OK**, `error` кожного елемента масиву = `sqlx::Error` Display.
2. **`promote.rs:257`** — `POST` promote, **200 OK**, `outbox_drain.error` = `LocalErr` Display (може містити PG-текст репліки).
3. **`admin_db_sources.rs:632`** (`+`:659/698/814, `+`:88) — **400** з host/port/user/db назвами з `sqlx`; сигнатура `readonly_net` відсутня → **не санується ніколи**.
4. **`route_local.rs:202`** — локальні касові роути (GET+POST), `LocalErr::Read` → 500 з PG-текстом репліки.
5. **`crud.rs:140` / `readdirs.rs:107`** — довідники (`/api/v1/...`): `"Помилка БД довідників: <raw>"`.

### Аномалії/неоднозначності (не вгадував)

- `route_local.rs:203` `LocalErr::Queue(m)` → тіло `m.clone()`: джерело — **SQLite**-помилки (`offline::db/commands`), а не PostgreSQL. Формально поза скоупом «PostgreSQL/SQLx», але сирий текст у тілі є. Позначаю **під питанням** (потрібне рішення щодо скоупу SQLite).
- `admin_db_sources.rs:88` `From<ProvisionError>` → `BadRequest(other.to_string())`: сам ланцюг однозначний (400 + Display), але чи містить `reason`/`stderr` саме PG-текст, залежить від джерела заповнення — обидві гілки: `ProvisionError::Connect{reason}` (`provision.rs:61`) і `PsqlFailed/StartFailed` (stdout/stderr psql). Класифіковано **A** за наявністю pg_basebackup/psql stderr.
