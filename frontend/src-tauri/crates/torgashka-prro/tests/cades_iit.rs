//! CAdES-BES (ДСТУ 4145-2002, крипто-ядро ІІТ EUSignCP) — golden/сумісність.
//!
//! Тести вимагають euscp.so (backend/vendor/iit-sdk) — якщо SDK не
//! встановлено, тести пропускаються з явним повідомленням (критерій 7.2 #4).
//!
//! Підпис ДСТУ 4145 НЕ детермінований (випадковий k) → байт-ідентичність
//! неможлива; golden = взаємна verify-сумісність Rust↔Python + структура
//! ContentInfo/signedData.

use torgashka_prro::crypto::{default_iit_sdk_path, signer_from_key_material};
use torgashka_prro::keystore;

fn iit_available() -> Option<std::path::PathBuf> {
    default_iit_sdk_path()
}

/// Знаходить JKS-ключ ДСТУ (certs/prro-test/pb_3791505547 (2).jks).
fn dstu_jks_path() -> std::path::PathBuf {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .unwrap()
        .join("certs/prro-test/pb_3791505547 (2).jks")
}

#[test]
fn cades_signer_serial_and_name_match_python() {
    let Some(so) = iit_available() else {
        eprintln!("SKIP: euscp.so не встановлено (backend/vendor/iit-sdk)");
        return;
    };
    let mut sdk = torgashka_prro::crypto::iit::IitSdk::load(&so).expect("SDK завантажується");
    sdk.initialize(None, &torgashka_prro::crypto::default_iit_cert_store().unwrap())
        .expect("EUInitialize");
    sdk.load_jks_key(&dstu_jks_path(), "test2003")
        .expect("JKS ДСТУ завантажується");

    // Python: serial=5E984D526F82F38F040000006E5EE80123BED307,
    //         name="МОРОЗ АНАСТАСІЯ-РОКСОЛАНА ВАСИЛІВНА"
    assert_eq!(
        sdk.get_signer_serial().expect("serial"),
        "5E984D526F82F38F040000006E5EE80123BED307"
    );
    let name = sdk.get_signer_name().expect("name");
    assert!(
        name.contains("МОРОЗ") && name.contains("АНАСТАСІЯ"),
        "name={name:?}"
    );
}

#[test]
fn cades_sign_verify_and_structure() {
    let Some(so) = iit_available() else {
        eprintln!("SKIP: euscp.so не встановлено (backend/vendor/iit-sdk)");
        return;
    };
    let mut sdk = torgashka_prro::crypto::iit::IitSdk::load(&so).expect("SDK");
    sdk.initialize(None, &torgashka_prro::crypto::default_iit_cert_store().unwrap())
        .expect("init");
    sdk.load_jks_key(&dstu_jks_path(), "test2003").expect("jks");

    let xml = b"<DAT DI=\"1\" FN=\"4538765845\" TN=\"345612052809\" V=\"1\" ZN=\"AA57506761\"><C T=\"0\"><P C=\"120\" NM=\"Test\" PRC=\"100\" Q=\"1\" SM=\"100\" TX=\"0\"/></C><TS>20260807112601</TS></DAT>";
    let sig = sdk.sign_data_internal(xml).expect("sign");

    // структура: ContentInfo signedData (OID 1.2.840.113549.1.7.2)
    assert_eq!(&sig[0], &0x30, "SEQUENCE");
    assert_eq!(&sig[1], &0x82, "довга форма довжини");
    assert_eq!(
        &sig[4..15],
        &[0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02, 0xa0][..11],
        "OID signedData"
    );
    assert!(
        sig.len() > 2000,
        "CAdES підпис не порожній ({}B)",
        sig.len()
    );

    // verify (дані всередині підпису — internal)
    assert!(sdk.verify_data_internal(&sig, None).expect("verify"));
    // verify з очікуваними даними
    assert!(sdk
        .verify_data_internal(&sig, Some(xml))
        .expect("verify data"));
    // підпис іншого документа не відповідає даним
    assert!(!sdk
        .verify_data_internal(&sig, Some(b"<DAT>other</DAT>"))
        .expect("verify other"));
    // пошкоджений підпис не валідний
    let mut bad = sig.clone();
    let n = bad.len();
    bad[n - 5] ^= 0xFF;
    assert!(!sdk.verify_data_internal(&bad, None).expect("verify bad"));
}

#[test]
fn cades_verify_python_golden_signature() {
    let Some(so) = iit_available() else {
        eprintln!("SKIP: euscp.so не встановлено (backend/vendor/iit-sdk)");
        return;
    };
    // CAdES недетермінований (випадковий k у ДСТУ 4145) — golden-вектор:
    // Python-підпис зафіксований у fixtures; Rust МАЄ його верифікувати.
    let golden = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cades_python_sig.bin"
    ))
    .expect("cades_python_sig.bin");
    assert!(golden.len() > 2000, "golden не порожній");

    let mut sdk = torgashka_prro::crypto::iit::IitSdk::load(&so).expect("SDK");
    sdk.initialize(None, &torgashka_prro::crypto::default_iit_cert_store().unwrap())
        .expect("init");
    sdk.load_jks_key(&dstu_jks_path(), "test2003").expect("jks");
    assert!(
        sdk.verify_data_internal(&golden, None).expect("verify"),
        "Rust verify Python CAdES golden"
    );
    // дані всередині підпису відповідають очікуваним
    let xml = b"<DAT DI=\"1\" FN=\"4538765845\" TN=\"345612052809\" V=\"1\" ZN=\"AA57506761\"><C T=\"0\"><P C=\"120\" NM=\"Golden\" PRC=\"100\" Q=\"1\" SM=\"100\" TX=\"0\"/></C><TS>20260807112601</TS></DAT>";
    assert!(
        sdk.verify_data_internal(&golden, Some(xml))
            .expect("verify data"),
        "Rust verify Python CAdES golden + data"
    );
}

#[test]
fn cades_signer_from_key_material_integration() {
    let Some(_so) = iit_available() else {
        eprintln!("SKIP: euscp.so не встановлено (backend/vendor/iit-sdk)");
        return;
    };
    // повний шлях: key_store (JKS) → фабрика → sign → verify (як Python
    // PrroCryptoSigner з бекендом iit)
    let material =
        keystore::load_key_material(&dstu_jks_path(), "test2003", None).expect("JKS читається");
    assert_eq!(
        material.algorithm_oid.as_deref(),
        Some("1.2.804.2.1.1.1.1.3.1.1"),
        "OID ДСТУ 4145 little endian"
    );
    let signer = signer_from_key_material(&material, "test2003").expect("фабрика → IitSigner");
    let xml = b"<DAT DI=\"1\"><C T=\"0\"><P C=\"1\" NM=\"A\" PRC=\"1\" Q=\"1\" SM=\"1\" TX=\"0\"/></C><TS>20260807112601</TS></DAT>";
    let signed = signer.sign(xml).expect("CAdES sign");
    assert!(signer.verify(&signed).expect("CAdES verify"));
    assert_eq!(
        signer.get_serial_number().expect("serial"),
        "5E984D526F82F38F040000006E5EE80123BED307"
    );
}
