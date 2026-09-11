//! Локальний JWT-секрет вузла — last-resort джерело (інстальована каса).
//!
//! Порядок резолву JWT-секрету (`torgashka-api::auth::resolve_jwt_secret`):
//! 1. env `TORGASHKA_JWT_SECRET` — явне перевизначення;
//! 2. `backend/.env` → `SECRET_KEY` — паритет із Python-бекендом (dev/сервер);
//! 3. **цей модуль** — `<data_dir>/jwt_secret.key`, поряд із `pgdata`.
//!
//! Навіщо локальний файл: інстальована каса (Windows, `%APPDATA%\Torgashka`)
//! не має `backend/.env`, а env `TORGASHKA_JWT_SECRET` ніхто не задає →
//! `bootstrap: крок 4` падає з `AuthError::MissingSecret`, і фасад вічно
//! віддає 503. За архітектурою JWT вузла локальний (heartbeat/join/sync
//! авторизуються `node_token`, user-JWT ніколи не валідується на іншому
//! вузлі), тож секрет генерується один раз і живе поряд із даними вузла.
//!
//! ⚠️ Значення секрету НІКОЛИ не логується: ні в `eprintln!`, ні в тексти
//! помилок, ні в повідомлення assert-ів.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Ім'я файлу секрету в каталозі даних вузла.
const SECRET_FILE_NAME: &str = "jwt_secret.key";
/// Мінімальна прийнятна довжина наявного секрету (коротший → `Err`).
const MIN_SECRET_LEN: usize = 16;
/// Кількість випадкових байт нового секрету (48 байт → 96 hex-символів).
const SECRET_BYTES: usize = 48;

/// Шлях до локального файлу секрету: `<data_dir>/jwt_secret.key`, де
/// `data_dir` — каталог кластера ([`crate::embedded_pg::data_dir_default`]),
/// тобто файл лежить ПОРЯД із `pgdata`, а не всередині нього.
///
/// Порожній parent (data_dir без каталогу-батька) → `jwt_secret.key` у CWD.
pub fn secret_path() -> PathBuf {
    match crate::embedded_pg::data_dir_default().parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(SECRET_FILE_NAME),
        _ => PathBuf::from(SECRET_FILE_NAME),
    }
}

/// Читає локальний секрет вузла, а якщо його ще немає — створює (48 випадкових
/// байт → lowercase-hex, 96 символів) у [`secret_path()`].
///
/// `Err` — наявний файл непридатний (закороткий/не читається) або запис не
/// вдався. Придатний наявний секрет НІКОЛИ не перезаписується.
pub fn load_or_create() -> Result<String, String> {
    load_or_create_at(&secret_path())
}

/// Ядро (тестоване на явному шляху): «створи, якщо немає» з захистом від
/// втрати чужого секрету.
pub fn load_or_create_at(path: &Path) -> Result<String, String> {
    let mut replace_empty = false;
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("локальний JWT-секрет {} не читається: {e}", path.display()))?;
        let trimmed = content.trim();
        if trimmed.len() >= MIN_SECRET_LEN {
            return Ok(trimmed.to_string());
        }
        if !trimmed.is_empty() {
            // Не перезаписуємо мовчки: можливо, це чужий/зіпсований секрет —
            // вирішує оператор.
            return Err(format!(
                "локальний JWT-секрет {} закороткий: {} симв. (мінімум {MIN_SECRET_LEN}) — \
                 файл НЕ перезаписано, виправте вручну",
                path.display(),
                trimmed.len()
            ));
        }
        // Порожній файл / лише пробіли — секрету там немає, тож перезаписуємо.
        replace_empty = true;
    }
    create(path, replace_empty)
}

/// Генерує й атомарно записує новий секрет.
///
/// `replace_empty = false` — `create if missing` (`persist_noclobber`, чужого
/// секрету не затираємо); `true` — у файлі вже лежить порожній/пробільний
/// вміст (секрету немає), тож заміна атомарним `rename` безпечна.
fn create(path: &Path, replace_empty: bool) -> Result<String, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("каталог {} не створити: {e}", parent.display()))?;
        }
    }
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let secret = random_hex(SECRET_BYTES);
    // Щонайбільше 2 спроби: друга — атомарна заміна порожнього файлу, який
    // з'явився гонкою між перевіркою й persist.
    for attempt in 0..2 {
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)
            .map_err(|e| format!("тимчасовий файл у {} не створити: {e}", dir.display()))?;
        tmp.write_all(secret.as_bytes())
            .and_then(|()| tmp.write_all(b"\n"))
            .and_then(|()| tmp.flush())
            .map_err(|e| format!("секрет {} не записати: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("права 0600 на {} не встановити: {e}", path.display()))?;
        }
        let overwrite = replace_empty || attempt > 0;
        let res: Result<(), std::io::Error> = if overwrite {
            tmp.persist(path).map(|_| ()).map_err(|e| e.error)
        } else {
            tmp.persist_noclobber(path).map(|_| ()).map_err(|e| e.error)
        };
        match res {
            Ok(()) => return Ok(secret),
            // Гонка: паралельний процес уже створив файл — чужого секрету не
            // затираємо, повертаємо наявний.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.exists() => {
                let content = std::fs::read_to_string(path).map_err(|e| {
                    format!("локальний JWT-секрет {} не читається: {e}", path.display())
                })?;
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
                // Порожній — повторюємо спробу з атомарною заміною.
            }
            Err(e) => return Err(format!("секрет {} не зберегти: {e}", path.display())),
        }
    }
    Err(format!(
        "секрет {} не зберегти: файл лишається порожнім",
        path.display()
    ))
}

