//! Читання реального тестового КЕП (JKS, ДСТУ 4145) — критерій прийняття 7.1 #4.
//!
//! Ключ: `certs/prro-test/pb_3791505547 (2).jks`, пароль `test2003`.
//! Фактичні значення звірені з Python (pyjks + ручний DER-парсер, 2026-08-07):
//! - alias: pb_sign_3791505547, ланцюг: 4 сертифікати;
//! - підписант cert2: CN=МОРОЗ АНАСТАСІЯ-РОКСОЛАНА ВАСИЛІВНА,
//!   serial=5E984D526F82F38F040000006E5EE80123BED307;
//! - приватний ключ: PKCS#8 з OID ДСТУ 4145-2002 (1.2.804.2.1.1.1.1.3.1.{1|2}).

use kasa_prro::keystore::{
    cert_serial_hex, cert_signer_name, detect_format, find_signer_cert, is_dstu4145,
    load_key_material, KeyFormat,
};
use std::path::PathBuf;

fn test_jks_path() -> PathBuf {
    // CARGO_MANIFEST_DIR = .../kasa/frontend/src-tauri/crates/kasa-prro
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .unwrap()
        .join("certs/prro-test/pb_3791505547 (2).jks")
}

#[test]
fn jks_detect_format_by_extension_and_magic() {
    let path = test_jks_path();
    assert!(path.is_file(), "JKS-ключ має існувати: {}", path.display());
    assert_eq!(detect_format(&path).unwrap(), KeyFormat::Jks);
}

#[test]
fn jks_loads_private_key_and_cert_chain() {
    let path = test_jks_path();
    let m = load_key_material(&path, "test2003", None).expect("JKS читається");

    // приватний ключ (розшифрований PKCS#8) — не порожній
    let key_der = m.private_key_der.expect("приватний ключ витягнуто");
    assert!(
        key_der.len() > 100,
        "PKCS#8 DER має бути змістовним, len={}",
        key_der.len()
    );

    // OID алгоритму — ДСТУ 4145-2002 (1:1 Python `_load_from_jks`)
    let oid = m.algorithm_oid.as_deref().expect("OID визначено");
    assert!(
        is_dstu4145(Some(oid)),
        "OID має бути ДСТУ 4145 (1.2.804.2.1.1.1.1.3.1.{{1,2}}), отримано {oid}"
    );

    // ланцюг сертифікатів — 4 (1:1 Python: chain=4)
    assert_eq!(m.certs.len(), 4, "ланцюг JKS містить 4 сертифікати");

    // підписант — кінцевий сертифікат (не ЦСК)
    let signer = find_signer_cert(&m.certs).expect("підписанта знайдено");
    let serial = cert_serial_hex(&signer).unwrap();
    assert_eq!(
        serial, "5E984D526F82F38F040000006E5EE80123BED307",
        "серійний номер підписанта (1:1 Python)"
    );
    let name = cert_signer_name(&signer).unwrap();
    assert!(name.contains("МОРОЗ"), "CN підписанта: {name}");
}

#[test]
fn jks_wrong_password_fails() {
    let path = test_jks_path();
    let err = load_key_material(&path, "wrong_password", None).unwrap_err();
    assert!(
        err.to_string().contains("пароль") || err.to_string().contains("JKS"),
        "помилка: {err}"
    );
}
