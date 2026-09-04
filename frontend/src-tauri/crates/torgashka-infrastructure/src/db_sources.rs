//! db_sources — конфігурація «Джерело даних» (Етап 3 адмін-панелі власника,
//! ТЗ розділи 2.4 / 5.8).
//!
//! Машина сервера тримає локальний конфіг-файл `db_sources.toml` (права 0600),
//! зразок:
//!
//! ```toml
//! active = "primary"
//!
//! [sources.primary]
//! label = "Основна (сервер)"
//! host = "127.0.0.1"
//! port = 5432
//! database = "pos_system"
//! user = "postgres"
//! password_encrypted = "base64(nonce(12) || tag(16) || ciphertext)"
//!
//! [sources.backup_restore]
//! # …резервне джерело (призначення імпорту/відновлення дампу)
//! ```
//!
//! # Безпека пароля
//! `password_encrypted` — AES-256-GCM, ключ 32 байти з:
//!   1. env `TORGASHKA_DBKEY` (base64 43 симв. або hex 64 симв.);
//!   2. файлу `.dbkey` у тій самій директорії, що й `db_sources.toml` (права
//!      0600; створюється автоматично при першому збереженні пароля).
//! Ключ НЕ хардкодиться в коді/конфігах репозиторію. Пароль у файлі ніколи
//! не зберігається у plaintext.
//!
//! # Шляхи пошуку db_sources.toml (перший знайдений виграє)
//!   1. env `TORGASHKA_DB_SOURCES` (абсолютний шлях);
//!   2. `./db_sources.toml` (CWD);
//!   3. `<repo>/frontend/src-tauri/db_sources.toml` (dev-збірка, поряд із
//!      config.toml.example; CARGO_MANIFEST_DIR — лише compile-time fallback).
//! Запис (активація/CRUD) — завжди в перший кандидат списку (env → CWD →
//! manifest), незалежно від того, чи файл уже існує.
//!
//! # Стабільність (stability_first, рішення зафіксоване)
//! `active` — авторитетне джерело ПРИ СТАРТІ: `resolve_database_url()`
//! (db.rs) підставляє URL активного джерела. ГАРЯЧОГО перепідключення пулів
//! фасаду НЕМАЄ: пули створюються один раз при serve_listener (кожен сервіс
//! має власний пул/репозиторій в AppState) — атомарно замінити їх у рантаймі
//! неможливо без ризику. Тому `activate` у /admin/db-sources лише:
//!   1) перевіряє з'єднання з джерелом (TCP + SELECT 1);
//!   2) зберігає `active = <id>` у db_sources.toml;
//!   3) повертає чесну відповідь «застосується після перезапуску сервісу».
//! Жоден існуючий роут НЕ чіпає робочий пул під час перемикання.

use std::io::Write;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Env з ключем шифрування паролів (base64 або hex, 32 байти).
pub const DBKEY_ENV: &str = "TORGASHKA_DBKEY";
/// Env зі шляхом до db_sources.toml (пріоритет над CWD/manifest).
pub const DB_SOURCES_ENV: &str = "TORGASHKA_DB_SOURCES";

// ─────────────────────────────────────────────────────────────────────────────
// Помилки
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DbSourcesError {
    #[error("помилка файлової операції: {0}")]
    Io(#[from] std::io::Error),
    #[error("db_sources.toml не читається: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("db_sources.toml не серіалізується: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("помилка шифрування пароля: {0}")]
    Crypto(String),
    #[error("ключ шифрування не знайдено: {0}")]
    KeyMissing(String),
    #[error("{0}")]
    Other(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Модель файлу
// ─────────────────────────────────────────────────────────────────────────────

/// Один запис `[sources.<id>]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// base64(nonce(12) || tag(16) || ciphertext); відсутній/порожній = без пароля.
    #[serde(
        default,
        rename = "password_encrypted",
        skip_serializing_if = "Option::is_none"
    )]
    pub password_encrypted: Option<String>,
}

/// Вміст db_sources.toml. `sources` — Vec, щоб зберегти порядок таблиць
/// (томл-мапа без preserve_order сортує за ключем; нам важливий порядок
/// створення джерел у файлі).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DbSourcesFile {
    /// id активного джерела (ключ у [sources.*]); None = «не задано» → фасад
    /// стартує як раніше (env DATABASE_URL → backend/.env → embedded PG).
    pub active: Option<String>,
    pub sources: Vec<(String, DbSource)>,
}

