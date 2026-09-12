//! R1 (ADR-0008, «Варіант B»): ХАБ віддає ЗНІМОК власної БД вузлу.
//!
//! `GET /api/v1/sync/snapshot` — найновіший `*.dump` з каталогу знімків, як
//! `application/octet-stream`, разом із sha256 і розміром. Це ДЖЕРЕЛО для
//! провіжну вузла (контракт R2): вузол забирає знімок і відновлює з нього
//! власну БД, не читаючи нічого з ФС хаба напряму.
//!
//! ЧОМУ БЕЗ StoreCtx (X-Store-Id): знімок — це ВСЯ БД, а не дані однієї
//! торговельної точки. Store-скоуп тут не лише зайвий, а й хибний: він
//! створив би враження, що відповідь залежить від точки (і вимагав би
//! заголовка, якого при провіжні вузла немає).
//!
//! ЧОМУ БЕЗ `State(state)`: хендлер читає файл з диска — жодного пула чи
//! репозиторія не потребує. Тому знімок віддається навіть тоді, коли БД хаба
//! ще не піднята, — а це саме той момент, коли він найпотрібніший.
//!
//! Розмір і хеш рахуються з ТИХ САМИХ байтів, які їдуть у тілі відповіді
//! (одне читання файла): «приблизний» розмір або хеш окремого проходу описував
//! би іншу версію файла, ніж отримав вузол.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use axum::{
    body::Body,
    extract::Extension,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::auth::Claims;
use crate::sync::SyncError;

/// Env із каталогом знімків (абсолютний або відносний шлях).
pub const SNAPSHOT_DIR_ENV: &str = "TORGASHKA_HUB_SNAPSHOT_DIR";

/// Каталог знімків за замовчуванням — від CWD процесу фасаду.
pub const DEFAULT_SNAPSHOT_DIR: &str = "artifacts/hub_snapshot";

/// Розширення файлів-знімків. Інші файли каталогу (`MANIFEST.md`,
/// `verify.log`) не є знімками й ігноруються.
pub const SNAPSHOT_EXTENSION: &str = "dump";

/// Ролі, яким дозволено забирати ЗНІМОК УСІЄЇ БД.
///
/// `device` — вузол мережі (провіжн себе), `admin`/`owner` — оператор.
/// Інші ролі (`cashier`, `store_manager`) → 403: знімок містить дані ВСІХ
/// точок мережі, тож роль однієї точки його не отримує.
pub const ALLOWED_ROLES: &[&str] = &["device", "admin", "owner"];

/// Каталог знімків: `TORGASHKA_HUB_SNAPSHOT_DIR`, інакше
/// [`DEFAULT_SNAPSHOT_DIR`] від CWD. Порожній env = не задано.
pub fn snapshot_dir() -> PathBuf {
    match std::env::var(SNAPSHOT_DIR_ENV) {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => PathBuf::from(DEFAULT_SNAPSHOT_DIR),
    }
}

