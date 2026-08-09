//! XAdES-BES enveloped підпис (чистий Rust, ADR-014) — 1:1 Python `signxml`.
//!
//! Відтворює байт-в-байт поведінку `signxml.XMLSigner(method=enveloped,
//! signature_algorithm="rsa-sha256", digest_algorithm="sha256")` з
//! `backend/app/infrastructure/services/prro/crypto_signer.py`:
//!
//! - CanonicalizationMethod: `http://www.w3.org/2006/12/xml-c14n11`
//!   (XML Canonicalization 1.1, INCLUSIVE — всі видимі namespace nodes);
//! - Reference URI="" + Transforms: enveloped-signature → c14n11;
//! - DigestMethod: `http://www.w3.org/2001/04/xmlenc#sha256`;
//! - SignatureMethod: rsa-sha256 (RSA PKCS#1 v1.5) / ecdsa-sha256 (EC);
//! - KeyInfo/X509Data/X509Certificate — PEM base64 без маркерів, 64 симв/рядок;
//! - `<ds:Signature>` вставляється ОСТАННІМ елементом кореня;
//! - XML declaration: `<?xml version='1.0' encoding='UTF-8'?>` (одинарні лапки).
//!
//! DigestValue = Base64(SHA-256(C14N11(документ без Signature))).
//! SignatureValue = Base64(Sign(SHA-256(C14N11(SignedInfo)))).

use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine as _;
use rsa::sha2::{Digest as _, Sha256};

use crate::crypto::{PrroCryptoError, PrroSigner};

const DS_NS: &str = "http://www.w3.org/2000/09/xmldsig#";
const C14N11: &str = "http://www.w3.org/2006/12/xml-c14n11";
const ENVELOPED: &str = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";
const SHA256: &str = "http://www.w3.org/2001/04/xmlenc#sha256";
const RSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256";
const ECDSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256";

/// Приватний ключ XAdES (RSA PKCS#1 v1.5 або ECDSA-SHA256).
#[derive(Clone)]
pub enum XadesKey {
    Rsa(rsa::RsaPrivateKey),
    EcP256(p256::ecdsa::SigningKey),
    EcP384(p384::ecdsa::SigningKey),
}

impl XadesKey {
    /// З PKCS#8 DER (як видає key_store). 1:1 Python `load_der_private_key`.
    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self, PrroCryptoError> {
        use rsa::pkcs8::DecodePrivateKey as _;
        if let Ok(key) = rsa::RsaPrivateKey::from_pkcs8_der(der) {
            return Ok(Self::Rsa(key));
        }
        use p256::pkcs8::DecodePrivateKey as _;
        if let Ok(key) = p256::ecdsa::SigningKey::from_pkcs8_der(der) {
            return Ok(Self::EcP256(key));
        }
        if let Ok(key) = p384::ecdsa::SigningKey::from_pkcs8_der(der) {
            return Ok(Self::EcP384(key));
        }
        Err(PrroCryptoError::Generic(
            "PKCS#8 ключ не є RSA/ECDSA (P-256/P-384)".into(),
        ))
    }

    fn signature_method(&self) -> &'static str {
        match self {
            Self::Rsa(_) => RSA_SHA256,
            Self::EcP256(_) | Self::EcP384(_) => ECDSA_SHA256,
        }
    }

    fn sign(&self, data: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        match self {
            Self::Rsa(key) => {
                use rsa::signature::{SignatureEncoding as _, Signer as _};
                let sk = rsa::pkcs1v15::SigningKey::<Sha256>::new(key.clone());
                let sig = sk.sign(data);
                Ok(sig.to_bytes().to_vec())
            }
            Self::EcP256(key) => {
                use p256::ecdsa::signature::Signer as _;
                let sig: p256::ecdsa::Signature = key.sign(data);
                Ok(sig.to_der().as_bytes().to_vec())
            }
            Self::EcP384(key) => {
                use p384::ecdsa::signature::Signer as _;
                let sig: p384::ecdsa::Signature = key.sign(data);
                Ok(sig.to_der().as_bytes().to_vec())
            }
        }
    }
}