/// Публічне представлення джерела для API/UI (ніколи не містить пароль).
#[derive(Debug, Clone, Serialize)]
pub struct DbSourceView {
    pub id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// Чи задано пароль (зберігається лише в password_encrypted, не видається).
    pub has_password: bool,
    /// Чи є це джерело активним.
    pub is_active: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Шляхи
// ─────────────────────────────────────────────────────────────────────────────

/// Кандидати шляху db_sources.toml у порядку пріоритету.
pub fn path_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var(DB_SOURCES_ENV) {
        if !p.trim().is_empty() {
            v.push(PathBuf::from(p));
        }
    }
    v.push(PathBuf::from("db_sources.toml")); // CWD
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // crates/torgashka-infrastructure → ../../db_sources.toml =
    // <repo>/frontend/src-tauri/db_sources.toml (dev поряд із config.toml.example).
    v.push(manifest.join("../../db_sources.toml"));
    v
}

/// Шлях для ЗАПИСУ (завжди перший кандидат: env → CWD → manifest).
pub fn write_path() -> PathBuf {
    path_candidates().into_iter().next().expect("кандидати")
}

/// Перший ІСНУЮЧИЙ файл конфігурації (None — файлу ще немає).
pub fn existing_path() -> Option<PathBuf> {
    path_candidates().into_iter().find(|p| p.is_file())
}

/// Директорія зберігання дампів (`<дир db_sources.toml>/dumps`).
pub fn dumps_dir() -> PathBuf {
    write_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("dumps")
}

// ─────────────────────────────────────────────────────────────────────────────
// Читання / запис (атомарно, права 0600)
// ─────────────────────────────────────────────────────────────────────────────

/// Завантажує конфігурацію. Немає файлу → Ok(None) (функціонал вимкнено).
pub fn load() -> Result<Option<DbSourcesFile>, DbSourcesError> {
    let Some(path) = existing_path() else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(&path)?;
    load_from_str(&content).map(Some)
}

