//! Налаштування ПРРО: get/save + key_store (шлях/пароль КЕП) + test_connection.
//! 1:1 Python `prro_settings_use_case.py` + `key_store.py` + `qr_url.py`.
//!
//! Відмінності від Python (задокументовані):
//! - Fernet: crate `fernet` (чиста Rust, сумісна з cryptography.fernet —
//!   стандарт Fernet: AES-128-CBC + HMAC-SHA256, base64url-токен);
//! - keystore-файли: `backend/.prro_keystore.json` / `.prro_master.key`
//!   (шлях через env PRRO_KEYSTORE_PATH / PRRO_MASTER_KEY_PATH,
//!   за замовчуванням — CWD, бо facade стартує з backend/);
//! - url: config env PRRO_TEST_URL / PRRO_PROD_URL / PRRO_MODE
//!   (Pydantic Settings у Python) — 1:1, з fallback на KEY_PRRO_URL з БД.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::models::{KEY_AUTO_FISCALIZE, KEY_PRRO_FN, KEY_PRRO_MODE, KEY_PRRO_TN, KEY_PRRO_ZN};
use super::repository::{PrroRepoError, PrroRepository};

/// Маска пароля для відображення — 1:1 Python `PASSWORD_MASK`.
pub const PASSWORD_MASK: &str = "••••";

/// Тип службового чеку ping — 1:1 Python `SERVICE_PING = "111"`.
pub const SERVICE_PING: &str = "111";

/// Дефолтні URL фіскальних серверів — 1:1 Python `config.py`.
pub const DEFAULT_PRRO_TEST_URL: &str = "cabinet.tax.gov.ua:9443";
pub const DEFAULT_PRRO_PROD_URL: &str = "prro.tax.gov.ua:443";
pub const DEFAULT_PRRO_MODE: &str = "test";

/// Помилка роботи з налаштуваннями ПРРО — 1:1 `PrroSettingsError`.
#[derive(Debug, thiserror::Error)]
#[error("[PRRO_SETTINGS_ERROR] {message}")]
pub struct PrroSettingsError {
    pub message: String,
}

impl PrroSettingsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<PrroRepoError> for PrroSettingsError {
    fn from(e: PrroRepoError) -> Self {
        Self::new(e.to_string())
    }
}

impl From<std::io::Error> for PrroSettingsError {
    fn from(e: std::io::Error) -> Self {
        Self::new(e.to_string())
    }
}

/// DTO налаштувань ПРРО — 1:1 `PrroSettingsDTO` (пароль замасковано).
#[derive(Debug, Clone, Serialize)]
pub struct PrroSettingsDto {
    pub key_file: Option<String>,
    pub key_password_masked: Option<String>,
    pub key_format: Option<String>,
    pub prro_fn: Option<String>,
    pub prro_tn: Option<String>,
    pub prro_zn: Option<String>,
    pub mode: String,
    pub url: Option<String>,
    pub shift_open: bool,
    pub online: bool,
    pub auto_fiscalize: bool,
}

/// Помилка key_store — 1:1 `PrroKeyStoreError`.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PrroKeyStoreError(pub String);

impl PrroKeyStoreError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Сховище шляху/формату/пароля ключа КЕП — 1:1 Python `PrroKeyStore`.
///
/// JSON-файл (за замовчуванням `backend/.prro_keystore.json`):
///   key_path, key_format, password_encrypted (Fernet).
/// Master-ключ: env PRRO_MASTER_KEY → файл `.prro_master.key` → генерація.
#[derive(Debug, Clone)]
pub struct PrroKeyStore {
    keystore_path: PathBuf,
    master_key_path: PathBuf,
    /// Готовий Fernet master-ключ (аргумент конструктора) — пріоритет №1.
    master_key: Option<String>,
}

