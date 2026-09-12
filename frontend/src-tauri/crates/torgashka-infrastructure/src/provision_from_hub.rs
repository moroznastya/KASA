//! provision_from_hub — «перший запуск вузла» (ADR-0008, «Варіант B»):
//! БД вузла НАРОДЖУЄТЬСЯ зі знімка хаба, а не з порожньої схеми.
//!
//! Що це вирішує: нова каса/вузол у мережі не має власника, точок, товарів і
//! PIN-логінів — самотужки вона не підніметься. Хаб віддає СВІЙ знімок БД
//! (`GET /api/v1/sync/snapshot`, контракт R1), вузол його відновлює у власну
//! БД і з цієї ж БД дізнається, куди форвардити вгору (`sync.hub_url` +
//! `sync.hub_token`).
//!
//! Послідовність (кожен крок = елемент звіту; невдача → `ok = false` з класом):
//!   1. `validate_url`   — нормалізація `hub_url` (trim, схема, хвіст `/`),
//!      перевірка хоста/порту; порожній URL АБО порожній токен → `BadUrl`
//!      (вузол без токена не форвардить — «все гаразд» тут було б брехнею);
//!   2. `hub_reachable`  — `GET {hub}/api/v1/health` (публічний, 200) з
//!      таймаутом [`DEFAULT_HEALTH_TIMEOUT`] → інакше `HubUnreachable`;
//!   3. `download`       — `GET {hub}/api/v1/sync/snapshot` з Bearer-токеном
//!      ([`DEFAULT_DOWNLOAD_TIMEOUT`]); збіг `X-Snapshot-Sha256` з фактичним
//!      sha256 ТИЛА обов'язковий, розбіжність → `DownloadFailed`;
//!   4. `restore`        — embedded PG піднято, БД існує, потім `pg_restore`;
//!   5. `configure`      — `sync.hub_url` / `sync.hub_token` у ЦЮ БД (через
//!      наявний репозиторій налаштувань, не raw-SQL).
//!
//! Кроки 2 і 3 НЕ виконуються, коли задано `local_dump_path`: офлайн-провіжн
//! («перший запуск із USB/файлу») не мусить вимагати мережі. У звіті ці два
//! кроки присутні з явним поясненням, що вони не виконувались — оператор
//! бачить повний чек-лист, а не порожнечу.
//!
//! # Чому цільова БД і прогрес — ПАРАМЕТРИ
//! * `target_db_url` — ціль відновлення. У проді це `db::resolve_database_url()`
//!   (зовнішня БД вузла) або `EmbeddedPostgres::database_url()` (вбудована).
//!   Тест підставляє СВІЙ кластер у tmp-каталозі — інакше «наскрізний» тест
//!   писав би в робочу БД машини.
//! * `progress` — колбек прогресу. Ядро не знає ні про Tauri, ні про події:
//!   UI-шар передає сюди `emit` (контракт R2), тест — `eprintln!`, і обидва
//!   отримують ТІ САМІ кроки/статуси без дублювання логіки.
//!
//! # Чому немає `--single-transaction`
//! Відновлення виконується рівно тими прапорцями, що зафіксовані контрактом:
//! `--clean --if-exists --no-owner --no-privileges`. `--single-transaction`
//! зробив би невідновлюваним частковий стан при падінні на середині дампа, але
//! у TOC знімка немає ані `CREATE DATABASE`, ані `\connect` (перевірено
//! `pg_restore -l`: 434 записи), тож причина для явної транзакції відсутня, а
//! зайвий режим — зайва поведінка. Ненульовий код виходу `pg_restore` завжди
//! `RestoreFailed` (попередження не ігноруються «наосліп»: вони їдуть у
//! `stderr_tail` і `steps[].detail`).
//!
//! # Чого тут НАВМИСНО немає
//! Застосування sync-шару (Alembic 0011–0024: `sync_meta`, `hub_outbox`,
//! `catalog_change_requests`) — у проді його додає міграція, а не провіжн.
//! Знімок хаба зафіксований на `alembic_version = 0014`, тому відновлена БД
//! неповна САМЕ ЩОДО sync-шару; це відоме обмеження знімка, а не дефект R2
//! (див. звіт контракту).
//!
//! # Безпека
//! * `hub_token` ніколи не потрапляє в повідомлення/деталі кроків — лише
//!   кількість символів; у БД він пишеться, як і належить;
//! * `target_db_url` у текстах проходить [`redact_url`] (пароль маскується);
//! * ім'я файлу дампа санітизується (`Path::file_name`) — заголовок хаба не
//!   може вивести запис за межі каталогу знімків.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use torgashka_domain::AuthService;

use crate::embedded_pg::{pg_restore_name, psql_conn_env, EmbeddedPostgres};
use crate::repositories::auth::SqlxAuth;
use crate::store_ctx::StorePool;
use crate::sync_settings::{HUB_TOKEN_SETTING, HUB_URL_SETTING};