/// Парсинг рядка toml (зберігає порядок [sources.*]).
pub fn load_from_str(content: &str) -> Result<DbSourcesFile, DbSourcesError> {
    let v: toml::Value = toml::from_str(content)?;
    let active = v
        .get("active")
        .and_then(|a| a.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let mut sources: Vec<(String, DbSource)> = Vec::new();
    if let Some(tbl) = v.get("sources").and_then(|s| s.as_table()) {
        for (name, sv) in tbl {
            // toml 0.8 не має публічного from_value для Value → типовуємо через
            // тимчасовий toml-текст таблиці (порядок ключів усередині неважливий).
            let tmp = toml::to_string(sv)?;
            let src: DbSource = toml::from_str(&tmp)?;
            sources.push((name.clone(), src));
        }
    }
    Ok(DbSourcesFile { active, sources })
}

/// Запис файлу: temp + rename (атомарно), права 0600 (unix).
pub fn save(cfg: &DbSourcesFile) -> Result<PathBuf, DbSourcesError> {
    let path = write_path();
    save_to(&path, cfg)?;
    Ok(path)
}

/// Запис у конкретний шлях (атомарно; 0600). Використовується тестами.
pub fn save_to(path: &Path, cfg: &DbSourcesFile) -> Result<(), DbSourcesError> {
    let text = to_string(cfg)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension(format!("toml.tmp{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    set_private_file(&tmp)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Серіалізація DbSourcesFile у toml-текст (порядок джерел збережено).
pub fn to_string(cfg: &DbSourcesFile) -> Result<String, DbSourcesError> {
    let mut root = toml::map::Map::new();
    if let Some(a) = &cfg.active {
        root.insert("active".into(), toml::Value::String(a.clone()));
    }
    let mut sources = toml::map::Map::new();
    for (name, src) in &cfg.sources {
        sources.insert(name.clone(), toml::Value::try_from(src)?);
    }
    root.insert("sources".into(), toml::Value::Table(sources));
    let pretty = toml::to_string_pretty(&toml::Value::Table(root))?;
    Ok(pretty)
}

/// Права 0600 (unix). Windows — no-op (права керуються ACL ОС).
#[cfg(unix)]
fn set_private_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Шифрування пароля (AES-256-GCM)
// ─────────────────────────────────────────────────────────────────────────────

/// Шлях до ключового файлу: `.dbkey` поруч із db_sources.toml.
fn key_file_for(cfg_path: &Path) -> PathBuf {
    cfg_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".dbkey")
}

fn decode_key_str(raw: &str) -> Result<[u8; 32], DbSourcesError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(DbSourcesError::KeyMissing(
            "TORGASHKA_DBKEY порожній".to_string(),
        ));
    }
    if let Ok(b) = B64.decode(s) {
        if let Ok(arr) = <[u8; 32]>::try_from(b.as_slice()) {
            return Ok(arr);
        }
    }
    if s.len() == 64 {
        let mut arr = [0u8; 32];
        for (i, c) in s.chars().enumerate() {
            let d = c
                .to_digit(16)
                .ok_or_else(|| DbSourcesError::KeyMissing(format!("ключ не валідний hex: {s}")))?;
            if i % 2 == 0 {
                arr[i / 2] = (d as u8) << 4;
            } else {
                arr[i / 2] |= d as u8;
            }
        }
        return Ok(arr);
    }
    Err(DbSourcesError::KeyMissing(format!(
        "ключ має бути 32 байти (base64 ≈43 симв. або hex 64 симв.), отримано {} симв.",
        s.len()
    )))
}

/// Ключ: env TORGASHKA_DBKEY → інакше вміст `.dbkey` (поруч із конфігом).
pub fn resolve_key(cfg_path: &Path) -> Result<[u8; 32], DbSourcesError> {
    if let Ok(raw) = std::env::var(DBKEY_ENV) {
        if !raw.trim().is_empty() {
            return decode_key_str(&raw);
        }
    }
    let kf = key_file_for(cfg_path);
    match std::fs::read_to_string(&kf) {
        Ok(raw) => decode_key_str(&raw),
        Err(e) => Err(DbSourcesError::KeyMissing(format!(
            "файл ключа {} не читається ({e}); задайте env {} або створіть .dbkey (права 0600)",
            kf.display(),
            DBKEY_ENV
        ))),
    }
}

/// Гарантує наявність ключа для ЗАПИСУ: env ключ → .dbkey існує → інакше
/// генерує 32 випадкові байти у .dbkey (0600).
pub fn ensure_key(cfg_path: &Path) -> Result<[u8; 32], DbSourcesError> {
    if let Ok(raw) = std::env::var(DBKEY_ENV) {
        if !raw.trim().is_empty() {
            return decode_key_str(&raw);
        }
    }
    let kf = key_file_for(cfg_path);
    if kf.is_file() {
        return resolve_key(cfg_path);
    }
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    let b64 = B64.encode(key);
    if let Some(parent) = kf.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut f = std::fs::File::create(&kf)?;
    f.write_all(b64.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    set_private_file(&kf)?;
    Ok(key)
}

/// Шифрує пароль: base64(nonce(12) || ciphertext+tag(16)).
pub fn encrypt_password(cfg_path: &Path, plain: &str) -> Result<String, DbSourcesError> {
    ensure_key(cfg_path)?;
    let key = resolve_key(cfg_path)?;
    let cipher = Aes256Gcm::new((&key).into());
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
        .map_err(|e| DbSourcesError::Crypto(format!("AES-256-GCM encrypt: {e}")))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(B64.encode(out))
}

/// Розшифровує пароль із password_encrypted.
pub fn decrypt_password(cfg_path: &Path, encrypted: &str) -> Result<String, DbSourcesError> {
    let key = resolve_key(cfg_path)?;
    let raw = B64
        .decode(encrypted)
        .map_err(|e| DbSourcesError::Crypto(format!("base64 password_encrypted: {e}")))?;
    if raw.len() < 12 + 16 {
        return Err(DbSourcesError::Crypto(
            "password_encrypted закороткий (очікується nonce(12)+tag(16)+ct)".to_string(),
        ));
    }
    let cipher = Aes256Gcm::new((&key).into());
    let (nonce, ct) = raw.split_at(12);
    let plain = cipher.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| {
        DbSourcesError::Crypto("не вдалося розшифрувати (невірний ключ?)".to_string())
    })?;
    String::from_utf8(plain)
        .map_err(|_| DbSourcesError::Crypto("розшифрований пароль не UTF-8".to_string()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Побудова URL підключення
// ─────────────────────────────────────────────────────────────────────────────

fn pct(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// postgresql:// URL джерела з РОЗШИФРОВАНИМ паролем (для sqlx / pg_*).
/// Пароль НЕ з'являється у файлі конфігурації — лише в пам'яті процесу та
/// (при виклику pg_*) в env PGPASSWORD дочірнього процесу (не в argv).
pub fn build_url(src: &DbSource, plain_password: &str) -> String {
    let host = if src.host.contains(':') && !src.host.starts_with('[') {
        format!("[{}]", src.host)
    } else {
        src.host.clone()
    };
    if plain_password.is_empty() {
        format!(
            "postgresql://{}@{}/{}",
            pct(&src.user),
            host,
            pct(&src.database)
        )
    } else {
        format!(
            "postgresql://{}:{}@{}/{}",
            pct(&src.user),
            pct(plain_password),
            host,
            pct(&src.database)
        )
    }
}

/// URL активного джерела (для resolve_database_url при старті).
///
/// - файлу/active немає → Ok(None): фасад працює як раніше;
/// - active задано → розшифровує пароль і повертає Ok(Some(url));
/// - помилка конфігурації/ключа → Err (чесна зупинка, а не мовчазний fallback).
pub fn active_source_url() -> Result<Option<String>, DbSourcesError> {
    let Some(cfg) = load()? else {
        return Ok(None);
    };
    let Some(active) = &cfg.active else {
        return Ok(None);
    };
    let (_, src) = cfg
        .sources
        .iter()
        .find(|(id, _)| id == active)
        .ok_or_else(|| {
            DbSourcesError::Other(format!(
                "active = \"{active}\", але джерела з таким id немає у db_sources.toml"
            ))
        })?;
    let path = existing_path().ok_or_else(|| {
        DbSourcesError::Other("db_sources.toml зник між читанням і побудовою URL".to_string())
    })?;
    let pw = match &src.password_encrypted {
        Some(enc) if !enc.is_empty() => decrypt_password(&path, enc)?,
        _ => String::new(),
    };
    Ok(Some(build_url(src, &pw)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Пошук pg-бінарників (pg_dump/pg_restore) у PATH
// ─────────────────────────────────────────────────────────────────────────────

/// Шукає виконуваний файл у PATH. Err — людська підказка встановити клієнт.
pub fn find_binary(name: &str) -> Result<PathBuf, DbSourcesError> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        let cand = dir.join(name);
        if cand.is_file() {
            return Ok(cand);
        }
        #[cfg(windows)]
        {
            let cand_exe = dir.join(format!("{name}.exe"));
            if cand_exe.is_file() {
                return Ok(cand_exe);
            }
        }
    }
    Err(DbSourcesError::Other(format!(
        "бінарник '{name}' не знайдено в PATH — встановіть PostgreSQL client tools (pg_dump/pg_restore), щоб користуватись дампами"
    )))
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести (без БД)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cfg() -> DbSourcesFile {
        DbSourcesFile {
            active: Some("primary".to_string()),
            sources: vec![
                (
                    "primary".to_string(),
                    DbSource {
                        label: Some("Основна".to_string()),
                        host: "127.0.0.1".to_string(),
                        port: 5432,
                        database: "pos_system".to_string(),
                        user: "postgres".to_string(),
                        password_encrypted: None,
                    },
                ),
                (
                    "backup_restore".to_string(),
                    DbSource {
                        label: Some("Резерв/відновлення".to_string()),
                        host: "10.0.0.5".to_string(),
                        port: 5432,
                        database: "torgashka_dump".to_string(),
                        user: "backup".to_string(),
                        password_encrypted: None,
                    },
                ),
            ],
        }
    }

    /// Записує .dbkey (base64) у директорію конфіга — без env-глобального стану.
    fn write_key(dir: &Path, key_byte: u8) {
        let kf = dir.join(".dbkey");
        let key = [key_byte; 32];
        std::fs::write(&kf, B64.encode(key)).expect("write .dbkey");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&kf, std::fs::Permissions::from_mode(0o600));
        }
    }

    #[test]
    fn toml_roundtrip_preserves_source_order_and_active() {
        let text = to_string(&sample_cfg()).expect("serialize");
        assert!(text.contains("active = \"primary\""), "{text}");
        assert!(text.contains("[sources.primary]"), "{text}");
        assert!(text.contains("[sources.backup_restore]"), "{text}");
        // Порядок таблиць = порядок у Vec.
        assert!(
            text.find("[sources.primary]").unwrap()
                < text.find("[sources.backup_restore]").unwrap(),
            "порядок [sources.*] має зберігатись"
        );
        let parsed = load_from_str(&text).expect("parse");
        assert_eq!(parsed, sample_cfg());
    }

    #[test]
    fn parse_empty_str_no_active_no_sources() {
        let parsed = load_from_str("").expect("parse empty");
        assert_eq!(parsed.active, None);
        assert!(parsed.sources.is_empty());
    }

    #[test]
    fn save_creates_0600_and_no_plaintext_password() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_key(dir.path(), 7u8);
        let cfg_path = dir.path().join("db_sources.toml");
        let enc = encrypt_password(&cfg_path, "S3cret!@:pass").expect("encrypt");
        let mut cfg = sample_cfg();
        cfg.sources[0].1.password_encrypted = Some(enc.clone());
        cfg.sources[1].1.password_encrypted =
            Some(encrypt_password(&cfg_path, "").expect("enc empty"));
        save_to(&cfg_path, &cfg).expect("save");

        let raw = std::fs::read_to_string(&cfg_path).expect("read");
        assert!(
            !raw.contains("S3cret!@:pass"),
            "пароль не має бути у файлі у plaintext: {raw}"
        );
        assert!(raw.contains("password_encrypted"), "{raw}");
        assert!(raw.contains(&enc), "має зберігатись шифроване значення");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&cfg_path)
                .expect("meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                mode, 0o600,
                "db_sources.toml має бути 0600, отримано {mode:o}"
            );
            let key_path = cfg_path.parent().unwrap().join(".dbkey");
            let k_mode = std::fs::metadata(&key_path)
                .expect("key meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(k_mode, 0o600, ".dbkey має бути 0600, отримано {k_mode:o}");
        }
        // Розшифровка назад дає оригінал.
        assert_eq!(
            decrypt_password(&cfg_path, &enc).expect("decrypt"),
            "S3cret!@:pass"
        );
        // Порожній пароль теж коректно шифрується/розшифровується.
        let enc2 = cfg.sources[1]
            .1
            .password_encrypted
            .as_deref()
            .expect("enc2");
        assert_eq!(decrypt_password(&cfg_path, enc2).expect("dec2"), "");
    }

    #[test]
    fn encrypt_decrypt_wrong_key_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("db_sources.toml");
        write_key(dir.path(), 1u8);
        let enc = encrypt_password(&cfg_path, "пароль123").expect("encrypt");
        write_key(dir.path(), 2u8); // ключ змінився → розшифрування неможливе
        let err = decrypt_password(&cfg_path, &enc).expect_err("має впасти");
        assert!(err.to_string().contains("невірний ключ"), "{err}");
    }

    #[test]
    fn ensure_key_auto_creates_and_is_reused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("db_sources.toml");
        // env-ключ НЕ задано → ensure_key створює .dbkey.
        let k1 = ensure_key(&cfg_path).expect("ensure");
        let k2 = resolve_key(&cfg_path).expect("resolve");
        assert_eq!(k1, k2, "ключ має лишитись стабільним між викликами");
        let kf = dir.path().join(".dbkey");
        assert!(kf.is_file(), ".dbkey створено");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&kf).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, ".dbkey 0600, отримано {mode:o}");
        }
    }

    #[test]
    fn build_url_percent_encodes_user_and_password() {
        let src = DbSource {
            label: None,
            host: "db.internal".to_string(),
            port: 5433,
            database: "mydb".to_string(),
            user: "u:ser".to_string(),
            password_encrypted: None,
        };
        let url = build_url(&src, "p@ss/word");
        assert_eq!(url, "postgresql://u%3Aser:p%40ss%2Fword@db.internal/mydb");
        // IPv6 host — у квадратних дужках.
        let v6 = DbSource {
            host: "::1".to_string(),
            ..src
        };
        assert_eq!(build_url(&v6, ""), "postgresql://u%3Aser@[::1]/mydb");
    }
}
