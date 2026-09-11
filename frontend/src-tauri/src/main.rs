// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Desktop Entry Point
// ─────────────────────────────────────────────────────────────────────────────
// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Записує діагностичну інформацію про середовище у /tmp/torgashka-debug.log.
/// Викликається ДО і ПІСЛЯ встановлення змінних рендерингу — щоб можна було
/// порівняти, які змінні були задані ззовні, а які встановив сам застосунок.
fn write_debug_log(stage: &str) {
    let mut content = String::new();
    content.push_str(&format!("[{}]\n", stage));

    // Значення ключових змінних середовища (графічна сесія, рендеринг WebKit)
    let vars = [
        "GDK_BACKEND",
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XDG_SESSION_TYPE",
        "XDG_SESSION_DESKTOP",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "LIBGL_ALWAYS_SOFTWARE",
    ];
    for var in vars {
        let value = std::env::var(var).unwrap_or_default();
        content.push_str(&format!("{}={}\n", var, value));
    }

    // Версія WebKit2GTK (якщо pkg-config доступний у системі)
    let webkit_version = std::process::Command::new("pkg-config")
        .args(["--modversion", "webkit2gtk-4.1"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    content.push_str(&format!("WEBKIT_VERSION={}\n", webkit_version));

    // Перезаписуємо файл — останній виклик (AFTER) містить повну картину
    let _ = std::fs::write("/tmp/torgashka-debug.log", content);
}

fn main() {
    // ── Режим SDK-хелпера ПРРО (ізоляція FFI EUSignCP у субпроцесі) ──────
    // IitSigner::sign/verify запускає current_exe з TORGASHKA_PRRO_SDK_HELPER=1;
    // хелпер робить SDK-роботу (load_jks_key/sign) і виходить. Крах багнутого
    // cspb.so (#GP/SIGSEGV) вбиває лише цей субпроцес — Torgashka виживає.
    // Перевірка ОБОВ'ЯЗКОВО до ініціалізації Tauri/GTK (хелпер без GUI).
    if std::env::var_os(torgashka_prro::crypto::iit::SDK_HELPER_ENV).is_some() {
        std::process::exit(torgashka_prro::crypto::iit::sdk_helper_main());
    }

    // ── Діагностика: стан середовища ДО встановлення змінних ──
    write_debug_log("BEFORE");

    // ─────────────────────────────────────────────────────────────────────────
    // ФІКС РЕНДЕРИНГУ ДЛЯ VM БЕЗ GPU/DRI3 (обов'язково ДО ініціалізації GTK/WebKit!)
    // ─────────────────────────────────────────────────────────────────────────
    // На цільовій VM зламаний GPU/DRI3 (лог: "libEGL warning: DRI3 error: Could
    // not get DRI3 device"). Шлях X11→XWayland зі зламаним GL НЕ презентує
    // контентні шари: видно лише фон #0F172B + скролбар (перевірено скриншотами:
    // всього 3 кольори на histogram).
    //
    // ПЕРЕВІРЕНО ТЕСТОМ (python3-gi + WebKit2GTK 4.1, ті самі змінні):
    // на НАТИВНОМУ Wayland WebKit рендерить сторінку Torgashka повністю —
    // 2004 унікальні кольори + текст (histogram), EGL-варнінги зникають.
    // Тому GDK_BACKEND НЕ форсуємо — GDK сам обирає wayland за наявності
    // WAYLAND_DISPLAY, WebKit малює через wl_shm у software-режимі:
    //   - WEBKIT_DISABLE_DMABUF_RENDERER=1 — без DMABUF (немає DRI3);
    //   - WEBKIT_DISABLE_COMPOSITING_MODE=1 — без композитингу шарів;
    //   - LIBGL_ALWAYS_SOFTWARE=1 — OpenGL через llvmpipe (без апаратного GPU).
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");

    // ── Діагностика: стан середовища ПІСЛЯ встановлення змінних ──
    write_debug_log("AFTER");

    torgashka_lib::run()
}
