//! Провіжинінг standby-вузла мережі магазинів (ЕТАП 16,
//! `network-replication-etap15-20.md` §7).
//!
//! Локальний PostgreSQL вузла перетворюється на **фізичну hot-standby-копію
//! primary** через реальний `pg_basebackup`:
//!
//! ```text
//! 1. Знайти бінарники PG (механізм [`embedded_pg`] — `pg_basebackup` лежить
//!    поруч із `pg_ctl` у тому самому `bin/`).
//! 2. Зупинити локальний PG ([`embedded_pg::EMBEDDED_PG_PORT`] = 5433), якщо запущено.
//! 3. Очистити/створити порожній `data_dir` — БЕЗ `initdb` (копія з primary).
//! 4. `pg_basebackup -h <primary> -p <port> -U <replicator> -D <data_dir>
//!    -X stream -C -S <slot> -R`  → створює replication slot на primary,
//!    пише `standby.signal` + `primary_conninfo` у `postgresql.auto.conf`.
//! 5. Пароль реплікації: plaintext у `postgresql.auto.conf` (PG читає
//!    `primary_conninfo` лише у plaintext; файл 0600 у `data_dir`) + ОКРЕМО
//!    зашифрована копія AES-256-GCM (`db_sources`-підхід, `.dbkey`) для
//!    повторного використання.
//! 6. Старт локального PG через [`embedded_pg::EmbeddedPostgres`] → hot_standby.
//! 7. Перевірка `SELECT pg_is_in_recovery()` = true.
//! ```
//!
//! Вхідні креденшли (роль `replicator_<short>`, слот `standby_<short>`, пароль,
//! адреса primary) — з відповіді `POST /api/v1/network-nodes/join` (ЕТАП 15,
//! `torgashka-api::network_nodes::ReplicationCreds`).
//!
//! Модуль не залежить від Tauri: чиста інфраструктурна операція, викликається
//! з будь-якого async-контексту (Tauri-команда, tokio-задача).

use std::path::{Path, PathBuf};

use crate::embedded_pg::{pg_log, EmbeddedPostgres, EMBEDDED_PG_PORT};

// ─────────────────────────────────────────────────────────────────────────────
// Помилки
// ─────────────────────────────────────────────────────────────────────────────