// ─────────────────────────────────────────────────────────────────────────────
// Кроки та класи помилок (заморожені контрактом R2 — літерали в одному місці)
// ─────────────────────────────────────────────────────────────────────────────

/// Крок: нормалізація та валідація адреси хаба.
pub const STEP_VALIDATE_URL: &str = "validate_url";
/// Крок: перевірка досяжності хаба (`/api/v1/health`).
pub const STEP_HUB_REACHABLE: &str = "hub_reachable";
/// Крок: отримання дампа (мережа або локальний файл).
pub const STEP_DOWNLOAD: &str = "download";
/// Крок: відновлення дампа в БД вузла (`pg_restore`).
pub const STEP_RESTORE: &str = "restore";
/// Крок: запис `sync.hub_url` / `sync.hub_token` у БД вузла.
pub const STEP_CONFIGURE: &str = "configure";

/// Клас: некоректна адреса хаба або порожній токен.
pub const CLASS_BAD_URL: &str = "BadUrl";
/// Клас: хаб недосяжний.
pub const CLASS_HUB_UNREACHABLE: &str = "HubUnreachable";
/// Клас: дамп не отримано/не прочитано (мережа, токен, sha256, IO).
pub const CLASS_DOWNLOAD_FAILED: &str = "DownloadFailed";
/// Клас: `pg_restore` не відновив БД.
pub const CLASS_RESTORE_FAILED: &str = "RestoreFailed";
/// Клас: БД вузла недоступна/не піднялась/не приймає налаштування.
pub const CLASS_DB_UNAVAILABLE: &str = "DbUnavailable";

/// Джерело дампа: мережа (хаб).
pub const SOURCE_HUB: &str = "hub";
/// Джерело дампа: локальний файл (офлайн-провіжн).
pub const SOURCE_FILE: &str = "file";

/// Шлях health-проби хаба (публічний, без JWT).
pub const HEALTH_PATH: &str = "/api/v1/health";
/// Шлях знімка БД (контракт R1).
pub const SNAPSHOT_PATH: &str = "/api/v1/sync/snapshot";
/// Заголовок з очікуваним sha256 знімка.
pub const SHA256_HEADER: &str = "x-snapshot-sha256";
/// Заголовок з іменем файлу знімка.
pub const FILENAME_HEADER: &str = "x-snapshot-filename";

/// Таймаут health-проби (контракт: ~5 с).
pub const DEFAULT_HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
/// Таймаут завантаження знімка (контракт: 60 с).
pub const DEFAULT_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// Таймаут підключення до БД вузла під час запису налаштувань.
pub const DB_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Скільки останніх рядків виводу інструмента вважати «хвостом».
pub const STDERR_TAIL_LINES: usize = 20;
/// Підкаталог тимчасових знімків у `std::env::temp_dir()`.
pub const DUMP_DIR_NAME: &str = "torgashka_node_provision";
/// Ім'я файлу знімка, коли хаб не повідомив `X-Snapshot-Filename`.
pub const DEFAULT_DUMP_FILENAME: &str = "hub_snapshot.dump";

// ─────────────────────────────────────────────────────────────────────────────
// Публічні типи
// ─────────────────────────────────────────────────────────────────────────────

/// Статус кроку в події прогресу (контракт: `started` | `ok` | `failed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStatus {
    /// Крок почався.
    Started,
    /// Крок завершився успішно.
    Ok,
    /// Крок завершився невдачею.
    Failed,
}

impl ProgressStatus {
    /// Рядкове значення для події (саме ці три слова бачить фронтенд).
    pub fn as_str(self) -> &'static str {
        match self {
            ProgressStatus::Started => "started",
            ProgressStatus::Ok => "ok",
            ProgressStatus::Failed => "failed",
        }
    }
}

/// Елемент звіту: що робили і чим скінчилось.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubProvisionStep {
    /// Назва кроку (див. `STEP_*`).
    pub step: String,
    /// Чи крок успішний (для кроків, що не виконувались у офлайн-режимі — `true`
    /// з поясненням у `detail`).
    pub ok: bool,
    /// Подробиці: фактичні значення, шляхи, код виходу, хвіст виводу.
    pub detail: String,
}

