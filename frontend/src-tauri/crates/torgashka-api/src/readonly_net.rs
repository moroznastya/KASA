//! Read-only net — «останній рубіж» для записів і помилок, що пройшли ПОВЗ
//! фунел `StorePool` (напр. транзакції через `StorePool::begin()`).
//!
//! ## Навіщо другий шар
//! Фунел `torgashka_infrastructure::store_ctx::StorePool` (його `Executor`)
//! ловить SQLSTATE 25006 на всіх одиночних запитах репозиторіїв і підмінює
//! помилку на типізовану з маркером [`guard::MARKER`]. Але є шляхи, які фунел
//! не бачить: `sqlx::Transaction` (виконавець `&mut PgConnection`), прямі
//! `PgPool`-запити, а також помилки БД ІНШИХ класів (не 25006), які хендлер
//! віддав у тіло через `Display`.
//!
//! Цей middleware дивиться на РЕЗУЛЬТАТ хендлера і знає **рівно дві стабільні
//! сигнатури** (жодного regex/розбору тексту PostgreSQL):
//! 1. [`guard::MARKER`] — маркер нашого коду (фунел спіймав відмову репліки):
//!    відповідь переписується на контракт §4 (503 + заголовки + людський
//!    текст). Пріоритет цієї гілки — над будь-якими іншими;
//! 2. [`SQLX_DB_ERROR_PREFIX`] — префікс `Display` для `sqlx::Error::Database`,
//!    жорстко закодований у самій бібліотеці (sqlx-core-0.8.6/src/error.rs:44),
//!    тобто сигнатура БІБЛІОТЕКИ, а не конкретної помилки PG: сирий текст
//!    ВИРІЗАЄТЬСЯ з тіла (лишається у журналі сервера), статус хендлера
//!    НЕ змінюється, відповідь позначається [`SANITIZED_HEADER`] і рахується
//!    метрика `guard::sanitized_hits()`.
//!
//! Так клас помилки закривається без переліку маршрутів.
//!
//! ## Межа покриття (свідома, не прихована)
//! * `gate_middleware` і його власний 503 (`STANDBY_DETAIL`, без маркера) —
//!   ця відповідь маркера не має, тож фолбек її не переписує;
//! * рушій не бачить помилку, яку хендлер **повністю сховав** (замінив на
//!   власний загальний текст без префікса sqlx) — тоді в тілі немає жодної з
//!   двох сигнатур і воно лишається байт-в-байт (див. тест-контроль);
//! * `sqlx::Error::Io` має інший префікс (`error communicating with database: `)
//!   і НЕ вважається сигнатурою — свідомо, щоб не тримати в коді зайвих рядків;
//! * тіла > [`MAX_INSPECT_BYTES`] і стрімінгові тіла (невідомий `size_hint`)
//!   не читаються взагалі;
//! * режим `primary` **без** жодного перехоплення фунелом — middleware не
//!   втручається (стан вузла не свідчить про репліку);
//! * читання (write-методи визначає `write_gate::is_write_method`).

use axum::body::{Body, HttpBody};
use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use torgashka_infrastructure::readonly_guard::{self as guard, MARKER};

use crate::write_gate::{self, NODE_MODE_HEADER};
use crate::AppState;

/// Поріг інспекції тіла: більші тіла НЕ читаємо (не платимо за них і не
/// буферизуємо). Відповіді фасаду — `Json`, тож мають `Content-Length`.
const MAX_INSPECT_BYTES: usize = 64 * 1024;

/// Префікс `Display` для `sqlx::Error::Database` — жорстко закодований у
/// sqlx-core-0.8.6/src/error.rs:44 (`#[error("error returned from database: {0}")]`).
///
/// Це стабільна сигнатура БІБЛІОТЕКИ для всієї гілки sqlx 0.8.x, а не текст
/// конкретної помилки PostgreSQL: жодного regex і жодного розбору PG-повідомлень.
const SQLX_DB_ERROR_PREFIX: &str = "error returned from database: ";

/// Заголовок, яким позначається санація тіла (діагностика/клієнт бачать, що
/// тіло підмінене, а не отримане від хендлера).
pub const SANITIZED_HEADER: &str = "x-torgashka-sanitized";

/// Значення [`SANITIZED_HEADER`] для сирої помилки БД у тілі відповіді.
pub const SANITIZED_DB_ERROR: &str = "db-error";

/// Людський текст (українською), яким замінюється тіло з сирою помилкою БД.
/// Не містить жодного фрагмента PG/SQL; деталі лишаються у журналі сервера.
pub const SANITIZED_DETAIL: &str = "помилка бази даних: деталі приховано (див. журнал сервера)";

/// Скільки байтів після префікса беремо у fingerprint для метрики й журналу.
const MAX_LEAK_FRAGMENT_BYTES: usize = 200;

/// Анти-спам у stderr: не частіше одного рядка за цей інтервал.
const LOG_THROTTLE: Duration = Duration::from_secs(1);

