//! Read-only guard — перехоплювач «запис у репліку» ЗА КЛАСОМ, а не за інстансом.
//!
//! ## Проблема, яку закриває модуль
//! Ручна таблиця `URL → політика` (`torgashka-api::write_gate::classify_request`)
//! завжди відстає від реальності: ПРРО — третій випадок поспіль (чек → аудит
//! 109 DML-точок → ручний probe). Кожен новий маршрут, що пише в PG, спершу
//! проходить через `StorePool` і отримує від PostgreSQL помилку
//! `cannot execute INSERT in a read-only transaction`.
//!
//! ## Рішення
//! Ловити не «хтось забув рядок у таблиці», а САМ ФАКТ відмови сервера —
//! у єдиному місці, через яке фізично проходять усі одиночні запити
//! репозиторіїв: `crate::store_ctx::StorePool` (його `Executor`).
//! Гарантований сигнал — SQLSTATE **25006** (`read_only_sql_transaction`):
//! стабільний КОД, а не локалізований текст PostgreSQL.
//!
//! ## Межа покриття (важливо)
//! Фунел `StorePool` покриває `sqlx::query(...).fetch_*`/`execute(&pool)`.
//! Він НЕ покриває `StorePool::begin()` → `sqlx::Transaction` (там виконавець
//! `&mut PgConnection`, повз impl `Executor for &StorePool`) і не покриває
//! будь-який інший пул (`PgPool` напряму). Тому «останній рубіж» —
//! HTTP-шар: `torgashka-api::readonly_net` (ловить маркер у тілі відповіді).

use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sqlx::error::{DatabaseError, ErrorKind};
use sqlx::Error;

/// SQLSTATE «read_only_sql_transaction» — сигнал, що вузол у режимі standby,
/// і запис фізично неможливий (усі DML ідуть у read-only транзакцію/репліку).
pub const SQLSTATE_READ_ONLY: &str = "25006";

/// Стабільний маркер для шару відповіді (`torgashka-api::readonly_net`).
///
/// Це КОНТРАКТ між інфраструктурою та HTTP-шаром: якщо цей рядок присутній
/// у тілі відповіді, відповідь переписується на 503 §4 — незалежно від того,
/// який хендлер і який шлях спіймали помилку репліки.
pub const MARKER: &str = "[READ_ONLY_REPLICA]";

/// Людський текст (українською) БЕЗ жодного фрагмента тексту PostgreSQL.
pub const HUMAN_MESSAGE: &str =
    "вузол у режимі standby: запис у локальну репліку неможливий (SQLSTATE 25006)";

/// Максимум символів fingerprint (нормалізованого SQL) — для метрики.
const FINGERPRINT_LEN: usize = 80;

/// Не частіше одного рядка на запит за цей інтервал (анти-спам у лог).
const LOG_THROTTLE: std::time::Duration = std::time::Duration::from_secs(1);

// ─────────────────────────────────────────────────────────────────────────────
// Службові хелпери для шару обробників
// ─────────────────────────────────────────────────────────────────────────────

/// SQLSTATE помилки БД, якщо це `sqlx::Error::Database` (напр. `Some("25006")`).
///
/// Потрібен обробникам: дозволяє віддати клієнту СТАБІЛЬНИЙ машинний код
/// замість сирого тексту PostgreSQL. Для не-БД помилок (`Protocol`, `Io`,
/// `RowNotFound`, …) — `None`.
pub fn sqlstate_of(e: &Error) -> Option<String> {
    match e {
        Error::Database(db) => db.code().map(|c| c.to_string()),
        _ => None,
    }
}

/// Стабільний машинний код для тіл відповідей БЕЗ сирого тексту:
/// `[DB_ERROR <sqlstate>]`, а якщо SQLSTATE немає — `[DB_ERROR]`.
///
/// Свідомо без тексту PG: тіло відповіді не має розкривати ні запит, ні схему.
pub fn db_error_class(e: &Error) -> String {
    match sqlstate_of(e) {
        Some(code) => format!("[DB_ERROR {code}]"),
        None => "[DB_ERROR]".to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Типізована помилка (маркер у Display + SQLSTATE 25006 у code())
// ─────────────────────────────────────────────────────────────────────────────

/// Помилка «запис у read-only репліку»: людський текст + SQLSTATE 25006.
///
/// `Display` містить маркер [`MARKER`] і НЕ містить тексту PostgreSQL (сирий
/// текст сервера йде лише в stderr під час нормалізації).
#[derive(Debug)]
pub struct ReadOnlyReplicaError {
    /// Нормалізований початок SQL (перші 80 символів) — для метрики та логів.
    pub fingerprint: String,
}

impl std::fmt::Display for ReadOnlyReplicaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{MARKER} {HUMAN_MESSAGE}")
    }
}