/// XAdES-BES підписант — 1:1 Python `PrroCryptoSigner` (бекенд signxml).
pub struct XadesSigner {
    key: XadesKey,
    certificate_der: Vec<u8>,
}

impl XadesSigner {
    /// Створює підписанта з PKCS#8 приватного ключа + сертифіката (DER).
    pub fn new(key: XadesKey, certificate_der: Vec<u8>) -> Self {
        Self {
            key,
            certificate_der,
        }
    }

    /// З PKCS#8 DER ключа + DER сертифіката.
    pub fn from_pkcs8_der(
        key_der: &[u8],
        certificate_der: Vec<u8>,
    ) -> Result<Self, PrroCryptoError> {
        Ok(Self::new(
            XadesKey::from_pkcs8_der(key_der)?,
            certificate_der,
        ))
    }

    /// Підписаний XML (байт-в-байт як signxml, див. golden-тести).
    pub fn sign_xml(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        let input = std::str::from_utf8(xml_bytes)
            .map_err(|e| PrroCryptoError::Generic(format!("XML не UTF-8: {e}")))?;
        let mut root = parse_xml(input)?;

        // enveloped-signature: якщо вже є ds:Signature — видалити (перепідпис)
        root.children.retain(|c| match c {
            XChild::Elem(e) => !(e.prefix == "ds" && e.local == "Signature"),
            _ => true,
        });

        // 1) DigestValue = SHA-256(C14N11(документ без Signature))
        let mut doc_c14n = String::new();
        c14n_serialize(
            &root,
            &BTreeMap::new(),
            &std::collections::HashSet::new(),
            &mut doc_c14n,
        );
        let digest =
            base64::engine::general_purpose::STANDARD.encode(Sha256::digest(doc_c14n.as_bytes()));

        // 2) SignedInfo (шаблон signxml) → C14N11 → SHA-256 → Sign
        let signed_info = format!(
            "<ds:SignedInfo><ds:CanonicalizationMethod Algorithm=\"{C14N11}\"/><ds:SignatureMethod Algorithm=\"{}\"/><ds:Reference URI=\"\"><ds:Transforms><ds:Transform Algorithm=\"{ENVELOPED}\"/><ds:Transform Algorithm=\"{C14N11}\"/></ds:Transforms><ds:DigestMethod Algorithm=\"{SHA256}\"/><ds:DigestValue>{digest}</ds:DigestValue></ds:Reference></ds:SignedInfo>",
            self.key.signature_method()
        );
        let si_node = parse_xml(&signed_info)?;
        let mut si_ns = BTreeMap::new();
        si_ns.insert("ds".to_string(), DS_NS.to_string());
        let mut si_c14n = String::new();
        c14n_serialize(
            &si_node,
            &si_ns,
            &std::collections::HashSet::new(),
            &mut si_c14n,
        );
        // signxml передає C14N-байти в RSA sign (cryptography хешує всередині)
        let sig_value = self.key.sign(si_c14n.as_bytes())?;
        let sig_value_b64 = base64::engine::general_purpose::STANDARD.encode(&sig_value);

        // 3) KeyInfo: X509Certificate — PEM base64 без маркерів, 64 симв/рядок
        let cert_b64 = base64::engine::general_purpose::STANDARD.encode(&self.certificate_der);
        let cert_pem = wrap64(&cert_b64);

        // 4) Збірка ds:Signature (порожні елементи — самозакриття, як lxml)
        let signature = format!(
            "<ds:Signature xmlns:ds=\"{DS_NS}\"><ds:SignedInfo><ds:CanonicalizationMethod Algorithm=\"{C14N11}\"/><ds:SignatureMethod Algorithm=\"{}\"/><ds:Reference URI=\"\"><ds:Transforms><ds:Transform Algorithm=\"{ENVELOPED}\"/><ds:Transform Algorithm=\"{C14N11}\"/></ds:Transforms><ds:DigestMethod Algorithm=\"{SHA256}\"/><ds:DigestValue>{digest}</ds:DigestValue></ds:Reference></ds:SignedInfo><ds:SignatureValue>{sig_value_b64}</ds:SignatureValue><ds:KeyInfo><ds:X509Data><ds:X509Certificate>{cert_pem}</ds:X509Certificate></ds:X509Data></ds:KeyInfo></ds:Signature>",
            self.key.signature_method()
        );
        let sig_node = parse_xml(&signature)?;
        root.children.push(XChild::Elem(sig_node));

        // 5) Серіалізація: XML declaration + документ (порядок атрибутів — як у
        //    вхідному; порожні елементи — самозакриття; 1:1 lxml tostring).
        let mut out = String::from("<?xml version='1.0' encoding='UTF-8'?>\n");
        serialize(&root, &mut out);
        Ok(out.into_bytes())
    }