impl Default for PrroKeyStore {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl PrroKeyStore {
    /// Створює сховище. Шляхи: env PRRO_KEYSTORE_PATH / PRRO_MASTER_KEY_PATH,
    /// за замовчуванням — CWD (backend/ при запуску facade, як Python backend/).
    pub fn new(master_key: Option<&str>, keystore_path: Option<&str>) -> Self {
        let keystore_path = keystore_path
            .map(PathBuf::from)
            .or_else(|| std::env::var("PRRO_KEYSTORE_PATH").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(".prro_keystore.json"));
        let master_key_path = std::env::var("PRRO_MASTER_KEY_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".prro_master.key"));
        Self {
            keystore_path,
            master_key_path,
            master_key: master_key.map(str::to_string),
        }
    }

    /// Створює Fernet-ключ (аргумент → env → файл → генерація) — 1:1 Python.
    fn fernet(&self) -> Result<fernet::Fernet, PrroKeyStoreError> {
        let key_bytes: Vec<u8> = if let Some(k) = &self.master_key {
            k.as_bytes().to_vec()
        } else if let Ok(env_key) = std::env::var("PRRO_MASTER_KEY") {
            env_key.into_bytes()
        } else if self.master_key_path.is_file() {
            let raw = std::fs::read(&self.master_key_path).map_err(|e| {
                PrroKeyStoreError::new(format!("Не вдалося прочитати master-ключ: {e}"))
            })?;
            String::from_utf8_lossy(&raw).trim().as_bytes().to_vec()
        } else {
            let new_key = fernet::Fernet::generate_key();
            std::fs::write(&self.master_key_path, format!("{new_key}\n")).map_err(|e| {
                PrroKeyStoreError::new(format!("Не вдалося зберегти master-ключ: {e}"))
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &self.master_key_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
            new_key.into_bytes()
        };
        let key_str = String::from_utf8(key_bytes)
            .map_err(|_| PrroKeyStoreError::new("master-ключ не є UTF-8"))?;
        fernet::Fernet::new(&key_str)
            .ok_or_else(|| PrroKeyStoreError::new("Невірний Fernet master-ключ"))
    }

    /// Читає JSON-файл налаштувань (порожній dict, якщо файлу немає).
    fn load_data(&self) -> Result<HashMap<String, String>, PrroKeyStoreError> {
        if !self.keystore_path.is_file() {
            return Ok(HashMap::new());
        }
        let text = std::fs::read_to_string(&self.keystore_path).map_err(|e| {
            PrroKeyStoreError::new(format!("Пошкоджений файл налаштувань ключа: {e}"))
        })?;
        serde_json::from_str(&text)
            .map_err(|e| PrroKeyStoreError::new(format!("Пошкоджений файл налаштувань ключа: {e}")))
    }

    /// Записує JSON-файл налаштувань з правами 0600.
    fn save_data(&self, data: &HashMap<String, String>) -> Result<(), PrroKeyStoreError> {
        let json = serde_json::to_string_pretty(data).map_err(|e| {
            PrroKeyStoreError::new(format!("Не вдалося серіалізувати налаштування: {e}"))
        })?;
        std::fs::write(&self.keystore_path, json).map_err(|e| {
            PrroKeyStoreError::new(format!("Не вдалося зберегти налаштування ключа: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &self.keystore_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
        Ok(())
    }

    /// Зберігає шлях до файлу ключа та (опційно) формат.
    pub fn save_key_path(
        &self,
        key_path: &str,
        key_format: Option<&str>,
    ) -> Result<(), PrroKeyStoreError> {
        let mut data = self.load_data()?;
        data.insert("key_path".to_string(), key_path.to_string());
        if let Some(fmt) = key_format {
            data.insert("key_format".to_string(), fmt.to_string());
        }
        self.save_data(&data)
    }

    /// Повертає збережений шлях до файлу ключа.
    pub fn get_key_path(&self) -> Result<String, PrroKeyStoreError> {
        let data = self.load_data()?;
        data.get("key_path")
            .filter(|p| !p.is_empty())
            .cloned()
            .ok_or_else(|| {
                PrroKeyStoreError::new(
                    "Шлях до ключа ПРРО не налаштовано. Викличте save_key_path()",
                )
            })
    }

    /// Повертає збережений формат ключа (або None).
    pub fn get_key_format(&self) -> Option<String> {
        self.load_data().ok()?.get("key_format").cloned()
    }

    /// Шифрує пароль ключа (Fernet) та зберігає.
    pub fn save_password_encrypted(&self, password: &str) -> Result<(), PrroKeyStoreError> {
        if password.is_empty() {
            return Err(PrroKeyStoreError::new("Пароль ключа не може бути порожнім"));
        }
        let token = self.fernet()?.encrypt(password.as_bytes());
        let mut data = self.load_data()?;
        data.insert("password_encrypted".to_string(), token);
        self.save_data(&data)
    }

    /// Розшифровує та повертає пароль ключа.
    pub fn decrypt_password(&self) -> Result<String, PrroKeyStoreError> {
        let data = self.load_data()?;
        let token = data.get("password_encrypted").ok_or_else(|| {
            PrroKeyStoreError::new(
                "Пароль ключа ПРРО не збережено. Викличте save_password_encrypted()",
            )
        })?;
        let plain = self.fernet()?.decrypt(token).map_err(|_| {
            PrroKeyStoreError::new(
                "Не вдалося розшифрувати пароль: master-ключ не збігається \
                     (перевірте PRRO_MASTER_KEY або файл .prro_master.key)",
            )
        })?;
        String::from_utf8(plain).map_err(|_| PrroKeyStoreError::new("Пароль ключа не є UTF-8"))
    }

    /// True, якщо збережено і шлях, і пароль — 1:1 `is_configured`.
    pub fn is_configured(&self) -> bool {
        let data = match self.load_data() {
            Ok(d) => d,
            Err(_) => return false,
        };
        data.get("key_path").map(|p| !p.is_empty()).unwrap_or(false)
            && data
                .get("password_encrypted")
                .map(|p| !p.is_empty())
                .unwrap_or(false)
    }
}

/// Use case налаштувань ПРРО — 1:1 `PrroSettingsUseCase`.
pub struct PrroSettingsUseCase {
    key_store: PrroKeyStore,
}

impl PrroSettingsUseCase {
    pub fn new(key_store: PrroKeyStore) -> Self {
        Self { key_store }
    }

    pub fn key_store(&self) -> &PrroKeyStore {
        &self.key_store
    }

    /// Поточні налаштування (пароль замасковано) — 1:1 `get_settings`.
    /// `grpc` — опційний клієнт для `_check_online` (best-effort, 1:1 Python).
    pub async fn get_settings(
        &self,
        repo: &dyn PrroRepository,
        grpc: Option<&crate::grpc::PrroGrpcClient>,
    ) -> Result<PrroSettingsDto, PrroSettingsError> {
        let prro_fn = repo.get_setting(KEY_PRRO_FN).await?;
        let prro_tn = repo.get_setting(KEY_PRRO_TN).await?;
        let prro_zn = repo.get_setting(KEY_PRRO_ZN).await?;
        let mode_key = repo.get_setting(KEY_PRRO_MODE).await?;
        let auto_key = repo.get_setting(KEY_AUTO_FISCALIZE).await?;

        let mode = mode_key
            .filter(|m| !m.is_empty())
            .unwrap_or_else(config_mode);
        let url = config_url(&mode);

        let key_path = self.key_store.get_key_path().ok();
        let key_format = self.key_store.get_key_format();
        let has_password = self.key_store.is_configured();

        let open_shift = repo.get_open_shift().await?;
        let online = match grpc {
            Some(g) => self.check_online(g).await,
            None => false,
        };

        Ok(PrroSettingsDto {
            key_file: key_path,
            key_password_masked: if has_password {
                Some(PASSWORD_MASK.to_string())
            } else {
                None
            },
            key_format,
            prro_fn,
            prro_tn,
            prro_zn,
            mode,
            url: Some(url),
            shift_open: open_shift.is_some(),
            online,
            auto_fiscalize: parse_bool(auto_key.as_deref()),
        })
    }

    /// Перевіряє онлайн-статус ПРРО (best-effort) — 1:1 `_check_online`.
    pub async fn check_online(&self, grpc: &crate::grpc::PrroGrpcClient) -> bool {
        match tokio::time::timeout(std::time::Duration::from_secs(5), grpc.status()).await {
            Ok(Ok(resp)) => resp.online,
            _ => false,
        }
    }

    /// Фіскальний номер ПРРО (без мережевих перевірок) — 1:1 `get_prro_fn`.
    pub async fn get_prro_fn(
        &self,
        repo: &dyn PrroRepository,
    ) -> Result<Option<String>, PrroRepoError> {
        repo.get_setting(KEY_PRRO_FN).await
    }

    /// Зберігає налаштування — 1:1 `save_settings`.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_settings(
        &self,
        repo: &dyn PrroRepository,
        grpc: Option<&crate::grpc::PrroGrpcClient>,
        key_file_content: Option<&[u8]>,
        key_file_name: Option<&str>,
        key_file_path: Option<&str>,
        key_password: Option<&str>,
        prro_fn: Option<&str>,
        prro_tn: Option<&str>,
        prro_zn: Option<&str>,
        mode: Option<&str>,
        auto_fiscalize: Option<bool>,
    ) -> Result<PrroSettingsDto, PrroSettingsError> {
        if let Some(m) = mode {
            if m != "test" && m != "prod" {
                return Err(PrroSettingsError::new(format!(
                    "Невідомий режим ПРРО: '{m}'. Допустимі: 'test', 'prod'"
                )));
            }
        }

        let current_mode = repo
            .get_setting(KEY_PRRO_MODE)
            .await?
            .filter(|m| !m.is_empty())
            .unwrap_or_else(config_mode);
        let target_mode = mode.unwrap_or(&current_mode);

        // 2. Ключ: копіюємо у certs/prro-{mode}/
        let key_path: Option<String> = if let Some(content) = key_file_content {
            let name = key_file_name.ok_or_else(|| {
                PrroSettingsError::new("Не вказано ім'я файлу ключа (key_file_name)")
            })?;
            Some(save_uploaded_key(content, name, target_mode)?)
        } else if let Some(path) = key_file_path {
            if !path.is_empty() {
                Some(copy_key_file(path, target_mode)?)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(path) = &key_path {
            let ext = Path::new(path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            self.key_store
                .save_key_path(path, if ext.is_empty() { None } else { Some(&ext) })
                .map_err(|e| {
                    PrroSettingsError::new(format!("Не вдалося зберегти шлях ключа: {}", e.0))
                })?;
        }

        // 3. Пароль (Fernet)
        if let Some(pw) = key_password {
            if !pw.is_empty() {
                self.key_store.save_password_encrypted(pw).map_err(|e| {
                    PrroSettingsError::new(format!("Не вдалося зберегти пароль ключа: {}", e.0))
                })?;
            }
        }

        // 4. Реквізити ПРРО (з валідацією формату)
        if let Some(fn_val) = prro_fn {
            let fn_val = fn_val.trim();
            let digits_ok = !fn_val.is_empty()
                && fn_val.len() <= 15
                && fn_val.chars().all(|c| c.is_ascii_digit())
                && fn_val.len() >= 5;
            if !digits_ok {
                return Err(PrroSettingsError::new(format!(
                    "Невірний фіскальний номер (prro_fn): очікується 5–15 цифр, отримано '{fn_val}'"
                )));
            }
            repo.set_setting(KEY_PRRO_FN, fn_val).await?;
        }
        if let Some(tn_val) = prro_tn {
            let tn_val = tn_val.trim();
            if !(5..=20).contains(&tn_val.len()) {
                return Err(PrroSettingsError::new(format!(
                    "Невірний податковий номер (prro_tn): очікується 5–20 символів, отримано '{tn_val}'"
                )));
            }
            repo.set_setting(KEY_PRRO_TN, tn_val).await?;
        }
        if let Some(zn_val) = prro_zn {
            let zn_val = zn_val.trim();
            if !(3..=30).contains(&zn_val.len()) {
                return Err(PrroSettingsError::new(format!(
                    "Невірний заводський номер (prro_zn): очікується 3–30 символів, отримано '{zn_val}'"
                )));
            }
            repo.set_setting(KEY_PRRO_ZN, zn_val).await?;
        }
        if let Some(m) = mode {
            repo.set_setting(KEY_PRRO_MODE, m).await?;
        }

        // 5. Авто-фіскалізація
        if let Some(af) = auto_fiscalize {
            repo.set_setting(KEY_AUTO_FISCALIZE, if af { "true" } else { "false" })
                .await?;
        }

        self.get_settings(repo, grpc).await
    }

    // ─── Перевірка зв'язку ─────────────────────────────────────────────────

    /// Людською мовою пояснення статусів CheckResponse — 1:1 `_STATUS_MESSAGES`.
    pub fn status_message(status: i32) -> String {
        match status {
            1 => "Зв'язок із фіскальним сервером встановлено (OK).".to_string(),
            -1 => "Помилка перевірки підпису/розбору XML (ERROR_VEREFY). Найчастіші причини: (1) ключ КЕП не завантажено або його формат не підтримується; (2) XML у check_sign порожній або непідписаний; (3) сертифікат підписанта не зареєстровано в тестовому середовищі ДПС.".to_string(),
            -2 => "Помилка перевірки ПРРО (ERROR_CHECK). Перевірте реквізити ПРРО.".to_string(),
            -3 => "Помилка запису на сервері (ERROR_SAVE). Спробуйте пізніше.".to_string(),
            -4 => "Загальна помилка сервера (ERROR_UNKNOWN).".to_string(),
            -5 => "Помилка типу посилки (ERROR_TYPE). Перевірте check_type.".to_string(),
            -6 => "Немає Z-звіту за попередній день (ERROR_NOT_PREV_ZREPORT).".to_string(),
            -7 => "Невірний формат XML (ERROR_XML).".to_string(),
            -8 => "Дата не відповідає Check.date (ERROR_XML_DATE).".to_string(),
            -9 => "Невірний формат XML чеку (ERROR_XML_CHK).".to_string(),
            -10 => "Невірний формат Z-звіту (ERROR_XML_ZREPORT).".to_string(),
            -11 => "Перевищено ліміт 168 годин офлайну (ERROR_OFFLINE_168).".to_string(),
            -12 => "Невірний хеш попереднього чеку (ERROR_BAD_HASH_PREV).".to_string(),
            -13 => "ПРРО не зареєстровано (ERROR_NOT_REGISTERED_RRO). Зареєструйте ПРРО в кабінеті платника податків.".to_string(),
            -14 => "Підписант не зареєстрований (ERROR_NOT_REGISTERED_SIGNER). Зареєструйте сертифікат підписанта в кабінеті платника.".to_string(),
            -15 => "Не відкрита зміна (ERROR_NOT_OPEN_SHIFT). Відкрийте зміну (POST /api/v2/prro/shift/open).".to_string(),
            -16 => "Невірний офлайн ID (ERROR_OFFLINE_ID).".to_string(),
            _ => "Невідомий статус фіскального сервера.".to_string(),
        }
    }

    /// Формує XML службового чеку T=111 для ping — 1:1 `_build_ping_check_sign`.
    ///
    /// Returns (check_sign, sign_error): підписаний XML (або непідписаний
    /// fallback) + пояснення, чому ключ не вдалося використати.
    pub fn build_ping_check_sign(
        &self,
        builder: &mut crate::xml::XmlBuilder,
        signer: Option<&dyn crate::crypto::PrroSigner>,
    ) -> (Vec<u8>, Option<String>) {
        let ts = Utc::now().format("%Y%m%d%H%M%S").to_string();
        let dat_xml = match builder.build_service_check_xml(SERVICE_PING, &ts) {
            Ok(x) => x,
            Err(e) => return (Vec::new(), Some(e.to_string())),
        };
        let message = match builder.build_message(&dat_xml, None, false) {
            Ok(m) => m,
            Err(e) => return (Vec::new(), Some(e.to_string())),
        };
        if let Some(s) = signer {
            match s.sign(message.as_bytes()) {
                Ok(signed) => (signed, None),
                Err(e) => (message.into_bytes(), Some(e.to_string())),
            }
        } else {
            (
                message.into_bytes(),
                Some("ключ КЕП недоступний".to_string()),
            )
        }
    }

    /// Перевіряє зв'язок з фіскальним сервером (ping) — 1:1 `test_connection`.
    ///
    /// Returns JSON: {"status": int, "ok": bool, "error": str | None}.
    pub async fn test_connection(
        &self,
        grpc: &crate::grpc::PrroGrpcClient,
        builder: &mut crate::xml::XmlBuilder,
        signer: Option<&dyn crate::crypto::PrroSigner>,
    ) -> serde_json::Value {
        let (check_sign, sign_error) = self.build_ping_check_sign(builder, signer);
        match grpc.ping(check_sign).await {
            Ok(resp) => {
                let status = resp.status;
                let server_error = if resp.error_message.is_empty() {
                    None
                } else {
                    Some(resp.error_message.clone())
                };
                let mut parts: Vec<String> = Vec::new();
                if let Some(se) = &sign_error {
                    parts.push(format!("КЕП не вдалося використати: {se}"));
                }
                if let Some(se) = &server_error {
                    parts.push(format!(
                        "[{}] Відповідь сервера: {se}",
                        crate::prro::fiscalize::status_name(status)
                    ));
                }
                // Числовий код + ім'я + короткий людський опис (1:1 Python).
                parts.push(crate::prro::status_codes::status_error_text(status));
                parts.push(format!(
                    "[{}] {}",
                    crate::prro::fiscalize::status_name(status),
                    Self::status_message(status)
                ));
                serde_json::json!({
                    "status": status,
                    "ok": status == 1,
                    "error": parts.join(" | "),
                })
            }
            Err(e) => {
                serde_json::json!({
                    "status": 0,
                    "ok": false,
                    "error": e.to_string(),
                })
            }
        }
    }
}

/// env/дефолт mode — 1:1 Python `config.PRRO_MODE` (Pydantic Settings).
pub fn config_mode() -> String {
    std::env::var("PRRO_MODE").unwrap_or_else(|_| DEFAULT_PRRO_MODE.to_string())
}

/// URL фіскального сервера залежно від mode — 1:1 Python `prro_url()`.
pub fn config_url(mode: &str) -> String {
    if mode == "prod" {
        std::env::var("PRRO_PROD_URL").unwrap_or_else(|_| DEFAULT_PRRO_PROD_URL.to_string())
    } else {
        std::env::var("PRRO_TEST_URL").unwrap_or_else(|_| DEFAULT_PRRO_TEST_URL.to_string())
    }
}

/// Перетворює текстове значення прапора ('1'/'true'/'yes'/'on') у bool —
/// 1:1 Python `_parse_bool` (None → false).
pub fn parse_bool(value: Option<&str>) -> bool {
    match value {
        None => false,
        Some(v) => {
            let v = v.trim().to_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
    }
}

/// Директорія для ключів: backend/certs/prro-{mode}/ — 1:1 Python `_certs_dir`.
fn certs_dir(mode: &str) -> PathBuf {
    let root = std::env::var("PRRO_CERTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("certs"));
    let dir = root.join(format!("prro-{mode}"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Зберігає завантажений файл ключа у certs/prro-{mode}/ — 1:1 `_save_uploaded_key`.
fn save_uploaded_key(
    content: &[u8],
    filename: &str,
    mode: &str,
) -> Result<String, PrroSettingsError> {
    // захист від path traversal: беремо лише ім'я файлу (1:1 Python Path.name)
    let safe_name = Path::new(filename)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if safe_name.is_empty() {
        return Err(PrroSettingsError::new("Порожнє ім'я файлу ключа"));
    }
    let dest = certs_dir(mode).join(&safe_name);
    std::fs::write(&dest, content).map_err(|e| {
        PrroSettingsError::new(format!(
            "Не вдалося зберегти ключ у {}: {e}",
            dest.display()
        ))
    })?;
    Ok(dest.to_string_lossy().to_string())
}

/// Копіює файл ключа у certs/prro-{mode}/ — 1:1 `_copy_key_file`.
pub fn copy_key_file(src_path: &str, mode: &str) -> Result<String, PrroSettingsError> {
    let src = Path::new(src_path);
    if !src.is_file() {
        return Err(PrroSettingsError::new(format!(
            "Файл ключа не знайдено: {src_path}"
        )));
    }
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let dest = certs_dir(mode).join(&name);
    std::fs::copy(src, &dest).map_err(|e| {
        PrroSettingsError::new(format!(
            "Не вдалося скопіювати ключ у {}: {e}",
            dest.display()
        ))
    })?;
    Ok(dest.to_string_lossy().to_string())
}

/// Формує URL перевірки фіскального чеку (QR) — 1:1 Python `qr_url.py`.
pub fn build_fiscal_check_url(
    fiscal_number: &str,
    amount: rust_decimal::Decimal,
    prro_fn: &str,
    sent_at: chrono::DateTime<Utc>,
    mac: Option<&str>,
) -> Option<String> {
    if fiscal_number.is_empty() || prro_fn.is_empty() {
        return None;
    }
    let mac_value = match mac {
        Some(m) if !m.is_empty() => m.to_string(),
        _ => {
            // SHA-1 hex fallback — 1:1 Python `_fallback_mac`
            use sha1::{Digest, Sha1};
            let mut h = Sha1::new();
            h.update(fiscal_number.as_bytes());
            hex::encode(h.finalize())
        }
    };
    let sm = format!("{:.2}", amount.round_dp(2));
    let date = sent_at.format("%Y%m%d");
    let time = sent_at.format("%H%M");
    // V1: параметри URL-кодуються як Python `urllib.parse.urlencode`
    // (quote_plus) — 1:1 parity (base64 MAC містить + / =).
    Some(format!(
        "https://cabinet.tax.gov.ua/cashregs/check?mac={}&date={}&time={}&id={}&sm={}&fn={}",
        urlencode_plus(&mac_value),
        urlencode_plus(&date.to_string()),
        urlencode_plus(&time.to_string()),
        urlencode_plus(fiscal_number),
        urlencode_plus(&sm),
        urlencode_plus(prro_fn),
    ))
}

/// URL-encode значення параметра у стилі Python `urllib.parse.quote_plus`:
/// алфавіт `A-Za-z0-9-_.~` лишається, пробіл → '+', решта → %XX.
/// 1:1 Python `urllib.parse.urlencode` (V1, QR-перевірка ДПС).
pub fn urlencode_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Ім'я UUID-суфікса для NF-дубліката — 1:1 Python `uuid4().hex[:6]`.
pub fn uuid6() -> String {
    Uuid::new_v4().simple().to_string()[..6].to_string()
}

#[cfg(test)]
mod fernet_tests {
    #[test]
    fn fernet_decrypts_python_token() {
        // Ключ і токен, згенеровані Python cryptography.fernet (еталон,
        // зафіксовані для golden-тесту сумісності).
        let key = "l6mOznsttl_clf0EO0xJs-Rh50RkwUQsUWrxsWhmuqA=";
        let token = "gAAAAABqdnuzRRPagIqqdPv5bBsZIfQCQZMZJzdwvkrcvYrpuzIVeN_BR5tCoLKodPRUDdT4MnE8xF4SzFJpef_a70jhGpGh8Q==";
        let f = fernet::Fernet::new(key).expect("фернет-ключ валідний");
        let plain = f.decrypt(token).expect("токен Python має розшифруватись");
        assert_eq!(String::from_utf8(plain).unwrap(), "test2003");
    }
}

#[cfg(test)]
mod real_keystore_tests {
    #[test]
    fn decrypts_real_python_keystore() {
        // Реальні файли, створені Python-бекендом (backend/app/infrastructure/).
        // Тест читає їх напряму — перевірка 1:1 сумісності Fernet.
        let key_path = std::env::var("REAL_KS_PATH").unwrap_or_else(|_| "UNSET".into());
        if key_path == "UNSET" {
            eprintln!("SKIP: REAL_KS_PATH не задано");
            return;
        }
        let json = std::fs::read_to_string(&key_path).unwrap();
        let data: serde_json::Value = serde_json::from_str(&json).unwrap();
        let token = data["password_encrypted"].as_str().unwrap();
        let master_path = std::env::var("REAL_MASTER_PATH").unwrap();
        let key_raw = std::fs::read_to_string(&master_path).unwrap();
        let key = key_raw.trim();
        let f = fernet::Fernet::new(key).expect("ключ валідний");
        let plain = f
            .decrypt(token)
            .expect("реальний Python-токен розшифровується");
        assert_eq!(String::from_utf8(plain).unwrap(), "test2003");
    }
}
