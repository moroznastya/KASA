//! Читання КЕП: JKS / PKCS#12 / PEM — 1:1 Python `crypto_signer.py`.
//!
//! Авто-визначення формату (розширення + магічні байти), витяг приватного
//! ключа (DER PKCS#8) + сертифікатів (DER X.509) + OID алгоритму ключа
//! (для вибору крипто-бекенда: ДСТУ 4145 → IIT SDK, RSA/EC → чистий Rust).

use crate::jks;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFormat {
    Jks,
    Pkcs12,
    Pem,
}

impl KeyFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jks => "jks",
            Self::Pkcs12 => "p12",
            Self::Pem => "pem",
        }
    }
}

/// Матеріали ключа КЕП (приватний ключ + сертифікати + алгоритм).
#[derive(Debug, Clone, Default)]
pub struct KeyMaterial {
    pub format: Option<KeyFormat>,
    /// Приватний ключ у DER (PKCS#8) — якщо формат дозволяє витягти.
    pub private_key_der: Option<Vec<u8>>,
    /// Усі сертифікати ланцюга (DER X.509).
    pub certs: Vec<Vec<u8>>,
    /// OID алгоритму приватного ключа (з PKCS#8), напр. ДСТУ 4145.
    pub algorithm_oid: Option<String>,
    /// Шлях до файлу ключа (для IIT SDK: EUGetJKSPrivateKeyFile).
    pub key_path: Option<String>,
    /// Пароль (для IIT SDK: EUReadPrivateKeyBinary).
    pub key_password: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum KeyStoreError {
    #[error("Файл ключа не знайдено: {0}")]
    NotFound(String),
    #[error("Не вдалося прочитати ключ: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "Невідоме розширення ключа: {0:?}. Підтримуються: .jks, .p12, .pfx, .pem, .crt, .cer, .key"
    )]
    UnknownExtension(String),
    #[error("JKS: {0}")]
    Jks(#[from] jks::JksError),
    #[error("PKCS#12: {0}")]
    Pkcs12(String),
    #[error("PEM: {0}")]
    Pem(String),
    #[error("У ключі відсутній сертифікат X.509")]
    NoCertificate,
    #[error("У ключі відсутній приватний ключ")]
    NoPrivateKey,
    #[error("X.509: {0}")]
    X509(String),
    #[error("OpenSSL: {0}")]
    OpenSsl(String),
}

/// OID ДСТУ 4145-2002 (little/big endian) — 1:1 Python `crypto_signer`.
pub const OID_DSTU4145_LE: &str = "1.2.804.2.1.1.1.1.3.1.1";
pub const OID_DSTU4145_BE: &str = "1.2.804.2.1.1.1.1.3.1.2";

/// Визначає, чи OID алгоритму є ДСТУ 4145-2002.
pub fn is_dstu4145(oid: Option<&str>) -> bool {
    matches!(oid, Some(OID_DSTU4145_LE) | Some(OID_DSTU4145_BE))
}

/// Розширення → формат (1:1 Python `_EXTENSION_FORMATS`).
fn extension_format(suffix: &str) -> Option<KeyFormat> {
    match suffix.to_lowercase().as_str() {
        ".pfx" | ".p12" => Some(KeyFormat::Pkcs12),
        ".jks" => Some(KeyFormat::Jks),
        ".pem" | ".crt" | ".cer" | ".key" => Some(KeyFormat::Pem),
        _ => None,
    }
}

/// Авто-визначення формату: розширення + сигнатура (1:1 Python `detect_format`).
pub fn detect_format(path: &Path) -> Result<KeyFormat, KeyStoreError> {
    let suffix = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let fmt = extension_format(&suffix).ok_or(KeyStoreError::UnknownExtension(suffix))?;

    // Додаткова перевірка сигнатури (перші байти) — Python лише логує warning,
    // формат не змінюється; тут сигнатура перевіряється в detect (без зміни).
    let head = std::fs::read(path)
        .map_err(KeyStoreError::Io)?
        .into_iter()
        .take(16)
        .collect::<Vec<_>>();

    match fmt {
        KeyFormat::Jks => {
            if !(head.starts_with(&jks::JKS_MAGIC) || head.starts_with(&jks::JCEKS_MAGIC)) {
                // Python: logger.warning — формат залишається
            }
        }
        KeyFormat::Pkcs12 => {
            if !head.starts_with(&[0x30]) {
                // Python: logger.warning
            }
        }
        KeyFormat::Pem => {
            if !head.starts_with(b"-----BEGIN") {
                // Python: logger.warning
            }
        }
    }
    Ok(fmt)
}