    /// Перевірка XAdES-підпису (1:1 Python `XMLVerifier`): digest документа +
    /// криптографічна валідність SignatureValue публічним ключем з KeyInfo.
    pub fn verify_xml(&self, signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
        let input = std::str::from_utf8(signed_xml)
            .map_err(|e| PrroCryptoError::Generic(format!("XML не UTF-8: {e}")))?;
        let root = parse_xml(input)?;

        // знайти ds:Signature
        let sig = root
            .children
            .iter()
            .find_map(|c| match c {
                XChild::Elem(e) if e.prefix == "ds" && e.local == "Signature" => Some(e),
                _ => None,
            })
            .ok_or_else(|| PrroCryptoError::Generic("ds:Signature не знайдено".into()))?;

        // SignedInfo + SignatureValue + X509Certificate
        let signed_info = sig
            .children
            .iter()
            .find_map(|c| match c {
                XChild::Elem(e) if e.local == "SignedInfo" => Some(e),
                _ => None,
            })
            .ok_or_else(|| PrroCryptoError::Generic("SignedInfo не знайдено".into()))?;
        let sig_value = sig
            .children
            .iter()
            .find_map(|c| match c {
                XChild::Elem(e) if e.local == "SignatureValue" => Some(e),
                _ => None,
            })
            .and_then(|e| e.text())
            .ok_or_else(|| PrroCryptoError::Generic("SignatureValue не знайдено".into()))?;
        let cert_b64 = sig
            .children
            .iter()
            .find_map(|c| match c {
                XChild::Elem(e) if e.local == "KeyInfo" => Some(e),
                _ => None,
            })
            .and_then(|ki| {
                ki.children.iter().find_map(|c| match c {
                    XChild::Elem(e) if e.local == "X509Data" => Some(e),
                    _ => None,
                })
            })
            .and_then(|xd| {
                xd.children.iter().find_map(|c| match c {
                    XChild::Elem(e) if e.local == "X509Certificate" => Some(e),
                    _ => None,
                })
            })
            .and_then(|e| e.text())
            .ok_or_else(|| PrroCryptoError::Generic("X509Certificate не знайдено".into()))?;

        // digest: документ без Signature → C14N → SHA-256 == DigestValue
        let mut stripped = root.clone();
        stripped.children.retain(
            |c| !matches!(c, XChild::Elem(e) if e.prefix == "ds" && e.local == "Signature"),
        );
        let mut doc_c14n = String::new();
        c14n_serialize(
            &stripped,
            &BTreeMap::new(),
            &std::collections::HashSet::new(),
            &mut doc_c14n,
        );
        let digest_actual =
            base64::engine::general_purpose::STANDARD.encode(Sha256::digest(doc_c14n.as_bytes()));

        let digest_expected = signed_info
            .children
            .iter()
            .find_map(|c| match c {
                XChild::Elem(e) if e.local == "Reference" => Some(e),
                _ => None,
            })
            .and_then(|r| {
                r.children.iter().find_map(|c| match c {
                    XChild::Elem(e) if e.local == "DigestValue" => Some(e),
                    _ => None,
                })
            })
            .and_then(|e| e.text())
            .ok_or_else(|| PrroCryptoError::Generic("DigestValue не знайдено".into()))?;
        if digest_actual != digest_expected {
            return Ok(false);
        }

        // signature: C14N(SignedInfo) → SHA-256 → verify публічним ключем з cert
        let mut si_ns = BTreeMap::new();
        si_ns.insert("ds".to_string(), DS_NS.to_string());
        let mut si_c14n = String::new();
        c14n_serialize(
            signed_info,
            &si_ns,
            &std::collections::HashSet::new(),
            &mut si_c14n,
        );
        let sig_clean: String = sig_value.chars().filter(|c| !c.is_whitespace()).collect();
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(sig_clean)
            .map_err(|e| PrroCryptoError::Generic(format!("SignatureValue base64: {e}")))?;
        let cert_b64_clean: String = cert_b64.chars().filter(|c| !c.is_whitespace()).collect();
        let cert_der = base64::engine::general_purpose::STANDARD
            .decode(cert_b64_clean)
            .map_err(|e| PrroCryptoError::Generic(format!("X509Certificate base64: {e}")))?;

        let ok = verify_with_cert(&cert_der, si_c14n.as_bytes(), &sig_bytes)?;
        Ok(ok)
    }
}