/// Middleware «останній рубіж»: read-only репліка → 503 §4, сира помилка БД → санація.
///
/// Монтується ЗОВНІШНІМ щодо `write_gate::gate_middleware` (див.
/// `router_v1::build_router`), тому бачить ОСТАТОЧНУ відповідь хендлера —
/// ту саму, яку вже не змінить жоден інший шар.
pub async fn readonly_net_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    // Дешевий вихід: читання не інспектуємо НІКОЛИ.
    if !write_gate::is_write_method(req.method()) {
        return next.run(req).await;
    }

    let route = req.uri().path().to_string();
    let resp = next.run(req).await;

    // Умова втручання (перевіряється ПІСЛЯ хендлера — щоб бачити свіжу метрику):
    //   * `standby` — нормальний режим репліки; АБО
    //   * `readonly_guard::hits() > 0` — у ЦЬОМУ процесі фунел уже спіймав
    //     відмову репліки, тобто пули вузла фактично read-only (невдалий
    //     promote, кабель у репліку, `mode` ще `primary`). Без цієї гілки
    //     клас «гейт не знав про маршрут» на такій конфігурації лишався б
    //     сирим 400/500 зі стеком PG.
    if !state.node_config.is_standby() && guard::hits() == 0 {
        return resp;
    }

    // Поріг: інспектуємо ЛИШЕ тіла відомого розміру, що вкладаються у ліміт.
    // `size_hint()` — до відправки; `Content-Length` ставить транспорт (hyper)
    // уже ПІСЛЯ нас, тож на самій відповіді його зазвичай немає.
    let known = resp.body().size_hint().exact();
    if known.is_none() || known.is_some_and(|n| n as usize > MAX_INSPECT_BYTES) {
        // Стрімінгове або завелике тіло: НЕ читаємо (не руйнуємо його).
        return resp;
    }

    let (mut parts, body) = resp.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_INSPECT_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            // Тіло зіпсоване на рівні транспорту — зберегти його неможливо.
            eprintln!("[standby-net] тіло відповіді не прочитано ({e}); відповідь не змінено");
            return Response::from_parts(parts, Body::empty());
        }
    };

    // ── Гілка 1 (пріоритет): наш маркер відмови репліки → 503 §4 ─────────
    if contains_marker(&bytes) {
        guard::record_fallback();
        let mut out = write_gate::standby_503(&route);
        stamp_node_mode(&mut out);
        return out;
    }

    // ── Гілка 2: префікс sqlx → вирізати сирий текст, статус НЕ змінювати ─
    if let Some(fragment) = leaked_db_error(&bytes) {
        // Метрика «знову протекло повз усе» + сирий текст у ЖУРНАЛ (не клієнту).
        guard::record_sanitized(&fragment);
        log_sanitized(&route, &fragment);

        // Тіло змінюється → старий `Content-Length` недійсний (його поставить
        // транспорт за фактичним розміром нового тіла).
        parts.headers.remove(header::CONTENT_LENGTH);
        parts.headers.insert(
            HeaderName::from_static(SANITIZED_HEADER),
            HeaderValue::from_static(SANITIZED_DB_ERROR),
        );
        // Статус хендлера збережено як є (400 лишається 400, 500 — 500).
        return Response::from_parts(parts, sanitized_body());
    }

    // Жодної з двох сигнатур → відповідь БАЙТ-В-БАЙТ та сама.
    Response::from_parts(parts, Body::from(bytes))
}

/// Тіло замість сирої помилки БД: той самий JSON-контракт `{"detail": …}`.
fn sanitized_body() -> Body {
    Body::from(
        serde_json::json!({ "detail": SANITIZED_DETAIL })
            .to_string()
            .into_bytes(),
    )
}

/// Чи є маркер read-only репліки у тілі (побайтово, без припущень про UTF-8).
fn contains_marker(bytes: &[u8]) -> bool {
    bytes.windows(MARKER.len()).any(|w| w == MARKER.as_bytes())
}

/// Шукає префікс `Display` помилки БД sqlx і повертає fingerprint витоку
/// (для метрики й журналу). Пошук — побайтово за ОДНІЄЮ стабільною
/// сигнатурою; фрагмент після префікса — лише для логу, не для рішень.
fn leaked_db_error(bytes: &[u8]) -> Option<String> {
    let prefix = SQLX_DB_ERROR_PREFIX.as_bytes();
    let pos = bytes.windows(prefix.len()).position(|w| w == prefix)?;
    let tail = &bytes[pos + prefix.len()..];
    // Кінець фрагмента: лапка/екранована лапка/новий рядок (JSON-рядок) або ліміт.
    let end = tail
        .iter()
        .position(|b| matches!(*b, b'"' | b'\\' | b'\n' | b'\r'))
        .unwrap_or(tail.len());
    let take = end.min(MAX_LEAK_FRAGMENT_BYTES);
    Some(guard::fingerprint(
        String::from_utf8_lossy(&tail[..take]).trim(),
    ))
}

/// Rate-limited рядок у stderr: сирий фрагмент бачить ЛИШЕ журнал сервера.
fn log_sanitized(route: &str, fragment: &str) {
    static LAST: OnceLock<Mutex<Instant>> = OnceLock::new();
    let last = LAST.get_or_init(|| Mutex::new(Instant::now()));
    {
        let mut guard = match last.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.elapsed() < LOG_THROTTLE {
            return;
        }
        *guard = Instant::now();
    }
    eprintln!("[readonly_net] сира помилка БД у тілі відповіді — санація ({route}): {fragment}");
}

/// Додає `X-Torgashka-Node-Mode: standby` (§4) — той самий заголовок і
/// значення, що ставить `gate_middleware`; тут він потрібен, бо ми
/// ПІДМІНЮЄМО відповідь (зовнішній шар бачить уже нову).
fn stamp_node_mode(resp: &mut Response) {
    resp.headers_mut().insert(
        HeaderName::from_static(NODE_MODE_HEADER),
        HeaderValue::from_static("standby"),
    );
}
