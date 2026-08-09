//! GOLDEN PARITY XAdES: Rust == Python signxml, байт-в-байт (критерій 7.2 #1).
//!
//! Вектори згенеровані Python-еталоном
//! (`backend/app/infrastructure/services/prro/crypto_signer.py` +
//! `scripts/gen_golden_vectors.py`) з ключем `certs/prro-test/test-rsa.pem`:
//! підписаний XML цілком + DigestValue + SignatureValue для незалежної
//! перевірки. Rust-підпис МАЄ бути ідентичним байт-в-байт (RSA PKCS#1 v1.5
//! детермінований, C14N детермінований).

use torgashka_prro::crypto::{PrroSigner, XadesKey, XadesSigner};
use torgashka_prro::keystore;

fn load_signer() -> XadesSigner {
    let Some(pem_path) = torgashka_prro::crypto::xades::test_rsa_key_path() else {
        panic!("certs/prro-test/test-rsa.pem не знайдено");
    };
    let material =
        keystore::load_key_material(&pem_path, "", None).expect("RSA PEM ключ читається");
    let cert = material.certs.first().expect("сертифікат");
    let key_der = material.private_key_der.as_deref().expect("ключ");
    XadesSigner::from_pkcs8_der(key_der, cert.clone()).expect("XadesSigner")
}

fn golden() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/xades_golden.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("golden file")).expect("golden json")
}

#[test]
fn xades_sign_matches_python_byte_for_byte() {
    let signer = load_signer();
    let golden = golden();
    let mut checked = 0;
    for (name, case) in golden.as_object().unwrap() {
        let xml = case["signed_xml"].as_str().unwrap();
        // відновлюємо ВХІДНИЙ XML: видаляємо ds:Signature (enveloped)
        let input = strip_signature(xml);
        let signed = signer.sign(input.as_bytes()).expect("sign");
        let signed_str = String::from_utf8(signed).expect("utf8");
        assert_eq!(
            signed_str, xml,
            "XAdES підпис {name} має бути байт-ідентичним Python signxml"
        );
        checked += 1;
    }
    assert!(checked >= 5, "має бути >= 5 golden-векторів, є {checked}");
}

#[test]
fn xades_digest_and_signature_values_match_golden() {
    let signer = load_signer();
    let golden = golden();
    for (name, case) in golden.as_object().unwrap() {
        let signed = case["signed_xml"].as_str().unwrap().as_bytes();
        let verified = signer.verify(signed).expect("verify");
        assert!(verified, "verify Python-підписаного XML ({name})");
        // SignatureValue == golden (детермінований RSA)
        let sv = extract_sig_value(signed);
        assert_eq!(
            sv,
            case["signature_value"].as_str().unwrap(),
            "SignatureValue {name}"
        );
    }
}

#[test]
fn xades_verify_own_and_python_signed() {
    let signer = load_signer();
    let golden = golden();
    let input = strip_signature(golden["v1_receipt_sale"]["signed_xml"].as_str().unwrap());
    let own = signer.sign(input.as_bytes()).expect("sign");
    assert!(signer.verify(&own).expect("verify own"), "власний підпис");
    // Python-підписаний XML теж валідний
    let py = golden["v5_zreport"]["signed_xml"]
        .as_str()
        .unwrap()
        .as_bytes();
    assert!(signer.verify(py).expect("verify python"), "Python-підпис");
}

#[test]
fn xades_verify_rejects_tampered() {
    let signer = load_signer();
    let golden = golden();
    let signed = golden["v1_receipt_sale"]["signed_xml"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    // змінити ціну в чеку (перший SM=)
    let mut tampered = signed.clone();
    let needle = b"SM=\"137\"";
    let idx = tampered
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("SM=137 знайдено");
    tampered[idx + 4] = b'9'; // 137 → 197
    assert!(
        !signer.verify(&tampered).expect("verify tampered"),
        "змінений чек не має проходити verify"
    );
}

#[test]
fn xades_signer_identity_matches_python() {
    let signer = load_signer();
    // Python: serial=6FC54869E8DC5E44BAC89F53525143676805901B,
    //         name="Тестовий Підписант Torgashka" (з certs/prro-test/test-rsa.pem)
    assert_eq!(
        signer.get_serial_number().expect("serial"),
        "7AED62741A8E1483BEB1038028B42B03BF58244E"
    );
    let name = signer.get_signer_name().expect("name");
    assert!(
        name.contains("Тестовий") && name.contains("Підписант"),
        "name={name:?}"
    );
}

#[test]
fn xades_rsa_key_from_pkcs8_der() {
    let Some(pem_path) = torgashka_prro::crypto::xades::test_rsa_key_path() else {
        panic!("test-rsa.pem відсутній");
    };
    let material = keystore::load_key_material(&pem_path, "", None).unwrap();
    let key = XadesKey::from_pkcs8_der(material.private_key_der.as_deref().unwrap()).unwrap();
    assert!(matches!(key, XadesKey::Rsa(_)));
}

/// Видаляє ds:Signature з підписаного XML — відновлює вхідний документ.
fn strip_signature(signed: &str) -> String {
    let sig_start = signed
        .rfind("<ds:Signature xmlns:ds=")
        .expect("ds:Signature знайдено");
    let sig_end =
        signed.rfind("</ds:Signature>").expect("кінець Signature") + "</ds:Signature>".len();
    let mut out = String::from(&signed[..sig_start]);
    out.push_str(&signed[sig_end..]);
    out
}

fn extract_sig_value(signed: &[u8]) -> String {
    let s = String::from_utf8(signed.to_vec()).unwrap();
    let start = s.find("<ds:SignatureValue>").unwrap() + "<ds:SignatureValue>".len();
    let end = s.find("</ds:SignatureValue>").unwrap();
    s[start..end].to_string()
}
