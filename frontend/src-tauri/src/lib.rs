// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Desktop (Tauri v2)
// ─────────────────────────────────────────────────────────────────────────────
// Головний модуль застосунку. Оголошує підмодулі та реєструє Tauri-команди.
// ─────────────────────────────────────────────────────────────────────────────

// ── Підмодулі ───────────────────────────────────────────────────────────────

pub mod commands;
pub mod db;
pub mod print;
pub mod utils;

// ── Імпорти ─────────────────────────────────────────────────────────────────

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

// ── Допоміжні функції роботи з вікном ───────────────────────────────────────

/// Показати та сфокусувати головне вікно POS
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Сховати головне вікно POS (залишити в треї)
fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// Перемкнути видимість головного вікна (для гарячої клавіші)
fn toggle_main_window(app: &tauri::AppHandle) {
    let visible = app
        .get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false);
    if visible {
        hide_main_window(app);
    } else {
        show_main_window(app);
    }
}

// ── Точка входу ─────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // ═══════════════════════════════════════════════════════════════════
        // Single-instance — РЕЄСТРУЄТЬСЯ ПЕРШИМ (вимога плагіна):
        // запобігає запуску другої копії каси на одній машині. При повторному
        // запуску — фокусуємо існуюче вікно "main" замість створення нового.
        // ═══════════════════════════════════════════════════════════════════
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        // ═══════════════════════════════════════════════════════════════════
        // Autostart — автозапуск POS разом із системою.
        // Linux: ~/.config/autostart/kasa-pos.desktop
        // Windows: ключ реєстру HKCU\...\Run
        // macOS: LaunchAgent (~/Library/LaunchAgents)
        // Аргумент "--autostart" передається процесу при автозапуску —
        // застосунок може розпізнати цей режим (напр., сховати вікно).
        // ═══════════════════════════════════════════════════════════════════
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        // ═══════════════════════════════════════════════════════════════════
        // Updater — автооновлення. Конфігурація (endpoints/pubkey) у
        // tauri.conf.json → plugins.updater. Артефакти підписуються ключем
        // із TAURI_SIGNING_PRIVATE_KEY[_PATH]/TAURI_SIGNING_PRIVATE_KEY_PASSWORD
        // (див. frontend/src-tauri/.env — у .gitignore).
        // ═══════════════════════════════════════════════════════════════════
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Плагіни
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // ── Кастомний протокол для друку HTML (print_html) ──────────────
        // Обслуговує URL виду kasa-print://localhost/{token}/ — повертає
        // HTML-документ з реєстру PRINT_HTML_REGISTRY (див. commands/print.rs).
        .register_uri_scheme_protocol("kasa-print", |_ctx, req| {
            // Витягуємо токен зі шляху: /{token}/index.html
            let token = req
                .uri()
                .path()
                .trim_start_matches('/')
                .split('/')
                .next()
                .unwrap_or_default()
                .to_string();

            let html = crate::commands::print::take_print_html(&token);

            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .status(200)
                .body(html.into_bytes())
                .unwrap_or_else(|_| {
                    tauri::http::Response::builder()
                        .status(500)
                        .body(Vec::new())
                        .unwrap()
                })
        })
        // ═══════════════════════════════════════════════════════════════════
        // Setup: трей-іконка + гарячі клавіші
        // ═══════════════════════════════════════════════════════════════════
        .setup(|app| {
            // ── Трей-іконка (меню: Показати / Приховати / Вихід) ─────────
            // ЛКМ по трею → показати вікно; ПКМ → контекстне меню.
            let show_i = MenuItem::with_id(app, "show", "Показати вікно", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Приховати", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Вихід", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            let _tray = TrayIconBuilder::with_id("kasa-tray")
                .icon(
                    app.default_window_icon()
                        .expect("іконка застосунку не знайдена")
                        .clone(),
                )
                .tooltip("Kasa POS — каса працює")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "hide" => hide_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // Клік ЛКМ по трею → показати вікно
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // ── Гарячі клавіші (глобальні) ───────────────────────────────
            // Ctrl+Shift+K — показати/сховати вікно POS
            // Ctrl+Shift+P — швидкий друк чека (подія для frontend)
            let toggle_shortcut =
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyK);
            let quick_print_shortcut =
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyP);

            app.global_shortcut()
                .on_shortcut(toggle_shortcut, |app, _sc, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_main_window(app);
                    }
                })?;

            app.global_shortcut()
                .on_shortcut(quick_print_shortcut, |app, _sc, event| {
                    if event.state() == ShortcutState::Pressed {
                        // Frontend слухає подію "quick-print-receipt"
                        // (див. src/services/tauri/quickPrint.ts) і друкує
                        // останній чек без відкриття діалогу друку.
                        let _ = app.emit("quick-print-receipt", ());
                    }
                })?;

            Ok(())
        })
        // Реєстрація команд
        .invoke_handler(tauri::generate_handler![
            // ── Команди друку ──────────────────────────────────────────
            commands::print::print_image,
            commands::print::print_raster_image,
            commands::print::print_html,
            commands::print::get_printers,
            commands::print::open_cash_drawer,
            commands::print::get_system_info,
            commands::print::save_receipt_image,
            // ── Команди офлайн-режиму ─────────────────────────────────
            commands::offline::is_offline_available,
            commands::offline::get_unsynced_count,
            commands::offline::cache_products,
            commands::offline::log_frontend_error,
            commands::offline::get_cached_products,
            commands::offline::save_receipt_offline,
            commands::offline::get_unsynced_receipts,
            commands::offline::mark_receipt_synced,
            commands::offline::get_setting,
            commands::offline::set_setting,
            commands::offline::clear_product_cache,
            commands::offline::get_offline_stats,
            // ── Команди системної інтеграції ──────────────────────────
            commands::system::get_app_version,
            commands::system::get_platform,
            commands::system::check_online,
            commands::system::get_barcode_scanner_info,
            commands::system::get_usb_devices,
            commands::system::get_system_status,
            commands::system::get_keyboard_layout,
            commands::system::send_notification,
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Kasa POS");
}