/// Завантажує матеріали ключа (1:1 Python `PrroCryptoSigner._load_key_material`).
pub fn load_key_material(
    path: &Path,
    password: &str,
    forced_format: Option<KeyFormat>,
) -> Result<KeyMaterial, KeyStoreError> {
    if !path.is_file() {
        return Err(KeyStoreError::NotFound(path.display().to_string()));
    }
    let format = forced_format.unwrap_or(detect_format(path)?);

    let mut material = KeyMaterial {
        format: Some(format),
        key_path: Some(path.display().to_string()),
        key_password: Some(password.to_string()),
        ..Default::default()
    };

    match format {
        KeyFormat::Jks => {
            let entry = jks::load_jks_private_key(path, password)?;
            material.private_key_der = Some(entry.key_der.clone());
            material.certs = entry.cert_chain.clone();
            material.algorithm_oid = detect_pkcs8_algorithm(&entry.key_der);
        }
        KeyFormat::Pkcs12 => {
            let (key_der, certs) = load_pkcs12(path, password)?;
            material.private_key_der = Some(key_der.clone());
            material.certs = certs;
            material.algorithm_oid = detect_pkcs8_algorithm(&key_der);
        }
        KeyFormat::Pem => {
            let (key_der, certs) = load_pem(path, password)?;
            material.private_key_der = Some(key_der.clone());
            material.certs = certs;
            material.algorithm_oid = detect_pkcs8_algorithm(&key_der);
        }
    }

    Ok(material)
}

/// Витягує OID алгоритму з PKCS#8 DER (вручну — ДСТУ-параметри не розуміються
/// стандартними парсерами). 1:1 Python `_detect_pkcs8_algorithm`.
///
/// Структура: SEQUENCE { INTEGER 0, SEQUENCE { OBJECT oid, ... }, ... }
pub fn detect_pkcs8_algorithm(key_der: &[u8]) -> Option<String> {
    let mut i = 0usize;
    if key_der.get(i)? != &0x30 {
        return None;
    } // SEQUENCE
    i += 1;
    let ln = *key_der.get(i)?;
    i += 1;
    if ln & 0x80 != 0 {
        i += (ln & 0x7F) as usize;
    }
    if key_der.get(i)? != &0x02 {
        return None;
    } // INTEGER (version)
    i += 1;
    let ln = *key_der.get(i)?;
    i += 1;
    if ln & 0x80 != 0 {
        i += (ln & 0x7F) as usize;
    }
    i += ln as usize;
    if key_der.get(i)? != &0x30 {
        return None;
    } // SEQUENCE (AlgorithmIdentifier)
    i += 1;
    let ln = *key_der.get(i)?;
    i += 1;
    if ln & 0x80 != 0 {
        i += (ln & 0x7F) as usize;
    }
    if key_der.get(i)? != &0x06 {
        return None;
    } // OBJECT (OID)
    i += 1;
    let oid_len = *key_der.get(i)? as usize;
    i += 1;
    let oid_bytes = key_der.get(i..i + oid_len)?;

    Some(der_oid_to_string(oid_bytes))
}

/// Декодує DER-кодування OID у рядок.
fn der_oid_to_string(oid: &[u8]) -> String {
    let first = oid[0];
    let mut parts = Vec::new();
    if first < 80 {
        parts.push((first / 40).to_string());
        parts.push((first % 40).to_string());
    } else {
        parts.push("2".to_string());
        parts.push((first - 80).to_string());
    }
    let mut val: u64 = 0;
    let mut started = false;
    for &b in &oid[1..] {
        val = (val << 7) | (b & 0x7F) as u64;
        started = true;
        if b & 0x80 == 0 {
            parts.push(val.to_string());
            val = 0;
            started = false;
        }
    }
    if started {
        parts.push(val.to_string());
    }
    parts.join(".")
}

// ─── PKCS#12 ─────────────────────────────────────────────────────────────────

fn load_pkcs12(path: &Path, password: &str) -> Result<(Vec<u8>, Vec<Vec<u8>>), KeyStoreError> {
    use openssl::pkcs12::Pkcs12;
    let data = std::fs::read(path).map_err(KeyStoreError::Io)?;
    let pkcs12 = Pkcs12::from_der(&data)
        .map_err(|e| KeyStoreError::Pkcs12(format!("не вдалося розібрати контейнер: {e}")))?;
    let parsed = pkcs12
        .parse2(password)
        .map_err(|e| KeyStoreError::Pkcs12(format!("не вдалося відкрити контейнер: {e}")))?;

    // Приватний ключ → DER PKCS#8
    let key_der = parsed
        .pkey
        .as_ref()
        .map(|pkey| {
            pkey.private_key_to_der()
                .map_err(|e| KeyStoreError::OpenSsl(e.to_string()))
        })
        .transpose()?
        .ok_or(KeyStoreError::NoPrivateKey)?;

    // Сертифікати: основний + CA-ланцюг
    let mut certs: Vec<Vec<u8>> = Vec::new();
    if let Some(cert) = &parsed.cert {
        certs.push(
            cert.to_der()
                .map_err(|e| KeyStoreError::OpenSsl(e.to_string()))?,
        );
    }
    if let Some(ca) = &parsed.ca {
        for c in ca.iter() {
            certs.push(
                c.to_der()
                    .map_err(|e| KeyStoreError::OpenSsl(e.to_string()))?,
            );
        }
    }

    Ok((key_der, certs))
}

