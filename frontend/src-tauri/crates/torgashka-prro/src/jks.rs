//! Парсер Java KeyStore (JKS) + дешифрування приватного ключа.
//!
//! Байт-ідентичний pyjks (`jks_pkey_decrypt` + `_jks_keystream`):
//! - пароль → UTF-16BE (Java char units);
//! - `iv` (20 байт) + зашифровані дані + `check` (20 байт SHA-1);
//! - keystream: `cur = SHA1(password + cur)`, стартуючи з `cur = iv`;
//! - `key = data XOR keystream`;
//! - перевірка: `SHA1(password + key) == check`.
//!
//! Це JavaSoft proprietary key-protection (OID 1.3.6.1.4.1.42.2.17.1.1),
//! який використовує JKS-сховища (magic 0xFEEDFEED). JCEKS (0xCECECECE)
//! використовує інший алгоритм (PBEWithMD5AndTripleDES) — не підтримується
//! (чітка помилка, як Python).

use sha1::{Digest, Sha1};
use std::path::Path;

pub const JKS_MAGIC: [u8; 4] = [0xFE, 0xED, 0xFE, 0xED];
pub const JCEKS_MAGIC: [u8; 4] = [0xCE, 0xCE, 0xCE, 0xCE];

#[derive(Debug, thiserror::Error)]
pub enum JksError {
    #[error("Не вдалося прочитати файл JKS: {0}")]
    Io(#[from] std::io::Error),
    #[error("Некоректний JKS: {0}")]
    Invalid(String),
    #[error("JCEKS-сховища не підтримуються (використовуйте JKS або PKCS#12)")]
    JceksUnsupported,
    #[error("У JKS відсутні приватні ключі")]
    NoPrivateKeys,
    #[error("Не вдалося розшифрувати ключ JKS: {0}")]
    Decrypt(String),
    #[error("Некоректний пароль або пошкоджений ключ")]
    BadPassword,
}

/// Запис приватного ключа з JKS.
#[derive(Debug, Clone)]
pub struct JksPrivateKey {
    pub alias: String,
    /// Розшифрований PKCS#8 (DER) приватний ключ.
    pub key_der: Vec<u8>,
    /// Ланцюг сертифікатів (DER X.509), у порядку зі сховища.
    pub cert_chain: Vec<Vec<u8>>,
}

/// Розбирає JKS-файл і повертає перший приватний ключ (1:1 Python pyjks).
pub fn load_jks_private_key(path: &Path, password: &str) -> Result<JksPrivateKey, JksError> {
    let data = std::fs::read(path)?;
    let mut pos = 0usize;

    let magic = read_bytes(&data, &mut pos, 4)?;
    if magic == JCEKS_MAGIC {
        return Err(JksError::JceksUnsupported);
    }
    if magic != JKS_MAGIC {
        return Err(JksError::Invalid(
            "магічне число не схоже на JKS (0xFEEDFEED)".into(),
        ));
    }
    let _version = read_u32(&data, &mut pos)?;
    let count = read_u32(&data, &mut pos)? as usize;

    let mut private_keys: Vec<JksPrivateKey> = Vec::new();

    for _ in 0..count {
        let tag = read_u32(&data, &mut pos)?;
        let alias = read_utf(&data, &mut pos)?;
        let _timestamp = read_u64(&data, &mut pos)?;

        if tag == 1 {
            // private key entry
            let key_len = read_u32(&data, &mut pos)? as usize;
            let encrypted = read_bytes(&data, &mut pos, key_len)?;
            let chain_len = read_u32(&data, &mut pos)? as usize;
            let mut cert_chain = Vec::with_capacity(chain_len);
            for _ in 0..chain_len {
                let _cert_type = read_utf(&data, &mut pos)?; // "X.509"
                let cert_len = read_u32(&data, &mut pos)? as usize;
                let cert = read_bytes(&data, &mut pos, cert_len)?;
                cert_chain.push(cert);
            }
            // JKS зберігає EncryptedPrivateKeyInfo (DER): SEQUENCE {
            //   AlgorithmIdentifier { OID JavaSoft, params }, OCTET STRING }
            // Дешифрується OCTET STRING (iv + XOR-дані + check) — 1:1 pyjks.
            let payload = extract_octet_string(&encrypted)?;
            let key_der = jks_pkey_decrypt(&payload, password)?;
            private_keys.push(JksPrivateKey {
                alias,
                key_der,
                cert_chain,
            });
        } else if tag == 2 {
            // trusted cert entry
            let _cert_type = read_utf(&data, &mut pos)?;
            let cert_len = read_u32(&data, &mut pos)? as usize;
            let _cert = read_bytes(&data, &mut pos, cert_len)?;
        } else {
            return Err(JksError::Invalid(format!("невідомий тег запису: {tag}")));
        }
    }

    private_keys
        .into_iter()
        .next()
        .ok_or(JksError::NoPrivateKeys)
}

/// Витягує OCTET STRING з EncryptedPrivateKeyInfo (DER) — 1:1 pyjks
/// `decoder.decode(..., asn1Spec=rfc5208.EncryptedPrivateKeyInfo())[0]["encryptedData"]`.
fn extract_octet_string(der: &[u8]) -> Result<Vec<u8>, JksError> {
    if der.first() != Some(&0x30) {
        // не DER — сирий JavaSoft-блок (для сумісності)
        return Ok(der.to_vec());
    }
    let mut pos = 1usize;
    let (_, next) = read_der_len(der, pos)?;
    pos = next;
    // AlgorithmIdentifier: SEQUENCE { OBJECT, params }
    if der.get(pos) != Some(&0x30) {
        return Err(JksError::Invalid(
            "EncryptedPrivateKeyInfo без AlgorithmIdentifier".into(),
        ));
    }
    pos += 1;
    let (alg_len, next) = read_der_len(der, pos)?;
    pos = next;
    let alg_end = pos + alg_len;
    if der.get(pos) != Some(&0x06) {
        return Err(JksError::Invalid("AlgorithmIdentifier без OID".into()));
    }
    pos += 1;
    let (oid_len, next) = read_der_len(der, pos)?;
    pos = next + oid_len;
    if pos > alg_end {
        return Err(JksError::Invalid(
            "OID виходить за межі AlgorithmIdentifier".into(),
        ));
    }
    pos = alg_end; // пропустити параметри (NULL або SEQUENCE)
    if der.get(pos) != Some(&0x04) {
        return Err(JksError::Invalid(
            "EncryptedPrivateKeyInfo без OCTET STRING".into(),
        ));
    }
    pos += 1;
    let (oct_len, next) = read_der_len(der, pos)?;
    pos = next;
    if pos + oct_len > der.len() {
        return Err(JksError::Invalid(
            "OCTET STRING виходить за межі даних".into(),
        ));
    }
    Ok(der[pos..pos + oct_len].to_vec())
}

/// Читає DER-довжину (коротка/довга форма) і повертає (довжина, нова позиція).
fn read_der_len(data: &[u8], pos: usize) -> Result<(usize, usize), JksError> {
    let first = *data
        .get(pos)
        .ok_or_else(|| JksError::Invalid("несподіваний кінець DER".into()))?;
    if first & 0x80 == 0 {
        return Ok((first as usize, pos + 1));
    }
    let n = (first & 0x7F) as usize;
    if n > 4 || pos + 1 + n > data.len() {
        return Err(JksError::Invalid(
            "некоректна довга форма довжини DER".into(),
        ));
    }
    let mut len = 0usize;
    for i in 0..n {
        len = (len << 8) | data[pos + 1 + i] as usize;
    }
    Ok((len, pos + 1 + n))
}

/// JavaSoft key-protection decrypt — 1:1 pyjks `jks_pkey_decrypt`.
fn jks_pkey_decrypt(data: &[u8], password: &str) -> Result<Vec<u8>, JksError> {
    let password_bytes = password_to_utf16be(password);

    if data.len() < 40 {
        return Err(JksError::BadPassword);
    }
    let iv = &data[..20];
    let encrypted = &data[20..data.len() - 20];
    let check = &data[data.len() - 20..];

    let mut key = Vec::with_capacity(encrypted.len());
    let mut keystream = JksKeystream::new(iv, &password_bytes);
    for &b in encrypted {
        key.push(b ^ keystream.next_byte());
    }

    // перевірка цілісності
    let mut hasher = Sha1::new();
    hasher.update(&password_bytes);
    hasher.update(&key);
    let digest = hasher.finalize();
    if digest.as_slice() != check {
        return Err(JksError::BadPassword);
    }
    Ok(key)
}

/// Keystream: `cur = SHA1(password + cur)`, стартуючи з `cur = iv` — 1:1 pyjks.
struct JksKeystream {
    cur: Vec<u8>,
    idx: usize,
    password: Vec<u8>,
}

impl JksKeystream {
    fn new(iv: &[u8], password: &[u8]) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(password);
        hasher.update(iv);
        Self {
            cur: hasher.finalize().to_vec(),
            idx: 0,
            password: password.to_vec(),
        }
    }