impl StdError for ReadOnlyReplicaError {}

impl DatabaseError for ReadOnlyReplicaError {
    fn message(&self) -> &str {
        HUMAN_MESSAGE
    }

    fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(SQLSTATE_READ_ONLY))
    }

    fn kind(&self) -> ErrorKind {
        // У sqlx 0.8 немає варіанта `ReadOnly` — класифікуємо як `Other`
        // (єдиний сигнал для класу — SQLSTATE 25006, не `kind`).
        ErrorKind::Other
    }

    #[doc(hidden)]
    fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
        self
    }

    #[doc(hidden)]
    fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
        self
    }

    #[doc(hidden)]
    fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Розпізнавання
// ─────────────────────────────────────────────────────────────────────────────

/// Чи помилка означає запис у read-only репліку (клас, не інстанс).
///
/// Два незалежні сигнали:
///   1. SQLSTATE 25006 від PostgreSQL (реальна відмова сервера);
///   2. наш типізований маркер [`ReadOnlyReplicaError`] (після `normalize`,
///      коли код уже переписано на людський — напр., помилку завернув інший шар).
pub fn is_read_only_replica(e: &Error) -> bool {
    let Error::Database(db) = e else {
        return false;
    };
    if db.code().as_deref() == Some(SQLSTATE_READ_ONLY) {
        return true;
    }
    db.try_downcast_ref::<ReadOnlyReplicaError>().is_some()
}

// ─────────────────────────────────────────────────────────────────────────────
// Нормалізація
// ─────────────────────────────────────────────────────────────────────────────

/// Нормалізує SQL для метрики: літерали й позиційні параметри → `?`,
/// послідовні пробіли стиснуті, обрізано до 80 символів.
///
/// Мета — групувати однакові запити (`INSERT INTO receipts (…) VALUES (?, ?)`),
/// а не рахувати кожен інстанс DML окремо.
pub fn fingerprint(sql: &str) -> String {
    let mut out = String::with_capacity(FINGERPRINT_LEN + 8);
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                // 'літерал' → ? (екрановані '' всередині — частина літерала)
                out.push('?');
                let mut prev_backslash = false;
                for n in chars.by_ref() {
                    if n == '\'' && !prev_backslash {
                        break;
                    }
                    prev_backslash = n == '\\';
                }
            }
            '$' => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if num.is_empty() {
                    out.push('$');
                } else {
                    out.push('?');
                }
            }
            '\n' | '\t' | '\r' => out.push(' '),
            ' ' => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            _ => out.push(c),
        }
        if out.chars().count() >= FINGERPRINT_LEN {
            break;
        }
    }
    out.trim().chars().take(FINGERPRINT_LEN).collect()
}

/// Нормалізує помилку запиту: якщо це відмова read-only репліки — підмінює її
/// на [`ReadOnlyReplicaError`] (маркер + SQLSTATE), рахує метрику і пише
/// rate-limited рядок у stderr. Інакше повертає помилку без змін.
///
/// `sql` — текст запиту (для fingerprint); `None` — fingerprint порожній
/// (виклик поза `StorePool`, де SQL недоступний).
pub fn normalize_with_sql(e: Error, sql: Option<&str>) -> Error {
    if !is_read_only_replica(&e) {
        return e;
    }
    let fp = sql.map(fingerprint).unwrap_or_default();
    record_hit(&fp);
    log_hit(&fp);
    Error::Database(Box::new(ReadOnlyReplicaError { fingerprint: fp }))
}

/// `normalize` без тексту SQL (SQL-фрагмент недоступний на цьому рівні).
pub fn normalize(e: Error) -> Error {
    normalize_with_sql(e, None)
}

// ─────────────────────────────────────────────────────────────────────────────
// Метрики (видимість дрейфу: /api/v1/local/status)
// ─────────────────────────────────────────────────────────────────────────────

static HITS: AtomicU64 = AtomicU64::new(0);
static FALLBACK_HITS: AtomicU64 = AtomicU64::new(0);
static SANITIZED_HITS: AtomicU64 = AtomicU64::new(0);
static LOG_THROTTLE_MAP: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn by_fingerprint() -> &'static Mutex<HashMap<String, u64>> {
    static M: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn last_hit_slot() -> &'static Mutex<Option<(String, u64)>> {
    static M: OnceLock<Mutex<Option<(String, u64)>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

fn last_sanitized_slot() -> &'static Mutex<Option<(String, u64)>> {
    static M: OnceLock<Mutex<Option<(String, u64)>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