/// Повний результат провіжну (мапиться 1:1 у `ProvisionReport` Tauri-шару).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubProvisionOutcome {
    /// Чи все минуло успішно.
    pub ok: bool,
    /// Клас невдачі (`BadUrl`|`HubUnreachable`|`DownloadFailed`|`RestoreFailed`|`DbUnavailable`).
    pub class: Option<String>,
    /// Людське повідомлення українською (з фактами, а не «помилка»).
    pub message: String,
    /// Хвіст виводу інструмента (`pg_restore`) — не губиться навіть на успіху.
    pub stderr_tail: Option<String>,
    /// НОРМАЛІЗОВАНИЙ URL хаба, фактично записаний у БД вузла.
    pub hub_url: Option<String>,
    /// Джерело дампа: `hub` або `file`.
    pub source: Option<String>,
    /// Розмір використаного дампа, байт.
    pub dump_bytes: Option<u64>,
    /// sha256 фактично використаних байтів дампа.
    pub dump_sha256: Option<String>,
    /// Кроки у порядку виконання.
    pub steps: Vec<HubProvisionStep>,
}

/// Параметри провіжну.
#[derive(Debug, Clone)]
pub struct HubProvisionConfig {
    /// Адреса хаба як її ввів оператор (нормалізується всередині).
    pub hub_url: String,
    /// Токен вузла для хаба (порожній → `BadUrl`).
    pub token: String,
    /// Локальний `.dump` (офлайн-провіжн). `None` → завантаження з хаба.
    pub local_dump_path: Option<PathBuf>,
    /// Цільова БД вузла. Порожній рядок → взяти з піднятої embedded PG.
    pub target_db_url: String,
    /// Піднімати/перевіряти embedded PG вузла (у проді `true`; тест, що тримає
    /// власний кластер, ставить `false` і передає `target_db_url`).
    pub ensure_local_db: bool,
    /// Каталог для знімка з хаба (за замовчуванням `temp/torgashka_node_provision`).
    pub dump_dir: Option<PathBuf>,
    /// Таймаут health-проби.
    pub health_timeout: Duration,
    /// Таймаут завантаження знімка.
    pub download_timeout: Duration,
}

impl HubProvisionConfig {
    /// Конфіг з продакшн-дефолтами (таймаути 5 с / 60 с, каталог — у temp).
    pub fn new(
        hub_url: impl Into<String>,
        token: impl Into<String>,
        local_dump_path: Option<PathBuf>,
    ) -> Self {
        Self {
            hub_url: hub_url.into(),
            token: token.into(),
            local_dump_path,
            target_db_url: String::new(),
            ensure_local_db: true,
            dump_dir: None,
            health_timeout: DEFAULT_HEALTH_TIMEOUT,
            download_timeout: DEFAULT_DOWNLOAD_TIMEOUT,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Чисті хелпери (покриті юніт-тестами)
// ─────────────────────────────────────────────────────────────────────────────

/// Нормалізує адресу хаба: `trim` → схема `http://` за відсутності → без
/// хвостових `/`. Повертає `None`, якщо хост/порт невалідні.
///
/// Свідомо НЕ приймаємо: порожній рядок, пробіли всередині, порт не-число,
/// порт `0`, порт поза `u16`, «адресу» без хоста (`http:///api`), IPv6 без
/// дужок (двокрапки зламали б розбір хоста/порта — краще чесний `BadUrl`, ніж
/// тихо зібраний неправильний URL).
pub fn normalize_hub_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let mut cleaned = with_scheme;
    while cleaned.ends_with('/') {
        cleaned.pop();
    }
    let rest = cleaned
        .strip_prefix("http://")
        .or_else(|| cleaned.strip_prefix("https://"))?;
    if rest.is_empty() || rest.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let authority = rest.split('/').next().unwrap_or("");
    if !authority_is_valid(authority) {
        return None;
    }
    Some(cleaned)
}

/// Валідність `host[:port]` (IPv6 у дужках — дозволено, без дужок — ні).
fn authority_is_valid(authority: &str) -> bool {
    if authority.is_empty() {
        return false;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        // [::1] або [::1]:5433
        let Some(close) = rest.find(']') else {
            return false;
        };
        let host = &rest[..close];
        let tail = &rest[close + 1..];
        if host.is_empty() {
            return false;
        }
        match tail.strip_prefix(':') {
            None if tail.is_empty() => (host, None),
            Some(p) => (host, Some(p)),
            None => return false,
        }
    } else {
        match authority.split_once(':') {
            None => (authority, None),
            Some((h, p)) => (h, Some(p)),
        }
    };
    if host.is_empty() {
        return false;
    }
    match port {
        None => true,
        Some(p) => matches!(p.parse::<u16>(), Ok(v) if v > 0),
    }
}

/// Останні `n` рядків тексту (хвіст виводу інструмента; без хвостового `\n`).
pub fn tail_lines(text: &str, n: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(n);
    all[start..].join("\n")
}

/// Аргументи `pg_restore` (чиста функція): рівно контрактні прапорці.
pub fn pg_restore_args(database_url: &str, dump: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "--clean".into(),
        "--if-exists".into(),
        "--no-owner".into(),
        "--no-privileges".into(),
        "-d".into(),
        database_url.into(),
        dump.as_os_str().into(),
    ]
}

/// Маскує пароль у URL для логів/повідомлень (`postgresql://u:***@h/db`).
pub fn redact_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let head = &url[..scheme_end + 3];
    let rest = &url[scheme_end + 3..];
    let Some(at) = rest.find('@') else {
        return url.to_string();
    };
    let creds = &rest[..at];
    match creds.split_once(':') {
        Some((user, _)) => format!("{head}{user}:***{}", &rest[at..]),
        None => url.to_string(),
    }
}