/// Перевірка SignatureValue публічним ключем з X.509-сертифіката (DER).
/// Підтримуються RSA (PKCS#1 v1.5) та ECDSA (P-256/P-384) — 1:1 XMLVerifier.
fn verify_with_cert(cert_der: &[u8], data: &[u8], sig: &[u8]) -> Result<bool, PrroCryptoError> {
    use p256::pkcs8::DecodePublicKey as _;
    use rsa::pkcs8::DecodePublicKey as _;
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|e| PrroCryptoError::Generic(format!("X.509: {e}")))?;
    let spki = cert.public_key().raw;

    if let Ok(pubkey) = rsa::RsaPublicKey::from_public_key_der(spki) {
        use rsa::signature::Verifier as _;
        let vk = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(pubkey);
        let sig = rsa::pkcs1v15::Signature::try_from(sig)
            .map_err(|e| PrroCryptoError::Generic(format!("RSA sig: {e}")))?;
        return Ok(vk.verify(data, &sig).is_ok());
    }
    if let Ok(pubkey) = p256::ecdsa::VerifyingKey::from_public_key_der(spki) {
        use p256::ecdsa::signature::Verifier as _;
        let sig = p256::ecdsa::Signature::from_der(sig)
            .map_err(|e| PrroCryptoError::Generic(format!("ECDSA sig: {e}")))?;
        return Ok(pubkey.verify(data, &sig).is_ok());
    }
    if let Ok(pubkey) = p384::ecdsa::VerifyingKey::from_public_key_der(spki) {
        use p384::ecdsa::signature::Verifier as _;
        let sig = p384::ecdsa::Signature::from_der(sig)
            .map_err(|e| PrroCryptoError::Generic(format!("ECDSA sig: {e}")))?;
        return Ok(pubkey.verify(data, &sig).is_ok());
    }
    Err(PrroCryptoError::Generic(
        "Непідтримуваний алгоритм публічного ключа".into(),
    ))
}

impl PrroSigner for XadesSigner {
    fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        self.sign_xml(xml_bytes)
    }

    fn verify(&self, signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
        self.verify_xml(signed_xml)
    }

    fn get_serial_number(&self) -> Result<String, PrroCryptoError> {
        serial_from_cert(&self.certificate_der)
    }

    fn get_signer_name(&self) -> Result<String, PrroCryptoError> {
        name_from_cert(&self.certificate_der)
    }
}

