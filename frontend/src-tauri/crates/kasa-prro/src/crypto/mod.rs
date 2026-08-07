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
pub trait PrroSigner {
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
    use crate::crypto::iit::IitSdk;
    let oid = material.algorithm_oid.as_deref().unwrap_or("");
    let is_dstu = oid == "1.2.804.2.1.1.1.1.3.1.1" || oid == "1.2.804.2.1.1.1.1.3.1.2";
    if is_dstu {
        let so = default_iit_sdk_path()
            .ok_or_else(|| PrroCryptoError::UnsupportedFormat(
                "ДСТУ 4145: не встановлено IIT SDK (euscp.so). Запустіть backend/scripts/setup_iit_sdk.sh".into(),
            ))?;
        let store = default_iit_cert_store()
            .ok_or_else(|| PrroCryptoError::Generic("vendor/iit-sdk/certs не знайдено".into()))?;
        let mut sdk = IitSdk::load(&so).map_err(PrroCryptoError::Iit)?;
        sdk.initialize(None, &store).map_err(PrroCryptoError::Iit)?;
        let key_path = material
            .key_path
            .as_deref()
            .ok_or_else(|| PrroCryptoError::Generic("key_path не задано для JKS".into()))?;
        sdk.load_jks_key(Path::new(key_path), key_password)
            .map_err(PrroCryptoError::Iit)?;
        return Ok(Box::new(IitSigner { sdk }));
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

/// CAdES-BES підписант (ДСТУ 4145) — обгортка над IIT SDK, 1:1 Python
/// `PrroCryptoSigner` (бекенд iit).
pub struct IitSigner {
    sdk: iit::IitSdk,
}

impl PrroSigner for IitSigner {
    fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        self.sdk
            .sign_data_internal(xml_bytes)
            .map_err(PrroCryptoError::Iit)
    }

    fn verify(&self, signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
        self.sdk
            .verify_data_internal(signed_xml, None)
            .map_err(PrroCryptoError::Iit)
    }

    fn get_serial_number(&self) -> Result<String, PrroCryptoError> {
        self.sdk.get_signer_serial().map_err(PrroCryptoError::Iit)
    }

    fn get_signer_name(&self) -> Result<String, PrroCryptoError> {
        self.sdk.get_signer_name().map_err(PrroCryptoError::Iit)
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
    // kasa-prro crate: frontend/src-tauri/crates/kasa-prro → repo root kasa/
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())?;
    let so = repo_root.join("backend/vendor/iit-sdk/opt/iit/eu/sw/euscp.so");
    so.is_file().then_some(so)
}