/// Безпечне ім'я файлу знімка: лише базове ім'я, без шляхів і пробілів-спереду.
pub fn safe_dump_filename(raw: Option<&str>) -> String {
    let candidate = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| {
            Path::new(s)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_DUMP_FILENAME.to_string());
    candidate
}

/// sha256 байтів у hex (нижній регістр) — та сама форма, що `X-Snapshot-Sha256`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Звіт (внутрішній будівельник)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Report {
    steps: Vec<HubProvisionStep>,
    hub_url: Option<String>,
    source: Option<String>,
    dump_bytes: Option<u64>,
    dump_sha256: Option<String>,
    stderr_tail: Option<String>,
}

impl Report {
    /// Початок кроку: подія `started` + (для ока) нічого не пишемо у звіт.
    fn begin<F>(&self, step: &str, progress: &F, message: &str)
    where
        F: Fn(&str, ProgressStatus, &str) + Send + Sync,
    {
        progress(step, ProgressStatus::Started, message);
    }

    /// Крок завершено успішно: подія `ok` + запис у звіт.
    fn step_ok<F>(&mut self, step: &str, progress: &F, detail: String)
    where
        F: Fn(&str, ProgressStatus, &str) + Send + Sync,
    {
        progress(step, ProgressStatus::Ok, &detail);
        self.steps.push(HubProvisionStep {
            step: step.to_string(),
            ok: true,
            detail,
        });
    }

    /// Крок не виконувався (офлайн-режим): без події, але з явним поясненням.
    fn step_skipped(&mut self, step: &str, detail: String) {
        self.steps.push(HubProvisionStep {
            step: step.to_string(),
            ok: true,
            detail,
        });
    }

    /// Крок провалено: подія `failed` + запис у звіт.
    fn step_failed<F>(&mut self, step: &str, progress: &F, detail: String)
    where
        F: Fn(&str, ProgressStatus, &str) + Send + Sync,
    {
        progress(step, ProgressStatus::Failed, &detail);
        self.steps.push(HubProvisionStep {
            step: step.to_string(),
            ok: false,
            detail,
        });
    }

    /// Завершення з невдачею: клас + людське повідомлення.
    fn fail(self, class: &str, message: String) -> HubProvisionOutcome {
        HubProvisionOutcome {
            ok: false,
            class: Some(class.to_string()),
            message,
            stderr_tail: self.stderr_tail,
            hub_url: self.hub_url,
            source: self.source,
            dump_bytes: self.dump_bytes,
            dump_sha256: self.dump_sha256,
            steps: self.steps,
        }
    }