/// Серійний номер сертифіката (hex, upper) — 1:1 Python `format(serial, "X")`.
pub fn serial_from_cert(cert_der: &[u8]) -> Result<String, PrroCryptoError> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|e| PrroCryptoError::Generic(format!("X.509: {e}")))?;
    // raw_serial_as_string — hex з двокрапками ("6F:C5:.."); Python
    // format(serial, "X") — без розділювачів, upper.
    let hex = cert.raw_serial_as_string().replace(':', "").to_uppercase();
    Ok(hex)
}

/// ПІБ підписанта: CN → інакше GivenName + Surname (1:1 Python).
pub fn name_from_cert(cert_der: &[u8]) -> Result<String, PrroCryptoError> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|e| PrroCryptoError::Generic(format!("X.509: {e}")))?;
    if let Some(cn) = cert.subject().iter_common_name().next() {
        let cn = cn.as_str().unwrap_or("");
        if !cn.is_empty() {
            return Ok(cn.to_string());
        }
    }
    let given = cert
        .subject()
        .iter_by_oid(&x509_parser::oid_registry::OID_X509_GIVEN_NAME)
        .next();
    let surname = cert
        .subject()
        .iter_by_oid(&x509_parser::oid_registry::OID_X509_SURNAME)
        .next();
    let mut parts = Vec::new();
    if let Some(s) = surname {
        parts.push(s.as_str().unwrap_or("").to_string());
    }
    if let Some(g) = given {
        parts.push(g.as_str().unwrap_or("").to_string());
    }
    Ok(parts.join(" "))
}

// ═══════════════════════════════════════════════════════════════════════════
// Мінімальний XML DOM (префікси, xmlns, змішаний контент) + C14N 1.1 inclusive
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct XAttr {
    prefix: String,
    local: String,
    value: String,
}

#[derive(Debug, Clone)]
enum XChild {
    Text(String),
    Elem(XNode),
}

#[derive(Debug, Clone)]
struct XNode {
    prefix: String,
    local: String,
    attrs: Vec<XAttr>,
    children: Vec<XChild>,
}

impl XNode {
    fn text(&self) -> Option<String> {
        let mut s = String::new();
        for c in &self.children {
            if let XChild::Text(t) = c {
                s.push_str(t);
            }
        }
        (!s.is_empty()).then_some(s)
    }
}

fn parse_xml(input: &str) -> Result<XNode, PrroCryptoError> {
    let mut pos = 0usize;
    // XML declaration
    if input[pos..].starts_with("<?xml") {
        if let Some(end) = input[pos..].find("?>") {
            pos += end + 2;
        }
    }
    // пропустити пробільні символи до кореня
    while pos < input.len() && input.as_bytes()[pos].is_ascii_whitespace() {
        pos += 1;
    }
    parse_element(input, &mut pos)
}