    fn next_byte(&mut self) -> u8 {
        if self.idx >= self.cur.len() {
            let mut hasher = Sha1::new();
            hasher.update(&self.password);
            hasher.update(&self.cur);
            self.cur = hasher.finalize().to_vec();
            self.idx = 0;
        }
        let b = self.cur[self.idx];
        self.idx += 1;
        b
    }
}

/// Пароль → UTF-16BE байти (Java char units) — 1:1 pyjks.
fn password_to_utf16be(password: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(password.len() * 2);
    for unit in password.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

fn read_bytes(data: &[u8], pos: &mut usize, len: usize) -> Result<Vec<u8>, JksError> {
    if *pos + len > data.len() {
        return Err(JksError::Invalid("несподіваний кінець файлу".into()));
    }
    let out = data[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(out)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32, JksError> {
    let b = read_bytes(data, pos, 4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u64(data: &[u8], pos: &mut usize) -> Result<u64, JksError> {
    let b = read_bytes(data, pos, 8)?;
    Ok(u64::from_be_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

fn read_utf(data: &[u8], pos: &mut usize) -> Result<String, JksError> {
    let len = read_u16(data, pos)? as usize;
    let b = read_bytes(data, pos, len)?;
    String::from_utf8(b).map_err(|_| JksError::Invalid("alias не UTF-8".into()))
}

fn read_u16(data: &[u8], pos: &mut usize) -> Result<u16, JksError> {
    let b = read_bytes(data, pos, 2)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16be_encoding() {
        assert_eq!(password_to_utf16be("test2003"), {
            let mut v = Vec::new();
            for u in "test2003".encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            v
        });
        // "t" = 0x0074
        assert_eq!(password_to_utf16be("t"), vec![0x00, 0x74]);
    }

    #[test]
    fn keystream_matches_pyjks() {
        // Вектор: iv = 20 байт, пароль "x" → перший байт SHA1(utf16be("x") + iv)
        let iv: Vec<u8> = (0u8..20).collect();
        let pw = password_to_utf16be("x");
        let mut ks = JksKeystream::new(&iv, &pw);
        let mut hasher = Sha1::new();
        hasher.update(&pw);
        hasher.update(&iv);
        let expected = hasher.finalize();
        assert_eq!(ks.next_byte(), expected[0]);
        assert_eq!(ks.next_byte(), expected[1]);
    }

    #[test]
    fn magic_constants() {
        assert_eq!(JKS_MAGIC, [0xFE, 0xED, 0xFE, 0xED]);
        assert_eq!(JCEKS_MAGIC, [0xCE, 0xCE, 0xCE, 0xCE]);
    }
}