    /// Завершення з успіхом.
    fn done(self, message: String) -> HubProvisionOutcome {
        HubProvisionOutcome {
            ok: true,
            class: None,
            message,
            stderr_tail: self.stderr_tail,
            hub_url: self.hub_url,
            source: self.source,
            dump_bytes: self.dump_bytes,
            dump_sha256: self.dump_sha256,
            steps: self.steps,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Запуск зовнішніх інструментів
// ─────────────────────────────────────────────────────────────────────────────

/// Результат виконання зовнішнього інструмента.
struct ToolOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Запускає інструмент і збирає ПОВНІСТЮ stdout+stderr (хвіст іде у звіт).
///
/// Синхронний виклик: у async-контексті його обов'язково викликати через
/// `spawn_blocking` (див. [`restore_dump`]) — subprocess у потоці рантайму
/// заморожує HTTP-фасад застосунку.
fn run_tool(program: &Path, args: &[std::ffi::OsString]) -> std::io::Result<ToolOutput> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    for (k, v) in psql_conn_env() {
        command.env(k, v);
    }
    let out = command.output()?;
    Ok(ToolOutput {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Головна функція
// ─────────────────────────────────────────────────────────────────────────────

/// Виконує провіжн вузла. `progress` викликається на кожен крок
/// (`step`, `status`, `message`); для Tauri це `emit`, для тесту — лог.
pub async fn run<F>(cfg: HubProvisionConfig, progress: F) -> HubProvisionOutcome
where
    F: Fn(&str, ProgressStatus, &str) + Send + Sync,
{
    let mut report = Report::default();

    // ── (а) validate_url ────────────────────────────────────────────────────
    report.begin(
        STEP_VALIDATE_URL,
        &progress,
        "перевіряю адресу хаба та токен вузла",
    );
    let hub_url = match normalize_hub_url(&cfg.hub_url) {
        Some(u) => u,
        None => {
            let detail = format!("некоректна адреса хаба: {:?}", cfg.hub_url);
            report.step_failed(STEP_VALIDATE_URL, &progress, detail.clone());
            return report.fail(CLASS_BAD_URL, detail);
        }
    };
    let token = cfg.token.trim().to_string();
    if token.is_empty() {
        let detail = format!(
            "порожній токен вузла для хаба {hub_url} — без токена вузол не зможе \
             форвардити прийняте вгору"
        );
        report.step_failed(STEP_VALIDATE_URL, &progress, detail.clone());
        return report.fail(CLASS_BAD_URL, detail);
    }
    report.hub_url = Some(hub_url.clone());
    report.step_ok(
        STEP_VALIDATE_URL,
        &progress,
        format!(
            "адресу нормалізовано: {hub_url}; токен: {} символів (не логується)",
            token.chars().count()
        ),
    );

    // ── (б) hub_reachable та (в) download ───────────────────────────────────
    // Кроки виконуються ЛИШЕ для мережевого джерела: офлайн-провіжн не мусить
    // вимагати мережі взагалі.
    let dump_path: PathBuf;
    if let Some(local) = cfg.local_dump_path.clone() {
        report.source = Some(SOURCE_FILE.to_string());
        report.step_skipped(
            STEP_HUB_REACHABLE,
            format!(
                "не виконувалось: джерело — локальний файл ({})",
                local.display()
            ),
        );
        report.begin(
            STEP_DOWNLOAD,
            &progress,
            &format!("читаю локальний дамп {}", local.display()),
        );
        match std::fs::read(&local) {
            Ok(bytes) => {
                let size = bytes.len() as u64;
                let sha = sha256_hex(&bytes);
                report.dump_bytes = Some(size);
                report.dump_sha256 = Some(sha.clone());
                report.step_ok(
                    STEP_DOWNLOAD,
                    &progress,
                    format!(
                        "дамп із локального файлу {}: {size} Б, sha256={sha}",
                        local.display()
                    ),
                );
                dump_path = local;
            }
            Err(e) => {
                let detail = format!("файл дампа недоступний ({}): {e}", local.display());
                report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
                return report.fail(CLASS_DOWNLOAD_FAILED, detail);
            }
        }
    } else {
        // (б) досяжність хаба: публічний /health, таймаут ~5 с.
        report.begin(
            STEP_HUB_REACHABLE,
            &progress,
            &format!("перевіряю {hub_url}{HEALTH_PATH}"),
        );
        let health_client = match reqwest::Client::builder()
            .timeout(cfg.health_timeout)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                let detail = format!("HTTP-клієнт: {e}");
                report.step_failed(STEP_HUB_REACHABLE, &progress, detail.clone());
                return report.fail(CLASS_HUB_UNREACHABLE, detail);
            }
        };
        match health_client
            .get(format!("{hub_url}{HEALTH_PATH}"))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                report.step_ok(
                    STEP_HUB_REACHABLE,
                    &progress,
                    format!("{hub_url}{HEALTH_PATH} → HTTP {}", resp.status().as_u16()),
                );
            }
            Ok(resp) => {
                let code = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                let detail = format!(
                    "хаб відповів на {HEALTH_PATH} кодом {code}: {}",
                    tail_lines(&body, 3)
                );
                report.step_failed(STEP_HUB_REACHABLE, &progress, detail.clone());
                return report.fail(CLASS_HUB_UNREACHABLE, detail);
            }
            Err(e) => {
                let detail = format!(
                    "хаб недосяжний за {hub_url} (таймаут {:?}): {e}",
                    cfg.health_timeout
                );
                report.step_failed(STEP_HUB_REACHABLE, &progress, detail.clone());
                return report.fail(CLASS_HUB_UNREACHABLE, detail);
            }
        }

        // (в) завантаження знімка з хаба.
        report.begin(
            STEP_DOWNLOAD,
            &progress,
            &format!(
                "завантажую {hub_url}{SNAPSHOT_PATH} (таймаут {:?})",
                cfg.download_timeout
            ),
        );
        let client = match reqwest::Client::builder()
            .timeout(cfg.download_timeout)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                let detail = format!("HTTP-клієнт: {e}");
                report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
                return report.fail(CLASS_DOWNLOAD_FAILED, detail);
            }
        };
        let response = match client
            .get(format!("{hub_url}{SNAPSHOT_PATH}"))
            .bearer_auth(&token)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let detail = format!("запит знімка не вдався: {e}");
                report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
                return report.fail(CLASS_DOWNLOAD_FAILED, detail);
            }
        };
        let status = response.status();
        let headers = response.headers().clone();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            let detail = format!(
                "хаб відкинув токен вузла (HTTP {}): перевірте токен у налаштуваннях хаба",
                status.as_u16()
            );
            report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
            return report.fail(CLASS_DOWNLOAD_FAILED, detail);
        }
        if !status.is_success() {
            let code = status.as_u16();
            let body = response.text().await.unwrap_or_default();
            let detail = format!(
                "хаб повернув {code} на {SNAPSHOT_PATH}: {}",
                tail_lines(&body, 3)
            );
            report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
            return report.fail(CLASS_DOWNLOAD_FAILED, detail);
        }
        let expected_sha = headers
            .get(SHA256_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_ascii_lowercase());
        let filename =
            safe_dump_filename(headers.get(FILENAME_HEADER).and_then(|v| v.to_str().ok()));
        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let detail = format!("тіло знімка не дочитано: {e}");
                report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
                return report.fail(CLASS_DOWNLOAD_FAILED, detail);
            }
        };
        let size = bytes.len() as u64;
        let sha = sha256_hex(&bytes);
        // Заголовок є — звірка обов'язкова (жодного «приймемо як є»).
        if let Some(expected) = expected_sha.as_deref() {
            if expected != sha {
                let detail = format!(
                    "sha256 знімка не збігається: хаб заявив {expected}, отримано {sha} \
                     ({size} Б) — дамп НЕ прийнято"
                );
                report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
                report.dump_bytes = Some(size);
                report.dump_sha256 = Some(sha);
                return report.fail(CLASS_DOWNLOAD_FAILED, detail);
            }
        }
        let dir = cfg
            .dump_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join(DUMP_DIR_NAME));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            let detail = format!("каталог знімків {} недоступний: {e}", dir.display());
            report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
            return report.fail(CLASS_DOWNLOAD_FAILED, detail);
        }
        let target = dir.join(&filename);
        if let Err(e) = std::fs::write(&target, &bytes) {
            let detail = format!("знімок не записано у {}: {e}", target.display());
            report.step_failed(STEP_DOWNLOAD, &progress, detail.clone());
            return report.fail(CLASS_DOWNLOAD_FAILED, detail);
        }
        report.source = Some(SOURCE_HUB.to_string());
        report.dump_bytes = Some(size);
        report.dump_sha256 = Some(sha.clone());
        report.step_ok(
            STEP_DOWNLOAD,
            &progress,
            format!(
                "знімок отримано: {size} Б, sha256={sha}, файл {} (sha256 звірено з {SHA256_HEADER})",
                target.display()
            ),
        );
        dump_path = target;
    }

    // ── (г) restore ─────────────────────────────────────────────────────────
    report.begin(
        STEP_RESTORE,
        &progress,
        "готую БД вузла та виконую pg_restore",
    );

    // pg_restore резолвиться ТИМ САМИМ резолвером, що PG-бінарники.
    let Some(bin_dir) = EmbeddedPostgres::locate() else {
        let detail = "бінарники PostgreSQL не знайдено (TORGASHKA_PG_DIR, resources/postgres, \
                      .cache/pg, pg_config) — pg_restore запустити нічим"
            .to_string();
        report.step_failed(STEP_RESTORE, &progress, detail.clone());
        return report.fail(CLASS_DB_UNAVAILABLE, detail);
    };
    let pg_restore = bin_dir.join(pg_restore_name());
    if !pg_restore.is_file() {
        let detail = format!(
            "pg_restore не знайдено у {} (перевірте TORGASHKA_PG_DIR — потрібен каталог bin \
             PostgreSQL 17)",
            bin_dir.display()
        );
        report.step_failed(STEP_RESTORE, &progress, detail.clone());
        return report.fail(CLASS_DB_UNAVAILABLE, detail);
    }

    // Цільова БД: або піднята embedded PG вузла, або ін'єктована адреса.
    let database_url = if cfg.ensure_local_db {
        let pg = EmbeddedPostgres::new(bin_dir.clone());
        if let Err(e) = pg.ensure_initialized() {
            let detail = format!("initdb/перевірка каталогу даних не вдалась: {e}");
            report.step_failed(STEP_RESTORE, &progress, detail.clone());
            return report.fail(CLASS_DB_UNAVAILABLE, detail);
        }
        // start_detached: володіння сервером лишається рівню застосунку (Drop
        // цього екземпляра НЕ зупинить щойно піднятий сервер).
        if let Err(e) = pg.clone().start_detached() {
            let detail = format!("embedded PostgreSQL не піднявся: {e}");
            report.step_failed(STEP_RESTORE, &progress, detail.clone());
            return report.fail(CLASS_DB_UNAVAILABLE, detail);
        }
        if let Err(e) = pg.ensure_database() {
            let detail = format!("БД вузла не створена: {e}");
            report.step_failed(STEP_RESTORE, &progress, detail.clone());
            return report.fail(CLASS_DB_UNAVAILABLE, detail);
        }
        pg.database_url()
    } else {
        if cfg.target_db_url.trim().is_empty() {
            let detail = "цільову БД не задано (порожній URL і вимкнений embedded PG)".to_string();
            report.step_failed(STEP_RESTORE, &progress, detail.clone());
            return report.fail(CLASS_DB_UNAVAILABLE, detail);
        }
        cfg.target_db_url.trim().to_string()
    };

    let args = pg_restore_args(&database_url, &dump_path);
    let program = pg_restore.clone();
    let output = match tokio::task::spawn_blocking(move || run_tool(&program, &args)).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            let detail = format!("не вдалось запустити {}: {e}", pg_restore.display());
            report.step_failed(STEP_RESTORE, &progress, detail.clone());
            return report.fail(CLASS_RESTORE_FAILED, detail);
        }
        Err(e) => {
            let detail = format!("pg_restore перервано: {e}");
            report.step_failed(STEP_RESTORE, &progress, detail.clone());
            return report.fail(CLASS_RESTORE_FAILED, detail);
        }
    };
    let combined = format!("{}\n{}", output.stderr, output.stdout);
    let tail = tail_lines(combined.trim(), STDERR_TAIL_LINES);
    report.stderr_tail = if tail.is_empty() {
        None
    } else {
        Some(tail.clone())
    };
    if output.code != Some(0) {
        let detail = format!(
            "pg_restore завершився з кодом {:?} (БД {}) — відновлення НЕ виконано; хвіст виводу:\n{tail}",
            output.code,
            redact_url(&database_url)
        );
        report.step_failed(STEP_RESTORE, &progress, detail.clone());
        return report.fail(CLASS_RESTORE_FAILED, detail);
    }
    report.step_ok(
        STEP_RESTORE,
        &progress,
        format!(
            "pg_restore: код 0, БД {} (дамп {}); хвіст виводу:\n{}",
            redact_url(&database_url),
            dump_path.display(),
            if tail.is_empty() {
                "(порожній)".to_string()
            } else {
                tail
            }
        ),
    );

    // ── (д) configure ───────────────────────────────────────────────────────
    report.begin(
        STEP_CONFIGURE,
        &progress,
        "записую sync.hub_url і sync.hub_token у БД вузла",
    );
    match write_settings(&database_url, &hub_url, &token).await {
        Ok(detail) => {
            report.step_ok(STEP_CONFIGURE, &progress, detail);
        }
        Err(detail) => {
            report.step_failed(STEP_CONFIGURE, &progress, detail.clone());
            return report.fail(CLASS_DB_UNAVAILABLE, detail);
        }
    }

    // ── (е) результат ───────────────────────────────────────────────────────
    let source = report.source.clone().unwrap_or_else(|| "?".to_string());
    let bytes = report.dump_bytes.unwrap_or(0);
    let message = format!(
        "вузол налаштовано зі знімка: джерело={source}, розмір={bytes} Б, хаб={hub_url} \
         (sync.hub_url і sync.hub_token записано в БД вузла; відновлення pg_restore — код 0)"
    );
    report.done(message)
}