fn parse_element(input: &str, pos: &mut usize) -> Result<XNode, PrroCryptoError> {
    skip_ws(input, pos);
    expect(input, pos, '<')?;
    let (prefix, local) = read_name(input, pos)?;
    let mut attrs = Vec::new();
    loop {
        skip_ws(input, pos);
        if *pos >= input.len() {
            return Err(err("несподіваний кінець тега"));
        }
        let c = input.as_bytes()[*pos];
        if c == b'>' {
            *pos += 1;
            break;
        }
        if c == b'/' && input.as_bytes().get(*pos + 1) == Some(&b'>') {
            *pos += 2;
            return Ok(XNode {
                prefix,
                local,
                attrs,
                children: Vec::new(),
            });
        }
        let (ap, al) = read_name(input, pos)?;
        skip_ws(input, pos);
        expect(input, pos, '=')?;
        skip_ws(input, pos);
        let quote = input
            .as_bytes()
            .get(*pos)
            .copied()
            .ok_or_else(|| err("очікувались лапки"))?;
        if quote != b'"' && quote != b'\'' {
            return Err(err("очікувались лапки атрибута"));
        }
        *pos += 1;
        let start = *pos;
        while *pos < input.len() && input.as_bytes()[*pos] != quote {
            *pos += 1;
        }
        if *pos >= input.len() {
            return Err(err("незакриті лапки атрибута"));
        }
        let raw = &input[start..*pos];
        *pos += 1;
        attrs.push(XAttr {
            prefix: ap,
            local: al,
            value: unescape(raw),
        });
    }

    let mut children = Vec::new();
    loop {
        skip_ws_keep(input, pos);
        if *pos >= input.len() {
            return Err(err("незакритий елемент"));
        }
        if input.as_bytes()[*pos] == b'<' {
            // закриття?
            if input.as_bytes().get(*pos + 1) == Some(&b'/') {
                *pos += 2;
                let (cp, cl) = read_name(input, pos)?;
                if cp != prefix || cl != local {
                    return Err(err("невідповідність закриваючого тега"));
                }
                skip_ws(input, pos);
                expect(input, pos, '>')?;
                break;
            }
            // коментар / CDATA / інше
            if input[*pos..].starts_with("<!--") {
                if let Some(end) = input[*pos..].find("-->") {
                    *pos += end + 3;
                    continue;
                }
                return Err(err("незакритий коментар"));
            }
            children.push(XChild::Elem(parse_element(input, pos)?));
        } else {
            let start = *pos;
            while *pos < input.len() && input.as_bytes()[*pos] != b'<' {
                *pos += 1;
            }
            let raw = &input[start..*pos];
            if !raw.trim().is_empty() {
                children.push(XChild::Text(unescape(raw)));
            }
        }
    }
    Ok(XNode {
        prefix,
        local,
        attrs,
        children,
    })
}