/// Найновіший `*.dump` у каталозі за mtime.
///
/// Помилки: 404 — каталогу немає або в ньому немає жодного `*.dump` (у `detail`
/// завжди ФАКТИЧНИЙ шлях: оператор має бачити, куди дивився хаб); 500 — каталог
/// існує, але не читається (права, битий том).
pub fn newest_dump(dir: &Path) -> Result<PathBuf, SyncError> {
    let entries = std::fs::read_dir(dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            SyncError::NotFound(format!("каталог знімків не знайдено: {}", dir.display()))
        }
        _ => SyncError::Io(format!(
            "каталог знімків не читається: {} ({e})",
            dir.display()
        )),
    })?;

    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in entries {
        let entry = entry.map_err(|e| {
            SyncError::Io(format!(
                "каталог знімків не читається: {} ({e})",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some(SNAPSHOT_EXTENSION) {
            continue;
        }
        // mtime невідомий (дивний том/ФС) → трактуємо як найстаріший, щоб один
        // такий файл не перекрив живий знімок. Не мовчимо про помилку метаданих:
        // вона не заважає віддати інший, коректний знімок.
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        match &newest {
            Some((best, _)) if *best >= modified => {}
            _ => newest = Some((modified, path)),
        }
    }

    newest.map(|(_, path)| path).ok_or_else(|| {
        SyncError::NotFound(format!(
            "у каталозі немає жодного *.{SNAPSHOT_EXTENSION}: {}",
            dir.display()
        ))
    })
}

/// sha256 у hex (нижній регістр). Своя реалізація — щоб не тягнути крейт
/// `hex` заради 16 рядків.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// `GET /api/v1/sync/snapshot` — тіло відповіді = найновіший знімок БД.
///
/// 200: `Content-Type: application/octet-stream`, `Content-Length`,
/// `Content-Disposition`, `X-Snapshot-Sha256`, `X-Snapshot-Filename`.
/// 403 — роль не дозволена; 404 — каталог/знімок відсутній; 500 — IO.
pub async fn download(Extension(claims): Extension<Claims>) -> Result<Response, SyncError> {
    if !ALLOWED_ROLES.contains(&claims.role.as_str()) {
        return Err(SyncError::Forbidden(format!(
            "Доступ заборонено: знімок БД доступний ролям {}; ваша роль — «{}»",
            ALLOWED_ROLES.join("|"),
            claims.role
        )));
    }

    let dir = snapshot_dir();
    let path = newest_dump(&dir)?;

    // Одне читання: і тіло відповіді, і хеш, і розмір — з цих самих байтів.
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| SyncError::Io(format!("знімок не читається: {} ({e})", path.display())))?;
    let sha256 = sha256_hex(&bytes);
    let length = bytes.len();
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            SyncError::Io(format!(
                "ім'я знімка не є коректним UTF-8: {}",
                path.display()
            ))
        })?
        .to_string();
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|e| {
            SyncError::Io(format!(
                "ім'я знімка непридатне для заголовка Content-Disposition: {filename} ({e})"
            ))
        })?;
    let filename_header = HeaderValue::from_str(&filename)
        .map_err(|e| SyncError::Io(format!("ім'я знімка непридатне для заголовка: {e}")))?;
    let sha_header = HeaderValue::from_str(&sha256)
        .map_err(|e| SyncError::Io(format!("хеш непридатний для заголовка: {e}")))?;

    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    // Content-Length ставимо ЯВНО (а не покладаємось на оцінку тіла):
    // контракт R2 читає його як авторитетний розмір знімка.
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string())
            .map_err(|e| SyncError::Io(format!("розмір непридатний для заголовка: {e}")))?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(header::CONTENT_DISPOSITION, disposition);
    headers.insert(
        axum::http::HeaderName::from_static("x-snapshot-sha256"),
        sha_header,
    );
    headers.insert(
        axum::http::HeaderName::from_static("x-snapshot-filename"),
        filename_header,
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use serde_json::Value;
    use std::fs::File;
    use std::time::Duration;

    /// Env-змінна процесу глобальна → тести, які її чіпають, серіалізуємо.
    static ENV_SEQ: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Хеш і розмір РЕАЛЬНОГО артефакта хаба (перевірено `sha256sum`/`stat` у
    /// репо: artifacts/hub_snapshot/pos_system_fresh_20260912.dump).
    const REAL_SHA256: &str = "3d2d17c2f1592dccd39d824da047cbf5c07f9e4ac50a19a7fade75616fabb25a";
    const REAL_LENGTH: usize = 716167;
    const REAL_FILENAME: &str = "pos_system_fresh_20260912.dump";

    /// Ставить `TORGASHKA_HUB_SNAPSHOT_DIR` на час тесту й повертає попереднє
    /// значення (тести процесу — один на одного).
    struct EnvGuard {
        previous: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(dir: &Path) -> Self {
            let lock = ENV_SEQ.lock().unwrap_or_else(|e| e.into_inner());
            let previous = std::env::var(SNAPSHOT_DIR_ENV).ok();
            std::env::set_var(SNAPSHOT_DIR_ENV, dir);
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => std::env::set_var(SNAPSHOT_DIR_ENV, previous),
                None => std::env::remove_var(SNAPSHOT_DIR_ENV),
            }
        }
    }

    fn claims_for(role: &str) -> Claims {
        Claims {
            sub: "00000000-0000-0000-0000-0000000000ff".to_string(),
            role: role.to_string(),
            permissions: None,
            token_type: "access".to_string(),
            iat: 0,
            exp: usize::MAX,
        }
    }

    fn write_at(dir: &Path, name: &str, mtime_secs: u64) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"dump-inhalt").expect("запис файла");
        let file = File::options()
            .write(true)
            .open(&path)
            .expect("відкриття файла");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_secs))
            .expect("mtime");
        path
    }

    async fn body_of(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("тіло відповіді")
            .to_vec()
    }

    fn header<'a>(response: &'a Response, name: &str) -> &'a str {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| panic!("немає заголовка {name}"))
    }

    /// Каталог реального артефакта хаба (репо: <корінь>/artifacts/hub_snapshot).
    fn real_artifact_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../artifacts/hub_snapshot")
    }

    #[test]
    fn snapshot_dir_uses_env_and_defaults_when_unset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::set(dir.path());
        assert_eq!(snapshot_dir(), dir.path());

        std::env::remove_var(SNAPSHOT_DIR_ENV);
        assert_eq!(snapshot_dir(), PathBuf::from(DEFAULT_SNAPSHOT_DIR));

        // Порожній env = не задано (не «корінь ФС»).
        std::env::set_var(SNAPSHOT_DIR_ENV, "   ");
        assert_eq!(snapshot_dir(), PathBuf::from(DEFAULT_SNAPSHOT_DIR));
    }

    #[test]
    fn newest_dump_wins_by_mtime_and_non_dump_files_are_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let older = write_at(dir.path(), "older.dump", 1_600_000_000);
        let newest = write_at(dir.path(), "newest.dump", 1_700_000_000);
        // Найновіші за mtime, але НЕ знімки — не мають впливати на вибір.
        write_at(dir.path(), "MANIFEST.md", 1_800_000_000);
        write_at(dir.path(), "verify.log", 1_900_000_000);

        let picked = newest_dump(dir.path()).expect("знімок знайдено");
        assert_eq!(picked, newest);
        assert_ne!(picked, older);
        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some("newest.dump")
        );
    }

    #[tokio::test]
    async fn missing_dir_is_404_with_actual_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("snapshots-nope");
        let _guard = EnvGuard::set(&missing);

        let error = download(Extension(claims_for("admin")))
            .await
            .expect_err("404 на неіснуючий каталог");
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = body_of(response).await;
        let json: Value = serde_json::from_slice(&body).expect("JSON тіла 404");
        let detail = json["detail"].as_str().expect("detail");
        assert!(
            detail.contains(&missing.display().to_string()),
            "detail без фактичного шляху: {detail}"
        );
    }

    #[tokio::test]
    async fn empty_dir_is_404_with_actual_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::set(dir.path());
        // Каталог непорожній, але знімків у ньому немає (як verify.log).
        write_at(dir.path(), "verify.log", 1_900_000_000);

        let error = download(Extension(claims_for("admin")))
            .await
            .expect_err("404 на каталог без *.dump");
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = body_of(response).await;
        let json: Value = serde_json::from_slice(&body).expect("JSON тіла 404");
        let detail = json["detail"].as_str().expect("detail");
        assert!(
            detail.contains(&dir.path().display().to_string()),
            "detail без фактичного шляху: {detail}"
        );
        assert!(detail.contains("dump"), "detail без розширення: {detail}");
    }

    #[tokio::test]
    async fn download_serves_real_artifact_with_exact_sha256_and_length() {
        let dir = real_artifact_dir();
        let file = dir.join(REAL_FILENAME);
        assert!(
            file.is_file(),
            "немає реального артефакта хаба: {}",
            file.display()
        );
        let _guard = EnvGuard::set(&dir);

        let response = download(Extension(claims_for("device")))
            .await
            .expect("200 на реальному знімку");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            header(&response, "content-type"),
            "application/octet-stream"
        );
        assert_eq!(header(&response, "content-length"), REAL_LENGTH.to_string());
        assert_eq!(header(&response, "x-snapshot-sha256"), REAL_SHA256);
        assert_eq!(header(&response, "x-snapshot-filename"), REAL_FILENAME);
        assert_eq!(
            header(&response, "content-disposition"),
            format!("attachment; filename=\"{REAL_FILENAME}\"")
        );

        let body = body_of(response).await;
        assert_eq!(
            body.len(),
            REAL_LENGTH,
            "тіло не збігається з Content-Length"
        );
        // Хеш рахуємо З ТІЛА відповіді — доводить, що sha256 у заголовку
        // описує саме ті байти, які отримав клієнт.
        assert_eq!(sha256_hex(&body), REAL_SHA256);
    }

    #[tokio::test]
    async fn allowed_roles_get_the_file_others_get_403() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_at(dir.path(), "snapshot.dump", 1_700_000_000);
        let _guard = EnvGuard::set(dir.path());

        for role in ALLOWED_ROLES {
            let response = download(Extension(claims_for(role)))
                .await
                .unwrap_or_else(|e| panic!("роль {role} мусить мати доступ: {e:?}"));
            assert_eq!(response.status(), StatusCode::OK, "роль {role}");
        }

        for role in ["cashier", "store_manager", ""] {
            let error = download(Extension(claims_for(role)))
                .await
                .expect_err("403 для ролі без права на знімок");
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "роль {role}");
            let body = body_of(response).await;
            let json: Value = serde_json::from_slice(&body).expect("JSON тіла 403");
            let detail = json["detail"].as_str().expect("detail");
            // Пояснення, ЯКОЇ ролі бракує, і яка роль у того, хто стукає.
            assert!(
                detail.contains("device"),
                "detail без дозволених ролей: {detail}"
            );
            if !role.is_empty() {
                assert!(detail.contains(role), "detail без фактичної ролі: {detail}");
            }
        }
    }

    #[test]
    fn sha256_hex_matches_known_digest_of_real_file() {
        let bytes = std::fs::read(real_artifact_dir().join(REAL_FILENAME))
            .expect("реальний артефакт читається");
        assert_eq!(bytes.len(), REAL_LENGTH);
        assert_eq!(sha256_hex(&bytes), REAL_SHA256);
        // Порожній ввід — відомий дайджест (не «нулі», а справжній sha256).
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
