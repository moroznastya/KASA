//! Крипто-шар ПРРО (ADR-014): ДСТУ 4145 → FFI до IIT SDK EUSignCP,
//! RSA/ECDSA → чистий Rust (XAdES, 7.2). Тут — контракт і фабрика.

pub mod iit;

use std::path::Path;

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