/// Помилки провіжинінгу standby.
#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("бінарники PostgreSQL не знайдено (TORGASHKA_PG_DIR, resources/postgres, .cache/pg, pg_config)")]
    BinariesNotFound,
    #[error("у bin/ відсутній pg_basebackup: {0}")]
    MissingBinary(PathBuf),
    #[error("локальний PG не зупинено: {0}")]
    StopFailed(crate::embedded_pg::Error),
    #[error("не вдалося очистити data_dir {0}: {1}")]
    DataDir(String, std::io::Error),
    #[error("pg_basebackup завершився з кодом {code}; stderr: {stderr}")]
    BackupFailed { code: i32, stderr: String },
    #[error("primary_conninfo не знайдено у postgresql.auto.conf після pg_basebackup -R")]
    ConninfoMissing,
    #[error("не вдалося записати файл {path}: {err}")]
    WriteFile { path: PathBuf, err: std::io::Error },
    #[error("локальний PG не стартував: {0}")]
    StartFailed(crate::embedded_pg::Error),
    #[error("psql не зміг перевірити pg_is_in_recovery: {0}")]
    PsqlFailed(String),
    #[error("standby не в режимі recovery після старту (pg_is_in_recovery = {0}); перевірте postgres.log у data_dir")]
    NotInRecovery(String),
    #[error("крипто-помилка (AES-256-GCM/.dbkey): {0}")]
    Crypto(#[from] crate::db_sources::DbSourcesError),
    #[error("помилка введення/виведення: {0}")]
    Io(#[from] std::io::Error),
    #[error("некоректні параметри: {0}")]
    Invalid(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Параметри
// ─────────────────────────────────────────────────────────────────────────────

/// Вхідні параметри провіжинінгу standby-вузла.
///
/// Значення `replication_role`/`replication_slot`/`replication_password`/
/// `primary_host`/`primary_port` приходять з відповіді `POST
/// /api/v1/network-nodes/join` (ЕТАП 15); на диску вони НЕ зберігаються —
/// лише пароль (у `postgresql.auto.conf` для PG + зашифрована копія).
#[derive(Debug, Clone)]
pub struct StandbyParams {
    /// Host primary (з `ReplicationCreds.primary_host`).
    pub primary_host: String,
    /// Порт primary (з `ReplicationCreds.primary_port`).
    pub primary_port: u16,
    /// Ім'я ролі реплікації на primary (`replicator_<short>`).
    pub replication_role: String,
    /// Ім'я replication slot (`standby_<short>`).
    pub replication_slot: String,
    /// Пароль ролі реплікації (plaintext, повертається join-ом ОДИН раз).
    pub replication_password: String,
    /// Каталог даних локального standby. `None` → `crate::embedded_pg::data_dir_default()`.
    pub data_dir: Option<PathBuf>,
    /// Каталог `bin/` PostgreSQL. `None` → `EmbeddedPostgres::locate()`.
    pub bin_dir: Option<PathBuf>,
    /// Файл-«якір» для ключа шифрування: `.dbkey` створюється/читається
    /// ПОРУЧ із цим файлом (db_sources-підхід, `db_sources::ensure_key`).
    /// `None` → `<data_dir.parent()>/replication_secret.ctx` (той самий
    /// каталог, де живе локальний PG вузла).
    pub secret_anchor: Option<PathBuf>,
}

impl StandbyParams {
    /// Резолв каталогу даних (дефолт — стандартний `data_dir_default`).
    pub fn resolved_data_dir(&self) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(crate::embedded_pg::data_dir_default)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Дрібні хелпери (імена бінарників, права файлів, ін'єкція пароля)
// ─────────────────────────────────────────────────────────────────────────────

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn pg_basebackup_name() -> String {
    exe("pg_basebackup")
}

fn psql_name() -> String {
    exe("psql")
}

/// Локальний користувач PG (той самий env, що й embedded_pg).
fn pg_user() -> String {
    std::env::var("TORGASHKA_PG_USER").unwrap_or_else(|_| "postgres".to_string())
}

/// Права 0600 (unix); Windows — noop (ACL керує доступом).
#[cfg(unix)]
fn set_private_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Пароль можна вставляти в `primary_conninfo` без екранування лише якщо він
/// не містить розділювачів conninfo (пробіл, лапки, зворотний слеш).
fn password_is_conninfo_safe(pw: &str) -> bool {
    !pw.is_empty()
        && pw.len() <= 128
        && !pw
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'))
}

/// Формує аргументи `pg_basebackup` (без самого бінарника).
///
/// `with_create_slot = true` → `-C -S <slot>` (створити replication slot на
/// primary); `false` → `-S <slot>` (повторний provision: слот вже існує,
/// під'єднуємось до наявного). `-w` — без інтерактивного запиту пароля
/// (пароль передається через env `PGPASSWORD`).
fn pg_basebackup_args(
    host: &str,
    port: u16,
    role: &str,
    slot: &str,
    data_dir: &Path,
    with_create_slot: bool,
) -> Vec<String> {
    let mut args = vec![
        "-h".to_string(),
        host.to_string(),
        "-p".to_string(),
        port.to_string(),
        "-U".to_string(),
        role.to_string(),
        "-D".to_string(),
        data_dir.display().to_string(),
        "-X".to_string(),
        "stream".to_string(),
        "-R".to_string(),
        "-w".to_string(),
    ];
    if with_create_slot {
        args.push("-C".to_string());
    }
    args.push("-S".to_string());
    args.push(slot.to_string());
    args
}

/// Додає `password=<pw>` у рядок `primary_conninfo` всередині
/// `postgresql.auto.conf` (згенерований `pg_basebackup -R`).
///
/// - знаходить рядок `primary_conninfo = '...'`;
/// - прибирає старий `password=...` (ідемпотентність повторного виклику);
/// - вставляє новий перед закриваючим апострофом.
///
/// Повертає НОВИЙ повний вміст конфіга. Якщо `primary_conninfo` відсутній —
/// [`ProvisionError::ConninfoMissing`].
fn inject_conninfo_password(conf: &str, password: &str) -> Result<String, ProvisionError> {
    let mut out = String::with_capacity(conf.len() + password.len() + 16);
    let mut found = false;
    for line in conf.lines() {
        if line.trim_start().starts_with("primary_conninfo") {
            let Some(open) = line.find('\'') else {
                return Err(ProvisionError::ConninfoMissing);
            };
            let rest = &line[open + 1..];
            let Some(rel_close) = rest.find('\'') else {
                return Err(ProvisionError::ConninfoMissing);
            };
            let close = open + 1 + rel_close;
            // Внутрішній вміст conninfo без password-токена (якщо був).
            let mut inner = line[open + 1..close].to_string();
            if let Some(pos) = find_password_token(&inner) {
                inner.replace_range(pos..pos + token_len(&inner, pos), "");
                inner = inner.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            if !inner.trim().is_empty() {
                inner.push(' ');
            }
            inner.push_str("password=");
            inner.push_str(password);

            out.push_str(&line[..open + 1]);
            out.push_str(&inner);
            out.push_str(&line[close..]);
            out.push('\n');
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        return Err(ProvisionError::ConninfoMissing);
    }
    Ok(out)
}

/// Позиція токена `password=...` у conninfo (значення без пробілів —
/// `gen_replication_password` дає 24 hex; PG генерує conninfo без пробілів у
/// значеннях). `None` — токена немає.
fn find_password_token(conninfo: &str) -> Option<usize> {
    let bytes = conninfo.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if conninfo[i..].starts_with("password=") {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Довжина токена, що починається в `pos` (до пробілу/кінця рядка).
fn token_len(s: &str, pos: usize) -> usize {
    s[pos..].find(char::is_whitespace).unwrap_or(s.len() - pos)
}

/// Шлях зашифрованої копії пароля: `standby_replication.enc` поруч із data_dir
/// (НЕ всередині — каталог PG тримаємо чистим).
fn encrypted_copy_path(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("standby_replication.enc")
}

/// Якір для `.dbkey`: дефолт — файл-маркер поруч із data_dir (той самий
/// каталог, що й `data_dir_default`), щоб ключ був стабільним між запусками.
fn default_secret_anchor(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("replication_secret.ctx")
}

// ─────────────────────────────────────────────────────────────────────────────
// Публічна функція
// ─────────────────────────────────────────────────────────────────────────────

/// Виконує повний провіжинінг standby-вузла (алгоритм §7 плану).
///
/// Блокувальні/тривалі операції (`pg_ctl`, `pg_basebackup`, fs) виконуються
/// через `spawn_blocking`/`tokio::process` — функцію можна безпечно викликати
/// з tokio-runtime. Кожен крок логується через [`embedded_pg::pg_log`]
/// (stderr + `torgashka.log`).
pub async fn provision_standby(p: StandbyParams) -> Result<(), ProvisionError> {
    // ── 1. Бінарники PG ─────────────────────────────────────────────────────
    let bin_dir = match p.bin_dir.clone() {
        Some(b) => b,
        None => EmbeddedPostgres::locate().ok_or(ProvisionError::BinariesNotFound)?,
    };
    let pg_basebackup = bin_dir.join(pg_basebackup_name());
    if !pg_basebackup.is_file() {
        return Err(ProvisionError::MissingBinary(pg_basebackup));
    }
    pg_log(
        "INFO",
        &format!(
            "[standby] крок 1: pg_basebackup знайдено: {}",
            pg_basebackup.display()
        ),
    );

    // Валідація параметрів (ранній вихід, не чіпаючи data_dir).
    let data_dir = p.resolved_data_dir();
    if p.primary_host.trim().is_empty() {
        return Err(ProvisionError::Invalid("primary_host порожній".into()));
    }
    if p.replication_role.trim().is_empty() {
        return Err(ProvisionError::Invalid("replication_role порожній".into()));
    }
    if p.replication_slot.trim().is_empty() {
        return Err(ProvisionError::Invalid("replication_slot порожній".into()));
    }
    if !password_is_conninfo_safe(&p.replication_password) {
        return Err(ProvisionError::Invalid(
            "replication_password містить недопустимі символи для primary_conninfo \
             (пробіл, ', \", \\) або порожній/довший за 128 символів"
                .into(),
        ));
    }
    let secret_anchor = p
        .secret_anchor
        .clone()
        .unwrap_or_else(|| default_secret_anchor(&data_dir));

    pg_log(
        "INFO",
        &format!(
            "[standby] primary={}:{} role={} slot={} data_dir={}",
            p.primary_host,
            p.primary_port,
            p.replication_role,
            p.replication_slot,
            data_dir.display()
        ),
    );

    // ── 2. Зупинити локальний PG (якщо запущено) ───────────────────────────
    // stop() ідемпотентний: якщо 127.0.0.1:5433 не слухає — повертає Ok(())
    // без дій.
    {
        let stop = EmbeddedPostgres::with_data_dir(bin_dir.clone(), data_dir.clone());
        tokio::task::spawn_blocking(move || stop.stop())
            .await
            .map_err(|e| ProvisionError::Invalid(format!("spawn_blocking(stop): {e}")))?
            .map_err(ProvisionError::StopFailed)?;
    }
    pg_log(
        "INFO",
        "[standby] крок 2: локальний PG зупинено (або не був запущений)",
    );

    // ── 3. Очистити/створити порожній data_dir (БЕЗ initdb) ────────────────
    {
        let dir = data_dir.clone();
        let res = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            std::fs::create_dir_all(&dir)
        })
        .await
        .map_err(|e| ProvisionError::DataDir(data_dir.display().to_string(), e.into()))?;
        res.map_err(|e| ProvisionError::DataDir(data_dir.display().to_string(), e))?;
    }
    pg_log(
        "INFO",
        &format!(
            "[standby] крок 3: data_dir очищено і створено порожнім: {}",
            data_dir.display()
        ),
    );

    // ── 4. pg_basebackup ────────────────────────────────────────────────────
    // -X stream  — WAL передається потоком (без tar-файлів);
    // -C -S <slot> — створити replication slot на primary;
    // -R         — standby.signal + primary_conninfo (auto.conf);
    // -w         — не питати пароль інтерактивно (PGPASSWORD env).
    let with_create = [true, false];
    let mut backup_err = None;
    for create_slot in with_create {
        let mut cmd = tokio::process::Command::new(&pg_basebackup);
        cmd.args(pg_basebackup_args(
            &p.primary_host,
            p.primary_port,
            &p.replication_role,
            &p.replication_slot,
            &data_dir,
            create_slot,
        ))
        .env("PGPASSWORD", &p.replication_password);
        let out = cmd
            .output()
            .await
            .map_err(|e| ProvisionError::Invalid(format!("pg_basebackup spawn: {e}")))?;
        if out.status.success() {
            if !out.stderr.is_empty() {
                pg_log(
                    "INFO",
                    &format!(
                        "[standby] pg_basebackup stderr: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                );
            }
            pg_log(
                "INFO",
                &format!(
                    "[standby] крок 4: pg_basebackup завершено успішно ({} байт виводу)",
                    out.stdout.len() + out.stderr.len()
                ),
            );
            backup_err = None;
            break;
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        // Ідемпотентність повторного provision: слот вже створено на primary
        // попереднім запуском — повторюємо БЕЗ -C (під'єднуємось до наявного).
        if create_slot && stderr.to_lowercase().contains("already exists") {
            pg_log(
                "WARN",
                &format!(
                    "[standby] replication slot '{}' вже існує на primary — \
                     продовжую з існуючим слотом",
                    p.replication_slot
                ),
            );
            continue;
        }
        backup_err = Some(ProvisionError::BackupFailed { code, stderr });
        break;
    }
    if let Some(e) = backup_err {
        return Err(e);
    }

    // ── 5a. Пароль plaintext у postgresql.auto.conf (0600) ─────────────────
    let auto_conf = data_dir.join("postgresql.auto.conf");
    let conf_raw = std::fs::read_to_string(&auto_conf).map_err(|e| ProvisionError::WriteFile {
        path: auto_conf.clone(),
        err: e,
    })?;
    let conf_with_pw = inject_conninfo_password(&conf_raw, &p.replication_password)?;
    {
        let path = auto_conf.clone();
        let res = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::write(&path, conf_with_pw.as_bytes())?;
            set_private_file(&path)
        })
        .await
        .map_err(|e| ProvisionError::WriteFile {
            path: auto_conf.clone(),
            err: e.into(),
        })?;
        res.map_err(|err| ProvisionError::WriteFile {
            path: auto_conf.clone(),
            err,
        })?;
    }
    pg_log(
        "INFO",
        &format!(
            "[standby] крок 5: пароль реплікації записано у postgresql.auto.conf (0600): {}",
            auto_conf.display()
        ),
    );

    // ── 5b. Зашифрована копія пароля (AES-256-GCM, .dbkey поруч із anchor) ─
    let enc = crate::db_sources::encrypt_password(&secret_anchor, &p.replication_password)?;
    let enc_path = encrypted_copy_path(&data_dir);
    {
        let path = enc_path.clone();
        let res = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::write(&path, enc.as_bytes())?;
            set_private_file(&path)
        })
        .await
        .map_err(|e| ProvisionError::WriteFile {
            path: enc_path.clone(),
            err: e.into(),
        })?;
        res.map_err(|err| ProvisionError::WriteFile {
            path: enc_path.clone(),
            err,
        })?;
    }
    pg_log(
        "INFO",
        &format!(
            "[standby] крок 5b: зашифрована копія пароля (AES-256-GCM): {}",
            enc_path.display()
        ),
    );

    // ── 6. Старт локального PG → hot_standby ───────────────────────────────
    {
        let mut mgr = EmbeddedPostgres::with_data_dir(bin_dir.clone(), data_dir.clone());
        tokio::task::spawn_blocking(move || mgr.start())
            .await
            .map_err(|e| ProvisionError::Invalid(format!("spawn_blocking(start): {e}")))?
            .map_err(ProvisionError::StartFailed)?;
    }
    pg_log(
        "INFO",
        &format!(
            "[standby] крок 6: локальний PG стартував на 127.0.0.1:{} (hot_standby)",
            EMBEDDED_PG_PORT
        ),
    );

    // ── 7. Перевірка pg_is_in_recovery() = true ─────────────────────────────
    wait_recovery(&bin_dir, &pg_user()).await?;
    pg_log(
        "INFO",
        "[standby] крок 7: pg_is_in_recovery() = true — standby активний",
    );
    Ok(())
}

/// Poll `SELECT pg_is_in_recovery()` через psql (до 15 × 500 мс — standby
/// після старту може бути в crash recovery, поки не почне приймати запити).
async fn wait_recovery(bin_dir: &Path, user: &str) -> Result<(), ProvisionError> {
    let psql = bin_dir.join(psql_name());
    if !psql.is_file() {
        return Err(ProvisionError::MissingBinary(psql));
    }
    let mut last = String::new();
    for attempt in 1..=15u32 {
        let mut cmd = tokio::process::Command::new(&psql);
        cmd.arg("-h")
            .arg("127.0.0.1")
            .arg("-p")
            .arg(EMBEDDED_PG_PORT.to_string())
            .arg("-U")
            .arg(user)
            .arg("-d")
            .arg("postgres")
            .arg("-tAc")
            .arg("SELECT pg_is_in_recovery()");
        // Локальний доступ після копії успадковує pg_hba primary (initdb
        // embedded PG — -A trust, тому локально пароль не потрібен; якщо
        // задано TORGASHKA_PG_PASSWORD — передаємо про всяк).
        if let Ok(pw) = std::env::var("TORGASHKA_PG_PASSWORD") {
            if !pw.is_empty() {
                cmd.env("PGPASSWORD", pw);
            }
        }
        let out = cmd
            .output()
            .await
            .map_err(|e| ProvisionError::PsqlFailed(format!("psql spawn: {e}")))?;
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s == "t" {
                return Ok(());
            }
            last = format!("stdout={s:?}");
        } else {
            last = format!(
                "exit={:?} stderr={}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        if attempt < 15 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    Err(ProvisionError::NotInRecovery(last))
}

// ─────────────────────────────────────────────────────────────────────────────
// Тести
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_adds_password_to_conninfo() {
        let conf = "\
# Do not edit this file manually!
# It will be overwritten by the pg_basebackup utility.
primary_conninfo = 'host=10.0.0.5 port=5433 user=replicator_abc12345 application_name=standby_abc12345'
";
        let out = inject_conninfo_password(conf, "a1b2c3d4e5f60718293a4b5c").unwrap();
        assert!(out.contains("password=a1b2c3d4e5f60718293a4b5c"));
        assert!(out.contains("host=10.0.0.5"));
        assert!(out.contains("user=replicator_abc12345"));
        // Лапки conninfo не зламані — рівно два апострофи на рядку.
        let line = out
            .lines()
            .find(|l| l.starts_with("primary_conninfo"))
            .unwrap();
        assert_eq!(line.matches('\'').count(), 2);
        assert!(!line.contains("password=password="));
    }

    #[test]
    fn inject_replaces_existing_password_idempotently() {
        let conf = "\
primary_conninfo = 'host=h port=5432 user=u password=oldpass1234567890abcdef app=standby_x'
";
        let once = inject_conninfo_password(conf, "newpass").unwrap();
        let twice = inject_conninfo_password(&once, "newpass").unwrap();
        assert_eq!(once, twice, "повторний виклик має бути ідемпотентним");
        assert_eq!(once.matches("password=newpass").count(), 1);
        assert!(!once.contains("oldpass"));
        assert!(!once.contains("password=password="));
    }

    #[test]
    fn inject_fails_when_conninfo_missing() {
        let conf = "# коментар без primary_conninfo\nfoo = 'bar'\n";
        let err = inject_conninfo_password(conf, "pw").unwrap_err();
        assert!(matches!(err, ProvisionError::ConninfoMissing));
    }

    #[test]
    fn inject_handles_empty_conninfo() {
        let conf = "primary_conninfo = ''\n";
        let out = inject_conninfo_password(conf, "pw1234567890abcdef1234").unwrap();
        let line = out
            .lines()
            .find(|l| l.starts_with("primary_conninfo"))
            .unwrap();
        assert!(line.contains("password=pw1234567890abcdef1234"));
        assert!(line.trim_end().ends_with('\''));
    }

    #[test]
    fn password_safety_rejects_conninfo_breakers() {
        assert!(password_is_conninfo_safe("0123456789abcdef01234567"));
        assert!(!password_is_conninfo_safe("has space"));
        assert!(!password_is_conninfo_safe("has'quote"));
        assert!(!password_is_conninfo_safe("has\"quote"));
        assert!(!password_is_conninfo_safe("has\\slash"));
        assert!(!password_is_conninfo_safe(""));
        assert!(!password_is_conninfo_safe(&"x".repeat(200)));
    }

    #[test]
    fn find_password_token_locates_and_ignores_other() {
        let s = "host=h user=u password=abc123 app=x";
        let pos = find_password_token(s).unwrap();
        assert_eq!(token_len(s, pos), "password=abc123".len());
        assert_eq!(find_password_token("host=h user=u"), None);
    }

    #[test]
    fn resolved_data_dir_defaults_to_embedded() {
        let p = StandbyParams {
            primary_host: "10.0.0.5".into(),
            primary_port: 5433,
            replication_role: "replicator_x".into(),
            replication_slot: "standby_x".into(),
            replication_password: "0123456789abcdef01234567".into(),
            data_dir: None,
            bin_dir: None,
            secret_anchor: None,
        };
        assert_eq!(
            p.resolved_data_dir(),
            crate::embedded_pg::data_dir_default()
        );
    }

    #[test]
    fn encrypted_copy_lives_beside_data_dir() {
        let d = Path::new("/tmp/pgdata");
        assert_eq!(
            encrypted_copy_path(d),
            Path::new("/tmp/standby_replication.enc")
        );
        assert_eq!(
            default_secret_anchor(d),
            Path::new("/tmp/replication_secret.ctx")
        );
    }

    #[test]
    fn basebackup_args_shape() {
        let args = pg_basebackup_args(
            "10.0.0.5",
            5433,
            "replicator_abc12345",
            "standby_abc12345",
            Path::new("/tmp/x"),
            true,
        );
        let joined = args.join(" ");
        assert!(joined.contains("-h 10.0.0.5"));
        assert!(joined.contains("-p 5433"));
        assert!(joined.contains("-U replicator_abc12345"));
        assert!(joined.contains("-D /tmp/x"));
        assert!(joined.contains("-X stream"));
        assert!(joined.contains("-C"));
        assert!(joined.contains("-S standby_abc12345"));
        assert!(joined.contains("-R"));
        assert!(joined.contains("-w"));
        // Повторний provision (слот існує): без -C, але -S лишається.
        let again = pg_basebackup_args(
            "10.0.0.5",
            5433,
            "replicator_abc12345",
            "standby_abc12345",
            Path::new("/tmp/x"),
            false,
        );
        assert!(!again.iter().any(|a| a == "-C"));
        assert!(again.iter().any(|a| a == "-S"));
    }
}
