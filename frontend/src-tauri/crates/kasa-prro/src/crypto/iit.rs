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
}

/// C-сигнатури EUSignCP (1:1 Python iit_sdk.py).
type FnEuiInitialize = unsafe extern "C" fn() -> c_int;
type FnEuiSetSettingsFilePath = unsafe extern "C" fn(*const c_char) -> c_ulong;
type FnEuiSetFileStoreSettings =
    unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, c_int, c_ulong) -> c_ulong;
type FnEuGetJksPrivateKeyFile = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *mut *mut c_void,
    *mut c_ulong,
    *mut c_ulong,
    *mut *mut *mut c_void,
    *mut *mut c_ulong,
) -> c_ulong;
type FnEuSaveCertificate = unsafe extern "C" fn(*const c_char, c_ulong) -> c_ulong;
type FnEuReadPrivateKeyBinary = unsafe extern "C" fn(*const c_char, c_int, *const c_char) -> c_int;
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
    initialized: bool,
    key_loaded: bool,
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
        Ok(Self {
            lib,
            initialized: false,
            key_loaded: false,
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
        std::fs::create_dir_all(cert_store).map_err(|e| IitSdkError::Init(e.to_string()))?;

        if let Some(osplm) = settings_path {
            if osplm.is_file() {
                self.call_settings_path(osplm)?;
            }
        }
        // файлове сховище сертифікатів (1:1 Python: store_path, 0,0,0,0,0, 3600)
        let store_c = cstr(cert_store.to_string_lossy().as_ref());
        // SAFETY: EUSetFileStoreSettings — стабільний C ABI.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuiSetFileStoreSettings> =
                self.lib.get(b"EUSetFileStoreSettings\0")?;
            f(store_c.as_ptr(), 0, 0, 0, 0, 0, 3600)
        };
        if rc != 0 {
            return Err(IitSdkError::Init(self.error_text(rc as c_int)));
        }

        // SAFETY: EUInitialize — без аргументів.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuiInitialize> = self.lib.get(b"EUInitialize\0")?;
            f()
        };
        if rc != 0 {
            return Err(IitSdkError::Init(self.error_text(rc as c_int)));
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
            f(p.as_ptr())
        };
        if rc != 0 {
            return Err(IitSdkError::Init(self.error_text(rc as c_int)));
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

    /// EUFreeMemory (1:1 Python `_free`).
    ///
    /// # Safety
    /// `ptr` має бути вказівником, виділеним SDK.
    pub unsafe fn free(&self, ptr: *mut c_void) {
        if ptr.is_null() {
            return;
        }
        // SAFETY: викликається з валідним SDK-вказівником.
        let f: libloading::Symbol<FnEuFreeMemory> = match self.lib.get(b"EUFreeMemory\0") {
            Ok(f) => f,
            Err(_) => return,
        };
        f(ptr);
    }

    /// EUGetJKSPrivateKeyFile — ключ + ланцюг сертифікатів з JKS.
    ///
    /// # Safety
    /// Внутрішній FFI-виклик; пам'ять звільняється через `free`.
    pub unsafe fn get_jks_private_key_file(
        &self,
        jks_path: &str,
        alias: Option<&str>,
    ) -> Result<(Vec<u8>, Vec<Vec<u8>>), IitSdkError> {
        let path_c = cstr(jks_path);
        let alias_c = alias.map(cstr);
        let mut key_ptr: *mut c_void = std::ptr::null_mut();
        let mut key_len: c_ulong = 0;
        let mut cert_cnt: c_ulong = 0;
        let mut certs_ptr: *mut *mut c_void = std::ptr::null_mut();
        let mut cert_lens_ptr: *mut c_ulong = std::ptr::null_mut();

        // SAFETY: EUGetJKSPrivateKeyFile — стабільний C ABI, вихідні буфери SDK.
        let rc = unsafe {
            let f: libloading::Symbol<FnEuGetJksPrivateKeyFile> =
                self.lib.get(b"EUGetJKSPrivateKeyFile\0")?;
            f(
                path_c.as_ptr(),
                alias_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
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

        // копіюємо дані до звільнення пам'яті
        let mut key = Vec::new();
        if !key_ptr.is_null() && key_len > 0 {
            // SAFETY: key_ptr — валідний буфер SDK довжини key_len.
            key = std::slice::from_raw_parts(key_ptr as *const u8, key_len as usize).to_vec();
        }
        let mut certs = Vec::new();
        for i in 0..cert_cnt as usize {
            let cp = unsafe { *certs_ptr.add(i) };
            let cl = unsafe { *cert_lens_ptr.add(i) };
            if !cp.is_null() && cl > 0 {
                // SAFETY: cp — валідний буфер SDK довжини cl.
                let cert =
                    unsafe { std::slice::from_raw_parts(cp as *const u8, cl as usize) }.to_vec();
                certs.push(cert);
            }
        }

        // SAFETY: звільняємо пам'ять, виділену SDK.
        unsafe {
            if !key_ptr.is_null() {
                self.free(key_ptr);
            }
            if !certs_ptr.is_null() {
                for i in 0..cert_cnt as usize {
                    let cp = *certs_ptr.add(i);
                    if !cp.is_null() {
                        self.free(cp);
                    }
                }
                self.free(certs_ptr as *mut c_void);
            }
            if !cert_lens_ptr.is_null() {
                self.free(cert_lens_ptr as *mut c_void);
            }
        }

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
        let rc = unsafe {
            let f: libloading::Symbol<FnEuReadPrivateKeyBinary> =
                self.lib.get(b"EUReadPrivateKeyBinary\0")?;
            f(
                key.as_ptr() as *const c_char,
                key.len() as c_int,
                pw.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(IitSdkError::KeyLoad(self.error_text(rc as c_int)));
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