/// Поточний unix-час у секундах (0, якщо годинник до епохи).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Мʼютекс без паніки на «отруєному» стані (паніка в іншому потоці не має
/// валити обробку запиту).
fn lock<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Скільки разів фунел `StorePool` спіймав запис у read-only репліку.
pub fn hits() -> u64 {
    HITS.load(Ordering::Relaxed)
}

/// Скільки разів HTTP-шар (`readonly_net`) переписав відповідь на 503 §4
/// (метрика «останній рубіж відпрацював» — те, що пройшло повз фунел).
pub fn fallback_hits() -> u64 {
    FALLBACK_HITS.load(Ordering::Relaxed)
}

/// Скільки разів HTTP-шар (`readonly_net`) САНУВАВ сирий текст помилки БД
/// у тілі відповіді (гілка 2: префікс `Display` помилки БД sqlx).
///
/// Метрика «знову протекло повз усе»: сюди потрапляє те, що не є відмовою
/// read-only репліки, але все одно віддавало клієнту текст PostgreSQL.
pub fn sanitized_hits() -> u64 {
    SANITIZED_HITS.load(Ordering::Relaxed)
}

/// Остання санація тіла: (fingerprint витоку, unix-секунди).
pub fn last_sanitized() -> Option<(String, u64)> {
    lock(last_sanitized_slot()).clone()
}

/// Топ-20 fingerprint-ів за кількістю влучань (спадання).
pub fn hits_by_fingerprint() -> Vec<(String, u64)> {
    let guard = lock(by_fingerprint());
    let mut v: Vec<(String, u64)> = guard.iter().map(|(k, n)| (k.clone(), *n)).collect();
    drop(guard);
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(20);
    v
}

/// Останній перехоплений запис: (fingerprint, unix-секунди).
pub fn last_hit() -> Option<(String, u64)> {
    lock(last_hit_slot()).clone()
}

/// Фіксує перехоплення (викликається з [`normalize_with_sql`]).
pub fn record_hit(fp: &str) {
    HITS.fetch_add(1, Ordering::Relaxed);
    {
        let mut guard = lock(by_fingerprint());
        *guard.entry(fp.to_string()).or_insert(0) += 1;
    }
    *lock(last_hit_slot()) = Some((fp.to_string(), now_unix()));
}

/// Фіксує санацію сирої помилки БД у тілі відповіді (`readonly_net`, гілка 2).
/// Викликається ЗАМІСТЬ [`record_fallback`] — це різні класи:
/// фолбек = відмова репліки (25006), санація = будь-який інший текст БД.
pub fn record_sanitized(fp: &str) {
    SANITIZED_HITS.fetch_add(1, Ordering::Relaxed);
    *lock(last_sanitized_slot()) = Some((fp.to_string(), now_unix()));
}

/// Фіксує спрацювання HTTP-фолбеку (`readonly_net` переписав відповідь).
pub fn record_fallback() {
    FALLBACK_HITS.fetch_add(1, Ordering::Relaxed);
}

/// Rate-limited рядок у stderr (не частіше 1/сек на fingerprint).
pub fn log_hit(fp: &str) {
    let seen = LOG_THROTTLE_MAP.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    {
        let mut guard = match seen.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(prev) = guard.get(fp) {
            if now.duration_since(*prev) < LOG_THROTTLE {
                return;
            }
        }
        guard.insert(fp.to_string(), now);
    }
    eprintln!(
        "[standby-net] перехоплено запис у read-only репліку (SQLSTATE {SQLSTATE_READ_ONLY}); запит: {fp}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести (чиста логіка, без PG)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_collapses_literals_and_params() {
        let a = fingerprint("INSERT INTO receipts (id, total) VALUES ($1, $2)");
        let b = fingerprint("INSERT INTO receipts (id, total) VALUES ($7, $8)");
        assert_eq!(a, b, "різні інстанси одного запиту → один fingerprint");
        assert_eq!(
            fingerprint("SELECT * FROM t WHERE name = 'Іван' AND x = $1"),
            fingerprint("SELECT * FROM t WHERE name = 'Петро' AND x = $2"),
        );
        assert!(fingerprint("   SELECT   1  ").len() <= FINGERPRINT_LEN);
    }

    #[test]
    fn typed_marker_and_plain_errors() {
        let typed = Error::Database(Box::new(ReadOnlyReplicaError {
            fingerprint: fingerprint("SELECT 1 FROM stores WHERE id = $1"),
        }));
        assert!(
            is_read_only_replica(&typed),
            "типізований маркер розпізнається і після нормалізації"
        );
        assert!(!is_read_only_replica(&Error::RowNotFound));
        assert!(!is_read_only_replica(&Error::Protocol("x".into())));
    }
}
