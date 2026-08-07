//! FFI до крипто-ядра ІІТ (SDK EUSignCP, `euscp.so`) через `libloading`.
//!
//! Аналог Python `iit_sdk.py` (ctypes): ті самі 10 функцій, ті самі сигнатури,
//! той самий потік (EUGetJKSPrivateKeyFile → EUSaveCertificate →
//! EUReadPrivateKeyBinary → EUSignDataInternal). Рішення: ADR-014 (a).
//!
//! Чому libloading, а не bindgen: bindgen вимагає libclang у CI; для 10
//! стабільних функцій C ABI ручні `extern "C"` сигнатури — це точний аналог
//! ctypes-еталона (перевірено в продукти місяцями).

use libloading::Library;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum IitSdkError {
    #[error("Крипто-ядро ІІТ (SDK EUSignCP) не знайдено: {0}. Встановіть його скриптом backend/scripts/setup_iit_sdk.sh")]
    LibraryNotFound(PathBuf),
    #[error("Не вдалося завантажити {0}: {1}")]
    Load(String, String),
    #[error("Функція {0} не знайдена в euscp.so")]
    FunctionNotFound(String),
    #[error("SDK помилка (код {code}): {desc}")]
    Sdk { code: u32, desc: String },
    #[error("Файл JKS не знайдено: {0}")]
    JksNotFound(String),
    #[error("Не вдалося прочитати JKS: {0}")]
    JksRead(String),
    #[error("Не вдалося завантажити ключ у крипто-ядро ІІТ: {0}")]
    KeyLoad(String),
    #[error("Ключ не завантажено в крипто-ядро ІІТ. Спочатку викличте load_jks_key()")]
    KeyNotLoaded,
    #[error("Помилка формування підпису ДСТУ 4145: {0}")]
    Sign(String),
    #[error("SDK повернув порожній підпис")]
    EmptySignature,
    #[error("libloading: {0}")]
    LibLoading(#[from] libloading::Error),
    #[error("EUInitialize: {0}")]
    Init(String),
    #[error("Крипто-шар: {0}")]
    Generic(String),
}

/// C-сигнатури EUSignCP (1:1 Python iit_sdk.py).
type FnEuiInitialize = unsafe extern "C" fn() -> i32;
type FnEuiSetSettingsFilePath = unsafe extern "C" fn(*const u8) -> u64;
type FnEuiSetFileStoreSettings =
    unsafe extern "C" fn(*const u8, i32, i32, i32, i32, i32, i32, u64) -> u64;
type FnEuSaveCertificate = unsafe extern "C" fn(*const c_char, c_ulong) -> c_ulong;
type FnEuSignDataInternal = unsafe extern "C" fn(
    c_int,
    *const c_char,
    c_ulong,
    *mut *mut c_char,
    *mut *mut c_void,
    *mut c_ulong,
) -> c_ulong;
type FnEuVerifyDataInternal = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_ulong,
    *mut *mut c_void,
    *mut c_ulong,
    *mut c_void,
) -> c_ulong;
type FnEuFreeMemory = unsafe extern "C" fn(*mut c_void);
type FnEuGetErrorDesc = unsafe extern "C" fn(c_int) -> *const c_char;

#[allow(non_camel_case_types)]
type c_char = i8;
#[allow(non_camel_case_types)]
type c_int = i32;
#[allow(non_camel_case_types)]
type c_ulong = u64;
#[allow(non_camel_case_types)]
type c_void = core::ffi::c_void;

/// Singleton-обгортка SDK (1:1 Python `IitSdk`): один екземпляр, Mutex на виклики.
pub struct IitSdk {
    lib: Library,
    /// Шлях до euscp.so — для робочих викликів (див. read_private_key_binary).
    lib_path: PathBuf,
    /// Каталог SDK (поряд з euscp.so) — для osplm.ini (1:1 Python _VENDOR_SDK_DIR).
    sdk_dir: PathBuf,
    initialized: bool,
    key_loaded: bool,
    /// Сертифікат підписанта (DER) — для get_signer_serial/get_signer_name.
    signer_cert_der: Option<Vec<u8>>,
    _guard: Mutex<()>,
}