// ─── PEM ─────────────────────────────────────────────────────────────────────

fn load_pem(path: &Path, _password: &str) -> Result<(Vec<u8>, Vec<Vec<u8>>), KeyStoreError> {
    let data = std::fs::read_to_string(path).map_err(KeyStoreError::Io)?;
    let parsed = pem::parse_many(data.as_bytes()).map_err(|e| KeyStoreError::Pem(e.to_string()))?;

    // Приватний ключ: перший блок PRIVATE KEY (PKCS#8) або RSA/EC PRIVATE KEY
    let mut key_der: Option<Vec<u8>> = None;
    let mut certs: Vec<Vec<u8>> = Vec::new();
    let mut found_private = false;

    for p in &parsed {
        match p.tag() {
            "PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY" | "ENCRYPTED PRIVATE KEY" => {
                if found_private {
                    continue;
                }
                let der = p.contents().to_vec();
                if p.tag() == "RSA PRIVATE KEY" {
                    key_der = Some(rsa_pkcs1_to_pkcs8(&der)?);
                } else if p.tag() == "EC PRIVATE KEY" {
                    return Err(KeyStoreError::Pem(
                        "EC PRIVATE KEY (SEC1) не підтримується напряму: конвертуйте в PKCS#8"
                            .into(),
                    ));
                } else if p.tag() == "ENCRYPTED PRIVATE KEY" {
                    return Err(KeyStoreError::Pem(
                        "Зашифрований PEM-ключ не підтримується: використовуйте незашифрований PKCS#8".into(),
                    ));
                } else {
                    key_der = Some(der);
                }
                found_private = true;
            }
            "CERTIFICATE" => certs.push(p.contents().to_vec()),
            _ => {}
        }
    }

    // Якщо ключа немає в файлі — спробувати сусідні .key
    if key_der.is_none() {
        for candidate in [path.with_extension("key"), path.with_extension("pem")] {
            if candidate.is_file() && candidate != path {
                if let Ok(s) = std::fs::read_to_string(&candidate) {
                    if let Ok(parsed2) = pem::parse_many(s.as_bytes()) {
                        for p in &parsed2 {
                            if p.tag().contains("PRIVATE KEY") {
                                key_der = Some(p.contents().to_vec());
                                found_private = true;
                                break;
                            }
                        }
                    }
                }
            }
            if found_private {
                break;
            }
        }
    }

    // Сертифікат: спершу в тому ж файлі, потім .crt/.cer (1:1 Python)
    if certs.is_empty() {
        for candidate in [path.with_extension("crt"), path.with_extension("cer")] {
            if candidate.is_file() {
                if let Ok(s) = std::fs::read_to_string(&candidate) {
                    if let Ok(parsed2) = pem::parse_many(s.as_bytes()) {
                        for p in &parsed2 {
                            if p.tag() == "CERTIFICATE" {
                                certs.push(p.contents().to_vec());
                                break;
                            }
                        }
                    }
                }
            }
            if !certs.is_empty() {
                break;
            }
        }
    }

    if certs.is_empty() {
        return Err(KeyStoreError::NoCertificate);
    }
    let key_der = key_der.ok_or(KeyStoreError::NoPrivateKey)?;
    Ok((key_der, certs))
}

/// PKCS#1 RSA → PKCS#8 (обгортка SEQUENCE { 0, rsaEncryption, NULL, OCTET STRING key }).
fn rsa_pkcs1_to_pkcs8(pkcs1: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0x02, 0x01, 0x00]); // INTEGER 0
                                                 // AlgorithmIdentifier: SEQUENCE { OID 1.2.840.113549.1.1.1, NULL }
    let oid: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
    let alg_inner = [&[0x06, 0x09][..], oid, &[0x05, 0x00][..]].concat();
    body.push(0x30);
    body.push(alg_inner.len() as u8);
    body.extend_from_slice(&alg_inner);
    // OCTET STRING pkcs1
    if pkcs1.len() > 127 {
        return Err(KeyStoreError::Pem(
            "PKCS#1 ключ завеликий для простого обгортання".into(),
        ));
    }
    body.push(0x04);
    body.push(pkcs1.len() as u8);
    body.extend_from_slice(pkcs1);
    let mut out = vec![0x30];
    out.push(body.len() as u8);
    out.extend_from_slice(&body);
    Ok(out)
}