/// Пропускає пробіли (НЕ всередині тексту — окрема функція для контенту).
fn skip_ws(input: &str, pos: &mut usize) {
    while *pos < input.len() && input.as_bytes()[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

/// Для контенту: пробіли зберігаються (це не викликається — текст читається цілком).
fn skip_ws_keep(_input: &str, _pos: &mut usize) {}

fn expect(input: &str, pos: &mut usize, ch: char) -> Result<(), PrroCryptoError> {
    if input.as_bytes().get(*pos) == Some(&(ch as u8)) {
        *pos += 1;
        Ok(())
    } else {
        Err(err(&format!("очікувався '{ch}'")))
    }
}

fn read_name(input: &str, pos: &mut usize) -> Result<(String, String), PrroCryptoError> {
    let start = *pos;
    while *pos < input.len() {
        let c = input.as_bytes()[*pos];
        if c.is_ascii_alphanumeric() || c == b':' || c == b'-' || c == b'_' || c == b'.' {
            *pos += 1;
        } else {
            break;
        }
    }
    if *pos == start {
        return Err(err("порожнє ім'я"));
    }
    let name = &input[start..*pos];
    match name.split_once(':') {
        Some((p, l)) => Ok((p.to_string(), l.to_string())),
        None => Ok((String::new(), name.to_string())),
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\r', "&#xD;")
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Серіалізація у "вихідному" вигляді (1:1 lxml tostring): атрибути в порядку
/// вхідного XML, порожні елементи — самозакриття `<a/>`.
fn serialize(node: &XNode, out: &mut String) {
    out.push('<');
    if !node.prefix.is_empty() {
        out.push_str(&node.prefix);
        out.push(':');
    }
    out.push_str(&node.local);
    for a in &node.attrs {
        out.push(' ');
        if !a.prefix.is_empty() {
            out.push_str(&a.prefix);
            out.push(':');
        }
        out.push_str(&a.local);
        out.push_str("=\"");
        out.push_str(&escape_attr(&a.value));
        out.push('"');
    }
    let has_content = !node.children.is_empty();
    if !has_content {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &node.children {
        match c {
            XChild::Text(t) => out.push_str(&escape_text(t)),
            XChild::Elem(e) => serialize(e, out),
        }
    }
    out.push_str("</");
    if !node.prefix.is_empty() {
        out.push_str(&node.prefix);
        out.push(':');
    }
    out.push_str(&node.local);
    out.push('>');
}

/// C14N 1.1 inclusive (1:1 libxml2/lxml): namespace nodes — всі видимі,
/// але вже виведений на предку namespace не повторюється (специфікація §2.3);
/// атрибути сортуються за (namespace_uri, local); порожні елементи — пари
/// тегів `<a></a>`.
fn c14n_serialize(
    node: &XNode,
    ns: &BTreeMap<String, String>,
    rendered: &std::collections::HashSet<String>,
    out: &mut String,
) {
    // namespace nodes: inclusive — батьківські + власні xmlns-декларації
    let mut ns2 = ns.clone();
    for a in &node.attrs {
        if a.prefix == "xmlns" {
            ns2.insert(a.local.clone(), a.value.clone());
        } else if a.prefix.is_empty() && a.local == "xmlns" {
            ns2.insert(String::new(), a.value.clone());
        }
    }

    out.push('<');
    if !node.prefix.is_empty() {
        out.push_str(&node.prefix);
        out.push(':');
    }
    out.push_str(&node.local);

    // namespace nodes: default першим, потім за префіксом; виводимо тільки
    // ті, що ще не виведені на предку (rendered передається від батька).
    let mut rendered = rendered.clone();
    if let Some(uri) = ns2.get("") {
        if !rendered.contains("") {
            out.push_str(" xmlns=\"");
            out.push_str(&escape_attr(uri));
            out.push('"');
            rendered.insert(String::new());
        }
    }
    for (prefix, uri) in &ns2 {
        if prefix.is_empty() {
            continue;
        }
        if !rendered.contains(prefix) {
            out.push_str(" xmlns:");
            out.push_str(prefix);
            out.push_str("=\"");
            out.push_str(&escape_attr(uri));
            out.push('"');
            rendered.insert(prefix.clone());
        }
    }

    // звичайні атрибути за (namespace_uri, local)
    let mut attrs: Vec<(&XAttr, String)> = node
        .attrs
        .iter()
        .filter(|a| !(a.prefix == "xmlns" || (a.prefix.is_empty() && a.local == "xmlns")))
        .map(|a| {
            let uri = if a.prefix.is_empty() {
                String::new()
            } else {
                ns2.get(&a.prefix).cloned().unwrap_or_default()
            };
            (a, uri)
        })
        .collect();
    attrs.sort_by(|x, y| x.1.cmp(&y.1).then_with(|| x.0.local.cmp(&y.0.local)));
    for (a, _uri) in &attrs {
        out.push(' ');
        if !a.prefix.is_empty() {
            out.push_str(&a.prefix);
            out.push(':');
        }
        out.push_str(&a.local);
        out.push_str("=\"");
        out.push_str(&escape_attr(&a.value));
        out.push('"');
    }
    out.push('>');

    for c in &node.children {
        match c {
            XChild::Text(t) => out.push_str(&escape_text(t)),
            XChild::Elem(e) => c14n_serialize(e, &ns2, &rendered, out),
        }
    }

    out.push_str("</");
    if !node.prefix.is_empty() {
        out.push_str(&node.prefix);
        out.push(':');
    }
    out.push_str(&node.local);
    out.push('>');
}

fn wrap64(s: &str) -> String {
    let mut out = String::new();
    for (i, chunk) in s.as_bytes().chunks(64).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
    }
    out.push('\n');
    out
}

fn err(msg: &str) -> PrroCryptoError {
    PrroCryptoError::Generic(format!("XAdES XML: {msg}"))
}

/// Шлях до тестового RSA-ключа (golden XAdES) — tests/fixtures/test-rsa.pem
/// (клон certs/prro-test/test-rsa.pem; certs/ у .gitignore).
pub fn test_rsa_key_path() -> Option<std::path::PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pem = crate_dir.join("tests/fixtures/test-rsa.pem");
    pem.is_file().then_some(pem)
}