impl IitSdk {
    /// Завантажує euscp.so (шлях — як у Python: backend/vendor/iit-sdk/...).
    pub fn load(lib_path: &Path) -> Result<Self, IitSdkError> {
        if !lib_path.is_file() {
            return Err(IitSdkError::LibraryNotFound(lib_path.to_path_buf()));
        }
        // SAFETY: euscp.so — стабільний C ABI; Library::new — стандартний FFI-підхід.
        let lib = unsafe { Library::new(lib_path) }
            .map_err(|e| IitSdkError::Load(lib_path.display().to_string(), e.to_string()))?;
        let sdk_dir = lib_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            lib,
            lib_path: lib_path.to_path_buf(),
            sdk_dir,
            initialized: false,
            key_loaded: false,
            signer_cert_der: None,
            _guard: Mutex::new(()),
        })
    }

    /// Чи є бібліотека (перевірка наявності функцій — smoke FFI).
    pub fn check_api(&self) -> Result<(), IitSdkError> {
        for name in [
            "EUInitialize",
            "EUSetSettingsFilePath",
            "EUSetFileStoreSettings",
            "EUGetJKSPrivateKeyFile",
            "EUSaveCertificate",
            "EUReadPrivateKeyBinary",
            "EUSignDataInternal",
            "EUVerifyDataInternal",
            "EUFreeMemory",
            "EUGetErrorDesc",
        ] {
            // SAFETY: лише перевірка наявності символу.
            unsafe {
                self.lib
                    .get::<*mut core::ffi::c_void>(name.as_bytes())
                    .map_err(|_| IitSdkError::FunctionNotFound(name.to_string()))?;
            }
        }
        Ok(())
    }

    /// EUInitialize (1:1 Python `_init`). Викликається один раз.
    pub fn initialize(
        &mut self,
        settings_path: Option<&Path>,
        cert_store: &Path,
    ) -> Result<(), IitSdkError> {
        if self.initialized {
            return Ok(());
        }
        // osplm.ini — налаштування SDK (1:1 Python: _VENDOR_SDK_DIR / "osplm.ini")
        let osplm = settings_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.sdk_dir.join("osplm.ini"));
        if osplm.is_file() {
            // rc ігнорується — 1:1 Python `_call_simple` (rc не перевіряється)
            let _ = self.call_settings_path(&osplm);
        }
        // файлове сховище сертифікатів (1:1 Python `_call_typed` — rc не
        // перевіряється; EUSetFileStoreSettings може повертати 1 без наслідків)
        let store_c = cstr(cert_store.to_string_lossy().as_ref());
        // SAFETY: EUSetFileStoreSettings — стабільний C ABI.
        unsafe {
            let f: libloading::Symbol<FnEuiSetFileStoreSettings> =
                self.lib.get(b"EUSetFileStoreSettings\0")?;
            let _rc = f(store_c.as_ptr() as *const u8, 0, 0, 0, 0, 0, 0, 3600);
        }

        // SAFETY: EUInitialize — без аргументів; rc перевіряється (як Python).
        let rc = unsafe {
            let f: libloading::Symbol<FnEuiInitialize> = self.lib.get(b"EUInitialize\0")?;
            f()
        };
        if rc != 0 {
            return Err(IitSdkError::Init(self.error_text(rc)));
        }
        self.initialized = true;
        Ok(())
    }

    fn call_settings_path(&self, path: &Path) -> Result<(), IitSdkError> {
        let p = cstr(path.to_string_lossy().as_ref());
        // SAFETY: EUSetSettingsFilePath(const char*) — стабільний C ABI.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuiSetSettingsFilePath> =
                self.lib.get(b"EUSetSettingsFilePath\0")?;
            f(p.as_ptr() as *const u8)
        };
        if rc != 0 {
            return Err(IitSdkError::Init(self.error_text(rc as i32)));
        }
        Ok(())
    }

    /// EUGetErrorDesc — текст помилки (1:1 Python `_error_text`).
    pub fn error_text(&self, code: c_int) -> String {
        // SAFETY: EUGetErrorDesc(int) → const char*; рядок читається до NUL.
        unsafe {
            let f: libloading::Symbol<FnEuGetErrorDesc> = match self.lib.get(b"EUGetErrorDesc\0") {
                Ok(f) => f,
                Err(_) => return format!("код {code}"),
            };
            let ptr = f(code);
            if ptr.is_null() {
                return format!("код {code}");
            }
            let cstr = std::ffi::CStr::from_ptr(ptr);
            String::from_utf8_lossy(cstr.to_bytes()).into_owned()
        }
    }

    /// EUFreeMemory (1:1 Python `_free`). Використовується ТІЛЬКИ для буферів
    /// підпису (EUSignDataInternal/EUVerifyDataInternal). Буфери
    /// EUGetJKSPrivateKeyFile НЕ звільняються (SDK тримає їх — dangling).
    ///
    /// # Safety
    /// ptr — вказівник, виділений SDK.
    pub unsafe fn free(&self, ptr: *mut c_void) {
        let f: libloading::Symbol<FnEuFreeMemory> = match self.lib.get(b"EUFreeMemory\0") {
            Ok(f) => f,
            Err(_) => return,
        };
        // SAFETY: викликається на буферах SDK (підпис).
        unsafe { f(ptr) };
    }

    /// EUGetJKSPrivateKeyFile — ключ + ланцюг сертифікатів з JKS.
    ///
    /// # Safety
    /// Внутрішній FFI-виклик; буфери SDK НЕ звільняються (SDK тримає їх).
    pub unsafe fn get_jks_private_key_file(
        &self,
        jks_path: &str,
        alias: Option<&str>,
    ) -> Result<(Vec<u8>, Vec<Vec<u8>>), IitSdkError> {
        type FnGetJksDirect = unsafe extern "C" fn(
            *const u8,
            *const u8,
            *mut *mut c_void,
            *mut u64,
            *mut u64,
            *mut *mut *mut c_void,
            *mut *mut u64,
        ) -> u64;
        let path_c = cstr(jks_path);
        let alias_c = alias.map(cstr);
        let mut key_ptr: *mut c_void = std::ptr::null_mut();
        let mut key_len: u64 = 0;
        let mut cert_cnt: u64 = 0;
        let mut certs_ptr: *mut *mut c_void = std::ptr::null_mut();
        let mut cert_lens_ptr: *mut u64 = std::ptr::null_mut();

        // SAFETY: EUGetJKSPrivateKeyFile — стабільний C ABI, вихідні буфери SDK.
        // SDK quirk (перевірено експериментально): getJKS викликається через
        // self.lib, а read — через свіжий Library::new (див. read_private_key_binary).
        let rc = unsafe {
            let f: libloading::Symbol<FnGetJksDirect> =
                self.lib.get(b"EUGetJKSPrivateKeyFile\0")?;
            f(
                path_c.as_ptr() as *const u8,
                alias_c
                    .as_ref()
                    .map_or(std::ptr::null(), |c| c.as_ptr() as *const u8),
                &mut key_ptr,
                &mut key_len,
                &mut cert_cnt,
                &mut certs_ptr,
                &mut cert_lens_ptr,
            )
        };
        if rc != 0 {
            return Err(IitSdkError::JksRead(self.error_text(rc as c_int)));
        }

        let mut key = Vec::new();
        if !key_ptr.is_null() && key_len > 0 {
            key = std::slice::from_raw_parts(key_ptr as *const u8, key_len as usize).to_vec();
        }
        let mut certs = Vec::new();
        for i in 0..cert_cnt as usize {
            let cp = unsafe { *certs_ptr.add(i) };
            let cl = unsafe { *cert_lens_ptr.add(i) };
            if !cp.is_null() && cl > 0 {
                let cert =
                    unsafe { std::slice::from_raw_parts(cp as *const u8, cl as usize) }.to_vec();
                certs.push(cert);
            }
        }

        // УВАГА: НЕ звільняємо буфери SDK через EUFreeMemory.
        // EUSignCP тримає ВНУТРІШНІ вказівники на key/certs після
        // EUGetJKSPrivateKeyFile — EUFreeMemory реально звільняє пам'ять,
        // і наступний EUReadPrivateKeyBinary отримує dangling pointer
        // (rc=24 або SIGSEGV). Python-еталон викликає _free(), але той
        // фактично no-op — тож Rust свідомо не free-шує: поведінка 1:1,
        // пам'ять SDK живе до кінця процесу (одноразове завантаження ключа).

        Ok((key, certs))
    }

    /// EUSaveCertificate — збереження сертифіката у файлове сховище SDK.
    pub fn save_certificate(&self, cert: &[u8]) -> Result<(), IitSdkError> {
        // SAFETY: EUSaveCertificate(const char*, ulong) — стабільний C ABI.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuSaveCertificate> =
                self.lib.get(b"EUSaveCertificate\0")?;
            f(cert.as_ptr() as *const c_char, cert.len() as c_ulong)
        };
        if rc != 0 {
            // Python лише логує warning — тут повертаємо Ok, щоб не блокувати
            // (сертифікат може вже існувати у сховищі).
            let _ = self.error_text(rc as c_int);
        }
        Ok(())
    }

    /// EUReadPrivateKeyBinary — завантаження ключа в ядро.
    pub fn read_private_key_binary(
        &mut self,
        key: &[u8],
        password: &str,
    ) -> Result<(), IitSdkError> {
        let pw = cstr(password);
        // SAFETY: EUReadPrivateKeyBinary(const char*, int, const char*) — C ABI.
        type FnReadDirect = unsafe extern "C" fn(*const u8, i32, *const u8) -> i32;
        let rc = unsafe {
            let f: libloading::Symbol<FnReadDirect> = self.lib.get(b"EUReadPrivateKeyBinary\0")?;
            f(key.as_ptr(), key.len() as i32, pw.as_ptr() as *const u8)
        };
        if rc != 0 {
            return Err(IitSdkError::KeyLoad(format!(
                "код {rc}: {}",
                self.error_text(rc as c_int)
            )));
        }
        self.key_loaded = true;
        Ok(())
    }

    /// EUSignDataInternal — CAdES-BES підпис (ДСТУ 4145 + Стрибог-256).
    /// 1:1 Python `sign_data_internal` (`ee.SignInternal(true, data)`).
    pub fn sign_data_internal(&self, data: &[u8]) -> Result<Vec<u8>, IitSdkError> {
        if !self.key_loaded {
            return Err(IitSdkError::KeyNotLoaded);
        }
        let mut b64_out: *mut c_char = std::ptr::null_mut();
        let mut sign_ptr: *mut c_void = std::ptr::null_mut();
        let mut sign_len: c_ulong = 0;

        // SAFETY: EUSignDataInternal — C ABI; вихідні буфери SDK.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuSignDataInternal> =
                self.lib.get(b"EUSignDataInternal\0")?;
            f(
                1, // bAppendCert
                data.as_ptr() as *const c_char,
                data.len() as c_ulong,
                &mut b64_out,
                &mut sign_ptr,
                &mut sign_len,
            )
        };
        if rc != 0 {
            return Err(IitSdkError::Sign(self.error_text(rc as c_int)));
        }

        // SDK повертає підпис як base64-рядок (як Java ee.SignInternal)
        if !b64_out.is_null() {
            // SAFETY: b64_out — NUL-terminated рядок SDK.
            let cstr = unsafe { std::ffi::CStr::from_ptr(b64_out) };
            let b64 = cstr.to_bytes();
            if !b64.is_empty() {
                use base64::Engine as _;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| IitSdkError::Sign(format!("base64: {e}")))?;
                // SAFETY: звільняємо пам'ять SDK.
                unsafe { self.free(b64_out as *mut c_void) };
                return Ok(decoded);
            }
        }
        if !sign_ptr.is_null() && sign_len > 0 {
            // SAFETY: sign_ptr — валідний буфер SDK.
            let sig =
                unsafe { std::slice::from_raw_parts(sign_ptr as *const u8, sign_len as usize) }
                    .to_vec();
            // SAFETY: звільняємо пам'ять SDK.
            unsafe { self.free(sign_ptr) };
            return Ok(sig);
        }
        Err(IitSdkError::EmptySignature)
    }

    /// EUVerifyDataInternal — перевірка CAdES-BES підпису (1:1 Python).
    /// `expected_data`: очікувані дані; None — лише валідність підпису.
    pub fn verify_data_internal(
        &self,
        signature: &[u8],
        expected_data: Option<&[u8]>,
    ) -> Result<bool, IitSdkError> {
        let mut data_out: *mut c_void = std::ptr::null_mut();
        let mut data_len: c_ulong = 0;

        // SAFETY: EUVerifyDataInternal — C ABI; вихідні буфери SDK.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuVerifyDataInternal> =
                self.lib.get(b"EUVerifyDataInternal\0")?;
            f(
                std::ptr::null(), // pszSignedData (base64) — NULL
                signature.as_ptr() as *const c_char,
                signature.len() as c_ulong,
                &mut data_out,
                &mut data_len,
                std::ptr::null_mut(), // pSignInfo
            )
        };
        if rc != 0 {
            return Ok(false); // Python: logger.warning + False
        }
        let verified =
            unsafe { std::slice::from_raw_parts(data_out as *const u8, data_len as usize) }
                .to_vec();
        // SAFETY: звільняємо пам'ять SDK.
        unsafe { self.free(data_out) };
        match expected_data {
            None => Ok(true),
            Some(expected) => Ok(verified == expected),
        }
    }

    pub fn key_loaded(&self) -> bool {
        self.key_loaded
    }

    /// Завантажує JKS-ключ (ДСТУ 4145) у крипто-ядро — 1:1 Python
    /// `IitSdk.load_jks_key`: EUGetJKSPrivateKeyFile → EUSaveCertificate × N →
    /// EUReadPrivateKeyBinary; запам'ятовує сертифікат підписанта.
    ///
    /// ВАЖЛИВО (SDK quirk, перевірено експериментально): FFI-виклики через
    /// `self.lib` (Symbol, отриманий з дескриптора IitSdk) дають rc=24
    /// ("невірний пароль") для EUReadPrivateKeyBinary, хоча через окремий
    /// `Library::new` — rc=0. Причина не в сигнатурах (ABI однаковий, адреси
    /// функцій ідентичні) — схоже на внутрішній стан EUSignCP. Робочий
    /// патерн (1:1 Python `ctypes.CDLL`, один дескриптор на процес):
    /// ОДИН локальний `Library` на весь load_jks_key і прямі виклики через
    /// нього. `self.lib` використовується для EUInitialize (працює).
    pub fn load_jks_key(&mut self, jks_path: &Path, password: &str) -> Result<(), IitSdkError> {
        let path_str = jks_path.to_string_lossy().to_string();
        // SAFETY: euscp.so — стабільний C ABI.
        let lib = unsafe { Library::new(&self.lib_path) }
            .map_err(|e| IitSdkError::Load(self.lib_path.display().to_string(), e.to_string()))?;

        type FnGetJks = unsafe extern "C" fn(
            *const u8,
            *const u8,
            *mut *mut c_void,
            *mut u64,
            *mut u64,
            *mut *mut *mut c_void,
            *mut *mut u64,
        ) -> u64;
        type FnSave = unsafe extern "C" fn(*const u8, u64) -> u64;
        type FnRead = unsafe extern "C" fn(*const u8, i32, *const u8) -> i32;

        // 1) ключ + сертифікати з JKS
        let path_c = cstr(&path_str);
        let mut key_ptr: *mut c_void = std::ptr::null_mut();
        let mut key_len: u64 = 0;
        let mut cert_cnt: u64 = 0;
        let mut certs_ptr: *mut *mut c_void = std::ptr::null_mut();
        let mut cert_lens_ptr: *mut u64 = std::ptr::null_mut();
        let rc = unsafe {
            let f: libloading::Symbol<FnGetJks> = lib.get(b"EUGetJKSPrivateKeyFile\0")?;
            f(
                path_c.as_ptr() as *const u8,
                std::ptr::null(),
                &mut key_ptr,
                &mut key_len,
                &mut cert_cnt,
                &mut certs_ptr,
                &mut cert_lens_ptr,
            )
        };
        if rc != 0 {
            return Err(IitSdkError::JksRead(self.error_text(rc as i32)));
        }
        let mut key = Vec::new();
        if !key_ptr.is_null() && key_len > 0 {
            key = unsafe { std::slice::from_raw_parts(key_ptr as *const u8, key_len as usize) }
                .to_vec();
        }
        let mut certs = Vec::new();
        for i in 0..cert_cnt as usize {
            let cp = unsafe { *certs_ptr.add(i) };
            let cl = unsafe { *cert_lens_ptr.add(i) };
            if !cp.is_null() && cl > 0 {
                let cert =
                    unsafe { std::slice::from_raw_parts(cp as *const u8, cl as usize) }.to_vec();
                certs.push(cert);
            }
        }
        // Буфери SDK НЕ звільняємо (EUFreeMemory дає dangling pointer —
        // див. коментар у get_jks_private_key_file).

        // 2) сертифікати у файлове сховище (помилка — лише warning, як Python)
        for cert in &certs {
            let rc = unsafe {
                let f: libloading::Symbol<FnSave> = lib.get(b"EUSaveCertificate\0")?;
                f(cert.as_ptr(), cert.len() as u64)
            };
            if rc != 0 {
                let _ = self.error_text(rc as i32);
            }
        }

        // 3) ключ у ядро
        let pw = cstr(password);
        let rc = unsafe {
            let f: libloading::Symbol<FnRead> = lib.get(b"EUReadPrivateKeyBinary\0")?;
            f(key.as_ptr(), key.len() as i32, pw.as_ptr() as *const u8)
        };
        if rc != 0 {
            return Err(IitSdkError::KeyLoad(format!(
                "код {rc}: {}",
                self.error_text(rc)
            )));
        }
        self.key_loaded = true;
        self.signer_cert_der = find_signer_cert(&certs);
        tracing::info!(
            "PRRO_IIT_SDK | ключ JKS завантажено: {} (cert={})",
            jks_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            self.signer_cert_der.is_some()
        );
        Ok(())
    }

    /// Серійний номер сертифіката підписанта (hex, upper) — 1:1 Python.
    pub fn get_signer_serial(&self) -> Result<String, IitSdkError> {
        let der = self
            .signer_cert_der
            .as_deref()
            .ok_or(IitSdkError::KeyNotLoaded)?;
        crate::crypto::xades::serial_from_cert(der).map_err(|e| IitSdkError::Generic(e.to_string()))
    }

    /// ПІБ підписанта з сертифіката — 1:1 Python.
    pub fn get_signer_name(&self) -> Result<String, IitSdkError> {
        let Some(der) = self.signer_cert_der.as_deref() else {
            return Ok(String::new());
        };
        crate::crypto::xades::name_from_cert(der).map_err(|e| IitSdkError::Generic(e.to_string()))
    }
}

