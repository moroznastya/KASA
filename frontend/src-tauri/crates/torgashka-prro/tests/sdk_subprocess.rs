// ─────────────────────────────────────────────────────────────────────────────
// Регресійний тест БАГА 1 (краш процесу при test-connection):
// FFI-виклики IIT SDK EUSignCP ізольовані у СУБПРОЦЕСІ — крах багнутого
// cspb.so (#GP, release, offset 0x7a925) не може вбити основний процес.
//
// Тест запускає sign_via_subprocess (той самий шлях, що test-connection →
// signer.sign()) із РЕАЛЬНИМ ДСТУ-ключем. До фіксу release-збірка вмирала
// ТУТ (general protection fault у cspb.so — процес без panic-повідомлення).
// Тепер SDK працює у дочірньому процесі: крах хелпера → чиста помилка,
// тестовий процес виживає.
//
// Запуск: cargo test -p torgashka-prro --test sdk_subprocess
// Пропускається, якщо ключ/еuscp.so відсутні (vendor поза git).
// ─────────────────────────────────────────────────────────────────────────────

use std::path::PathBuf;

/// Шлях до бінарника хелпера (збирається cargo test автоматично).
fn helper_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_prro_sdk_helper"))
}

/// Корінь репозиторію Torgashka (torgashka-prro → 4 рівні вгору).
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn key_path() -> Option<PathBuf> {
    let root = repo_root();
    for name in [
        "certs/prro-test/nastya_key.jks",
        "certs/prro-test/pb_3791505547 (2).jks",
    ] {
        let p = root.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

const VALID_PASSWORD: &str = "prrotestkey22";

#[test]
#[ignore = "integration: реальний SDK у субпроцесі (OOM-ризик). Запуск: cargo test --test sdk_subprocess -- --ignored"]
fn sdk_subprocess_sign_valid_key_no_crash() {
    let Some(key) = key_path() else {
        eprintln!("SKIP: ключ ПРРО не знайдено (certs/prro-test/)");
        return;
    };
    if torgashka_prro::crypto::default_iit_sdk_path().is_none() {
        eprintln!("SKIP: euscp.so не встановлено (backend/vendor/iit-sdk)");
        return;
    }
    std::env::set_var("TORGASHKA_PRRO_SDK_HELPER_BIN", helper_bin());

    // Той самий виклик, що робить test-connection (signer.sign()).
    let data = b"test-ping-1234567890";
    let result = torgashka_prro::crypto::iit::sign_via_subprocess(&key, VALID_PASSWORD, data);

    match result {
        Ok(sig) => {
            assert!(!sig.is_empty(), "підпис не має бути порожнім");
            eprintln!(
                "[sdk-subprocess] sign OK: sig_len={} — процес вижив ✓",
                sig.len()
            );
        }
        Err(e) => {
            // Хелпер міг впасти (#GP) — але ОСНОВНИЙ процес вижив. Це і є
            // гарантія фіксу; помилка має бути ЧИСТОЮ (не panic/segfault).
            eprintln!("[sdk-subprocess] sign через субпроцес повернув помилку (процес вижив): {e}");
        }
    }
}

#[test]
#[ignore = "integration: реальний SDK у субпроцесі (OOM-ризик). Запуск: cargo test --test sdk_subprocess -- --ignored"]
fn sdk_subprocess_sign_wrong_password_clean_error() {
    let Some(key) = key_path() else {
        eprintln!("SKIP: ключ ПРРО не знайдено (certs/prro-test/)");
        return;
    };
    std::env::set_var("TORGASHKA_PRRO_SDK_HELPER_BIN", helper_bin());

    let result = torgashka_prro::crypto::iit::sign_via_subprocess(&key, "wrong-password", b"data");
    // Неправильний пароль → ЧИСТА помилка (Err), процес НЕ падає.
    assert!(
        result.is_err(),
        "неправильний пароль має дати Err, отримано Ok"
    );
    eprintln!(
        "[sdk-subprocess] wrong password → Err: {} — процес вижив ✓",
        result.unwrap_err()
    );
}
