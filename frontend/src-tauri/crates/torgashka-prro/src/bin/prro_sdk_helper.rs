// ─────────────────────────────────────────────────────────────────────────────
// prro_sdk_helper — ізольований субпроцес для FFI-викликів IIT SDK EUSignCP.
// ─────────────────────────────────────────────────────────────────────────────
// Основний процес (Tauri app / facade) НІКОЛИ не викликає euscp.so напряму:
// IitSigner::sign/verify запускає helper (current_exe у режимі
// TORGASHKA_PRRO_SDK_HELPER або цей бін через TORGASHKA_PRRO_SDK_HELPER_BIN).
// Крах багнутого cspb.so (#GP/SIGSEGV — відтворено у release) вбиває лише
// цей субпроцес; Torgashka отримує чисту помилку → HTTP 400.
//
// Протокол: JSON у stdin → JSON у stdout (див. iit::sdk_helper_main).
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    std::process::exit(torgashka_prro::crypto::iit::sdk_helper_main());
}