// ─── Допоміжні функції для сертифікатів ─────────────────────────────────────

/// Серійний номер сертифіката (hex, upper) — 1:1 Python `get_serial_number`.
pub fn cert_serial_hex(cert_der: &[u8]) -> Result<String, KeyStoreError> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|e| KeyStoreError::X509(e.to_string()))?;
    Ok(hex::encode_upper(cert.raw_serial()))
}

/// Ім'я підписанта з сертифіката (CN або Given+Surname) — 1:1 Python.
pub fn cert_signer_name(cert_der: &[u8]) -> Result<String, KeyStoreError> {
    use x509_parser::oid_registry::{OID_X509_COMMON_NAME, OID_X509_GIVEN_NAME, OID_X509_SURNAME};
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|e| KeyStoreError::X509(e.to_string()))?;
    for attr in cert.subject().iter_attributes() {
        if attr.attr_type() == &OID_X509_COMMON_NAME {
            return Ok(attr.as_str().unwrap_or("").to_string());
        }
    }
    let given = cert
        .subject()
        .iter_attributes()
        .find(|a| a.attr_type() == &OID_X509_GIVEN_NAME)
        .and_then(|a| a.as_str().ok())
        .unwrap_or("");
    let surname = cert
        .subject()
        .iter_attributes()
        .find(|a| a.attr_type() == &OID_X509_SURNAME)
        .and_then(|a| a.as_str().ok())
        .unwrap_or("");
    let full = format!("{surname} {given}");
    Ok(full.trim().to_string())
}

/// Знаходить сертифікат підписанта в ланцюгу (не ЦСК) — 1:1 Python `_find_signer_cert`.
pub fn find_signer_cert(certs: &[Vec<u8>]) -> Option<Vec<u8>> {
    for der in certs {
        let (_, cert) = x509_parser::parse_x509_certificate(der).ok()?;
        if cert.subject() == cert.issuer() {
            continue; // кореневий ЦСК
        }
        let is_ca = cert
            .basic_constraints()
            .ok()
            .flatten()
            .map(|bc| bc.value.ca)
            .unwrap_or(false);
        if is_ca {
            continue; // проміжний ЦСК
        }
        return Some(der.clone());
    }
    certs.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_oid_rsa() {
        // 1.2.840.113549.1.1.1
        let oid: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
        assert_eq!(der_oid_to_string(oid), "1.2.840.113549.1.1.1");
    }

    #[test]
    fn der_oid_dstu4145_le() {
        let oid = _der_oid("1.2.804.2.1.1.1.1.3.1.1");
        assert_eq!(der_oid_to_string(&oid), "1.2.804.2.1.1.1.1.3.1.1");
    }

    #[test]
    fn detect_oid_from_pkcs8() {
        let der = _pkcs8_der("1.2.804.2.1.1.1.1.3.1.1");
        assert_eq!(
            detect_pkcs8_algorithm(&der).as_deref(),
            Some("1.2.804.2.1.1.1.1.3.1.1")
        );
    }

    #[test]
    fn is_dstu_detection() {
        assert!(is_dstu4145(Some(OID_DSTU4145_LE)));
        assert!(is_dstu4145(Some(OID_DSTU4145_BE)));
        assert!(!is_dstu4145(Some("1.2.840.113549.1.1.1")));
        assert!(!is_dstu4145(None));
    }

    fn _der_oid(oid_str: &str) -> Vec<u8> {
        let parts: Vec<u64> = oid_str.split('.').map(|p| p.parse().unwrap()).collect();
        let mut body = vec![(parts[0] * 40 + parts[1]) as u8];
        for &p in &parts[2..] {
            let mut chunks = vec![(p & 0x7F) as u8];
            let mut v = p >> 7;
            while v > 0 {
                chunks.push(((v & 0x7F) as u8) | 0x80);
                v >>= 7;
            }
            chunks.reverse();
            body.extend_from_slice(&chunks);
        }
        body
    }

    fn _pkcs8_der(oid_str: &str) -> Vec<u8> {
        let oid_body = _der_oid(oid_str);
        let mut alg = vec![0x30];
        alg.push((oid_body.len() + 2) as u8);
        alg.push(0x06);
        alg.push(oid_body.len() as u8);
        alg.extend_from_slice(&oid_body);
        let inner = [vec![0x02, 0x01, 0x00], alg, vec![0x04, 0x00]].concat();
        let mut out = vec![0x30];
        out.push(inner.len() as u8);
        out.extend_from_slice(&inner);
        out
    }
}
