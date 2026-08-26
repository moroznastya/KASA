//! Крипто-шар ПРРО (ADR-014): ДСТУ 4145 → FFI до IIT SDK EUSignCP,
//! RSA/ECDSA → чистий Rust (XAdES, 7.2). Тут — контракт і фабрика.

pub mod iit;
pub mod xades;

use std::path::Path;

use crate::keystore::KeyMaterial;

pub use xades::{XadesKey, XadesSigner};

/// Результат підпису/перевірки.
#[derive(Debug, thiserror::Error)]
pub enum PrroCryptoError {
    #[error("Помилка крипто-шару: {0}")]
    Generic(String),
    #[error("IIT SDK: {0}")]
    Iit(#[from] iit::IitSdkError),
    #[error("Ключ не завантажено в крипто-ядро: {0}")]
    KeyNotLoaded(String),
    #[error("Непідтримуваний формат ключа для бекенда: {0}")]
    UnsupportedFormat(String),
}

/// Контракт підписанта (7.2: реалізації — IitSigner, XadesSigner).
/// `Send + Sync` — потрібно для axum-хендлерів (future має бути Send).
pub trait PrroSigner: Send + Sync {
    /// Підписує XML-документ СЗЗД. Для ДСТУ 4145 — CAdES-BES (ContentInfo),
    /// для RSA — XAdES-BES enveloped. 1:1 Python `PrroCryptoSigner.sign`.
    fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError>;

    /// Перевіряє підпис. 1:1 Python `PrroCryptoSigner.verify`.
    fn verify(&self, signed_xml: &[u8]) -> Result<bool, PrroCryptoError>;

    /// Серійний номер сертифіката підписанта (hex, upper).
    fn get_serial_number(&self) -> Result<String, PrroCryptoError>;

    /// ПІБ підписанта з сертифіката.
    fn get_signer_name(&self) -> Result<String, PrroCryptoError>;
}

/// Створює підписанта з ключового матеріалу key_store — 1:1 Python
/// `PrroCryptoSigner._load_key_material` (вибір бекенда за OID алгоритму):
/// - ДСТУ 4145-2002 (JKS) → FFI до IIT SDK EUSignCP (CAdES-BES);
/// - RSA/ECDSA (PKCS#8) → чистий Rust XAdES-BES enveloped.
pub fn signer_from_key_material(
    material: &KeyMaterial,
    key_password: &str,
) -> Result<Box<dyn PrroSigner>, PrroCryptoError> {
    let oid = material.algorithm_oid.as_deref().unwrap_or("");
    let is_dstu = oid == "1.2.804.2.1.1.1.1.3.1.1" || oid == "1.2.804.2.1.1.1.1.3.1.2";
    if is_dstu {
        // SDK НЕ завантажується в ОСНОВНИЙ процес: усі FFI-виклики EUSignCP
        // йдуть у ІЗОЛЬОВАНИЙ субпроцес (див. iit::sign_via_subprocess).
        // Крах багнутого cspb.so (#GP, release, offset 0x7a925 — відтворено)
        // вбиває лише хелпер; Torgashka отримує чисту помилку → HTTP 400.
        // Тут лише перевірка наявності euscp.so (шлях — без FFI).
        default_iit_sdk_path().ok_or_else(|| PrroCryptoError::UnsupportedFormat(
            "ДСТУ 4145: не встановлено IIT SDK (euscp.so). Запустіть backend/scripts/setup_iit_sdk.sh".into(),
        ))?;
        let key_path = material
            .key_path
            .as_deref()
            .ok_or_else(|| PrroCryptoError::Generic("key_path не задано для JKS".into()))?;
        // Сертифікат підписанта (для get_serial_number/get_signer_name) —
        // чистий Rust-парсинг, без SDK.
        let signer_cert_der = crate::keystore::find_signer_cert(&material.certs)
            .or_else(|| material.certs.first().cloned());
        return Ok(Box::new(IitSigner {
            key_path: Path::new(key_path).to_path_buf(),
            key_password: key_password.to_string(),
            signer_cert_der,
        }));
    }
    let cert_der = material
        .certs
        .first()
        .ok_or_else(|| PrroCryptoError::Generic("Сертифікат не завантажено".into()))?
        .clone();
    let key_der = material
        .private_key_der
        .as_deref()
        .ok_or_else(|| PrroCryptoError::Generic("Приватний ключ не завантажено".into()))?;
    Ok(Box::new(XadesSigner::from_pkcs8_der(key_der, cert_der)?))
}

/// CAdES-BES підписант (ДСТУ 4145) — 1:1 Python `PrroCryptoSigner` (бекенд iit).
///
/// НЕ тримає SDK у пам'яті: кожен sign/verify запускає ізольований субпроцес
/// (той самий бінарник у режимі SDK_HELPER_ENV), який завантажує euscp.so,
/// ініціалізує ядро, читає JKS і підписує. Якщо SDK впаде (#GP/SIGSEGV) —
/// вмирає лише хелпер, основний процес повертає чисту помилку.
pub struct IitSigner {
    /// Шлях до JKS-ключа (для субпроцесу).
    key_path: std::path::PathBuf,
    /// Пароль ключа (для субпроцесу).
    key_password: String,
    /// Сертифікат підписанта (DER) — для get_serial_number/get_signer_name
    /// (чистий Rust, без SDK).
    signer_cert_der: Option<Vec<u8>>,
}

impl PrroSigner for IitSigner {
    fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        iit::sign_via_subprocess(&self.key_path, &self.key_password, xml_bytes)
            .map_err(PrroCryptoError::Iit)
    }

    fn verify(&self, signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
        iit::verify_via_subprocess(&self.key_path, &self.key_password, signed_xml, None)
            .map_err(PrroCryptoError::Iit)
    }

    fn get_serial_number(&self) -> Result<String, PrroCryptoError> {
        let der = self.signer_cert_der.as_deref().ok_or_else(|| {
            PrroCryptoError::KeyNotLoaded("сертифікат підписанта недоступний".into())
        })?;
        crate::crypto::xades::serial_from_cert(der)
            .map_err(|e| PrroCryptoError::Generic(e.to_string()))
    }

    fn get_signer_name(&self) -> Result<String, PrroCryptoError> {
        let Some(der) = self.signer_cert_der.as_deref() else {
            return Ok(String::new());
        };
        crate::crypto::xades::name_from_cert(der)
            .map_err(|e| PrroCryptoError::Generic(e.to_string()))
    }
}

/// Шлях до файлового сховища сертифікатів SDK — 1:1 Python
/// `iit_sdk._DEFAULT_CERT_STORE` (vendor/iit-sdk/certs).
pub fn default_iit_cert_store() -> Option<std::path::PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())?;
    let dir = repo_root.join("backend/vendor/iit-sdk/certs");
    dir.is_dir().then_some(dir)
}

/// Шлях до IIT SDK EUSignCP (euscp.so) — 1:1 Python `iit_sdk._VENDOR_SDK_DIR`.
pub fn default_iit_sdk_path() -> Option<std::path::PathBuf> {
    // torgashka-prro crate: frontend/src-tauri/crates/torgashka-prro → repo root torgashka/
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())?;
    let so = repo_root.join("backend/vendor/iit-sdk/opt/iit/eu/sw/euscp.so");
    so.is_file().then_some(so)
}