/// Записує `sync.hub_url` і `sync.hub_token` у БД вузла НАЯВНИМ репозиторієм.
///
/// StoreCtx-скоуп: рядки мусять мати `store_id IS NULL` — провіжн виконується
/// від імені інстанса, а не точки. Пул на одну сесію + явне скидання
/// `app.store_id` робить це детермінованим (без цього налаштування могло б
/// «прилипнути» до точки каси). Факт перевіряється запитом після запису: якщо
/// рядок пішов у скоуп — це аномалія, і операція завершується невдачею, а не
/// «нібито все гаразд».
async fn write_settings(database_url: &str, hub_url: &str, token: &str) -> Result<String, String> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(DB_CONNECT_TIMEOUT)
        .connect(database_url)
        .await
        .map_err(|e| {
            format!(
                "БД вузла {} недоступна для запису налаштувань: {e}",
                redact_url(database_url)
            )
        })?;
    sqlx::query("SELECT set_config('app.store_id', '', false)")
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("скидання store-скоупу не вдалось: {e}"))?;
    let auth = SqlxAuth::new(StorePool::new(pool.clone()));
    for (key, value) in [(HUB_URL_SETTING, hub_url), (HUB_TOKEN_SETTING, token)] {
        auth.settings_update_key(key, Some(value.to_string()))
            .await
            .map_err(|e| format!("запис {key} у БД вузла не вдався: {e}"))?;
    }
    // Контроль скоупу: обидва рядки мусять бути інстансними (store_id IS NULL).
    let rows: Vec<(String, Option<String>, Option<bool>)> = sqlx::query_as(
        "SELECT key, value, (store_id IS NULL) FROM system_settings \
         WHERE key = ANY($1)",
    )
    .bind(vec![HUB_URL_SETTING, HUB_TOKEN_SETTING])
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("перевірка записаних налаштувань не вдалась: {e}"))?;
    let url_row = rows.iter().find(|(k, _, _)| k == HUB_URL_SETTING);
    let token_row = rows.iter().find(|(k, _, _)| k == HUB_TOKEN_SETTING);
    match (url_row, token_row) {
        (Some((_, Some(v), Some(true))), Some((_, Some(_), Some(true)))) if v == hub_url => {
            Ok(format!(
                "sync.hub_url={v} і sync.hub_token записано (обидва рядки з store_id IS NULL — \
             інстансний скоуп, не скоуп точки)"
            ))
        }
        (Some((_, _, Some(false))), _) | (_, Some((_, _, Some(false)))) => Err(
            "ЗАПИС У STORE-СКОП: system_settings.store_id IS NOT NULL для ключа синку — \
             налаштування вузла не має належати точці (аномалія, сигнал угору)"
                .to_string(),
        ),
        _ => Err(
            "після запису синк-налаштувань не знайдено рядків (або значення порожні) — \
             БД вузла не прийняла конфігурацію"
                .to_string(),
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Юніт-тести чистих функцій
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_url_gets_scheme_and_loses_trailing_slash() {
        assert_eq!(
            normalize_hub_url("  192.168.0.10:8080/  "),
            Some("http://192.168.0.10:8080".to_string())
        );
        assert_eq!(
            normalize_hub_url("https://hub.example.com/"),
            Some("https://hub.example.com".to_string())
        );
        // хвіст-шлях зберігається (проксі з префіксом), але без кінцевого слеша
        assert_eq!(
            normalize_hub_url("hub.local/base/"),
            Some("http://hub.local/base".to_string())
        );
        // IPv6 у дужках — валідний хост
        assert_eq!(
            normalize_hub_url("[::1]:8080"),
            Some("http://[::1]:8080".to_string())
        );
    }

    #[test]
    fn invalid_hub_urls_are_rejected() {
        for bad in [
            "",
            "   ",
            "http://",
            "http:///api",
            "host:0",
            "host:99999",
            "host:abc",
            "ho st",
            "::1:8080",
        ] {
            assert_eq!(
                normalize_hub_url(bad),
                None,
                "мусить бути відкинуто: {bad:?}"
            );
        }
    }

    #[test]
    fn tail_lines_keeps_last_lines_only() {
        let text = (1..=30)
            .map(|i| format!("рядок {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = tail_lines(&text, STDERR_TAIL_LINES);
        assert_eq!(tail.lines().count(), STDERR_TAIL_LINES);
        assert!(tail.starts_with("рядок 11"));
        assert!(tail.ends_with("рядок 30"));
        assert_eq!(tail_lines("", 5), "");
    }

    #[test]
    fn pg_restore_args_are_exactly_the_contract_flags() {
        let args = pg_restore_args(
            "postgresql://postgres@127.0.0.1:5433/torgashka",
            Path::new("/tmp/a.dump"),
        );
        let as_str: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            as_str,
            vec![
                "--clean",
                "--if-exists",
                "--no-owner",
                "--no-privileges",
                "-d",
                "postgresql://postgres@127.0.0.1:5433/torgashka",
                "/tmp/a.dump"
            ]
        );
    }

    #[test]
    fn redact_url_hides_password_only() {
        assert_eq!(
            redact_url("postgresql://postgres:s3cret@127.0.0.1:5433/torgashka"),
            "postgresql://postgres:***@127.0.0.1:5433/torgashka"
        );
        assert_eq!(
            redact_url("postgresql://postgres@127.0.0.1:5433/torgashka"),
            "postgresql://postgres@127.0.0.1:5433/torgashka"
        );
        assert_eq!(redact_url("не-url"), "не-url");
    }

    #[test]
    fn dump_filename_cannot_escape_the_directory() {
        assert_eq!(
            safe_dump_filename(Some("pos_system_fresh.dump")),
            "pos_system_fresh.dump"
        );
        assert_eq!(safe_dump_filename(Some("../../etc/passwd")), "passwd");
        assert_eq!(safe_dump_filename(Some("   ")), DEFAULT_DUMP_FILENAME);
        assert_eq!(safe_dump_filename(None), DEFAULT_DUMP_FILENAME);
    }

    #[test]
    fn sha256_matches_known_digests() {
        // Порожній ввід — еталон FIPS 180-4.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn progress_status_words_are_frozen() {
        assert_eq!(ProgressStatus::Started.as_str(), "started");
        assert_eq!(ProgressStatus::Ok.as_str(), "ok");
        assert_eq!(ProgressStatus::Failed.as_str(), "failed");
    }
}