/// Знаходить сертифікат підписанта в ланцюгу JKS — 1:1 Python
/// `_find_signer_cert`: кінцевий (не CA, subject != issuer), інакше перший.
fn find_signer_cert(certs: &[Vec<u8>]) -> Option<Vec<u8>> {
    let parsed: Vec<(&[u8], x509_parser::certificate::X509Certificate)> = certs
        .iter()
        .filter_map(|c| {
            x509_parser::parse_x509_certificate(c)
                .ok()
                .map(|(_, cert)| (c.as_slice(), cert))
        })
        .collect();
    for (raw, cert) in &parsed {
        // subject != issuer (порівняння за RFC 4514 — 1:1 Python)
        if cert.subject().to_string() == cert.issuer().to_string() {
            continue; // кореневий ЦСК
        }
        match cert.basic_constraints() {
            Ok(Some(bc)) if bc.value.ca => continue, // проміжний ЦСК
            _ => return Some(raw.to_vec()),          // кінцевий (CA=False або без BC)
        }
    }
    parsed.first().map(|(raw, _)| raw.to_vec())
}

/// С-рядок з Rust-рядка (без NUL у Rust-частині; буфер із завершальним NUL).
fn cstr(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).expect("NUL у C-рядку")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::default_iit_sdk_path;

    /// Smoke FFI: SDK доступний на цій машині → API-сигнатури на місці.
    /// Пропускається, якщо euscp.so не встановлено (vendor поза git).
    #[test]
    fn iit_sdk_api_available_if_installed() {
        let Some(so) = default_iit_sdk_path() else {
            eprintln!("SKIP: euscp.so не встановлено (backend/vendor/iit-sdk)");
            return;
        };
        let sdk = IitSdk::load(&so).expect("euscp.so завантажується");
        sdk.check_api().expect("всі 10 функцій SDK наявні");
    }
}