/// `n` випадкових байт → lowercase-hex (2n символів); CSPRNG (`thread_rng`).
fn random_hex(n: usize) -> String {
    use rand::RngCore;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(n * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Тимчасовий шлях `<tmp>/Torgashka/jwt_secret.key` (як прод-розкладка).
    fn tmp_key() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Torgashka").join(SECRET_FILE_NAME);
        (dir, path)
    }

    /// (а) Свіжий шлях: 96 hex-символів, файл існує, вміст = повернутому.
    #[test]
    fn creates_new_secret_96_hex_and_returns_it() {
        let (_d, path) = tmp_key();
        let secret = load_or_create_at(&path).expect("секрет створено");
        assert_eq!(secret.len(), 96, "48 випадкових байт → 96 hex-символів");
        assert!(
            secret.bytes().all(|b| b.is_ascii_hexdigit()),
            "лише hex-символи"
        );
        assert!(
            secret.bytes().all(|b| !b.is_ascii_uppercase()),
            "lowercase-hex"
        );
        assert!(path.is_file(), "файл створено: {}", path.display());
        let on_disk = std::fs::read_to_string(&path).expect("read secret file");
        assert!(
            on_disk.trim() == secret,
            "вміст файлу = поверненому значенню"
        );
    }

    /// (б) Повторний виклик → те саме значення (файл не перезаписується).
    #[test]
    fn second_call_returns_same_secret() {
        let (_d, path) = tmp_key();
        let first = load_or_create_at(&path).expect("перший виклик");
        let second = load_or_create_at(&path).expect("другий виклик");
        assert!(first == second, "секрет стабільний між викликами");
    }

    /// Наявний придатний секрет не переписується (mtime не змінюється).
    #[test]
    fn existing_secret_is_not_rewritten() {
        let (_d, path) = tmp_key();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let own = "f".repeat(32);
        std::fs::write(&path, format!("{own}\n")).expect("write own secret");
        let mtime_before = std::fs::metadata(&path)
            .expect("meta")
            .modified()
            .expect("mtime");

        let got = load_or_create_at(&path).expect("наявний секрет прочитано");
        assert!(got == own, "повернуто наявний секрет (не згенерований)");

        let mtime_after = std::fs::metadata(&path)
            .expect("meta")
            .modified()
            .expect("mtime");
        assert!(mtime_before == mtime_after, "файл не перезаписано");
    }

    /// (в) Порожній файл / лише пробіли → регенеровано, непорожній.
    #[test]
    fn empty_or_whitespace_file_is_regenerated() {
        let (_d, path) = tmp_key();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");

        std::fs::write(&path, "").expect("write empty");
        let s1 = load_or_create_at(&path).expect("порожній файл → новий секрет");
        assert_eq!(s1.len(), 96, "згенеровано повний секрет");

        std::fs::write(&path, "   \n\t").expect("write whitespace");
        let s2 = load_or_create_at(&path).expect("лише пробіли → новий секрет");
        assert_eq!(s2.len(), 96, "згенеровано повний секрет");
        assert!(
            s1 != s2,
            "порожній вміст → перегенерація, не той самий секрет"
        );

        let s3 = load_or_create_at(&path).expect("стабільність після регенерації");
        assert!(s2 == s3, "після запису секрет стабільний");
    }

    /// (г) Файл `"abc"` (3 симв.) → `Err`, файл НЕ перезаписано.
    #[test]
    fn too_short_file_is_error_and_not_overwritten() {
        let (_d, path) = tmp_key();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "abc").expect("write short secret");

        let err = load_or_create_at(&path).expect_err("закороткий секрет → Err");
        assert!(err.contains("закороткий"), "текст помилки: {err}");
        let on_disk = std::fs::read_to_string(&path).expect("read secret file");
        assert!(on_disk == "abc", "чужий секрет не втрачено");
    }

    /// (д) unix: права 0600 (жодного доступу для group/other).
    #[cfg(unix)]
    #[test]
    fn created_file_has_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, path) = tmp_key();
        load_or_create_at(&path).expect("секрет створено");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert!(
            mode & 0o077 == 0,
            "доступ лише власнику, отримано {:o}",
            mode
        );
        assert!(
            mode & 0o600 == 0o600,
            "власник читає/пише, отримано {:o}",
            mode
        );
    }

    /// (е) `secret_path()` — поряд із `pgdata`, не всередині нього.
    #[test]
    fn secret_path_is_beside_pgdata_not_inside() {
        let p = secret_path();
        assert!(!p.as_os_str().is_empty(), "шлях не порожній");
        assert!(p.ends_with(SECRET_FILE_NAME), "шлях: {}", p.display());
        let parent = p.parent().expect("parent").to_path_buf();
        assert!(!parent.ends_with("pgdata"), "parent: {}", parent.display());
    }
}
