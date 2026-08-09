// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Desktop (Tauri v2)
// ─────────────────────────────────────────────────────────────────────────────
// Головний модуль застосунку. Оголошує підмодулі та реєструє Tauri-команди.
// ─────────────────────────────────────────────────────────────────────────────

// ── Підмодулі ───────────────────────────────────────────────────────────────

pub mod commands;

// ── Вбудований HTTP-фасад (axum) — Rust-ядро (дезактивація Python) ──────

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

// ── Стан фасаду/сайдкара для graceful shutdown ──────────────────────────────

/// Стан вбудованого axum-фасаду (для shutdown-hook).
struct FacadeState {
    /// Таск axum-фасаду (abort при виході).
    facade: tauri::async_runtime::JoinHandle<()>,
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

            let html = kasa_infrastructure::print::commands::take_print_html(&token);

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

            // ── Підключені пристрої: автопідключення enabled-конфігів ─────
            // Завантажує devices.json і для кожного пристрою з enabled=true
            // запускає фонове підключення (ваги/термінали). Помилки не падають —
            // логуються в stderr (eprintln!).
            kasa_infrastructure::devices::init_auto_connect(app.handle());

            // ── Вбудований axum-фасад :8000 — Rust-ядро (етап 8) ───────
            // Повна дезактивація Python sidecar: фасад біндиться на :8000 і
            // обслуговує ВСІ активні роути нативно (0 CRIT, 0 ALIAS).
            // Дефолтні KASA_RUST_*=1 встановлює kasa_api::serve (єдине місце);
            // LEGACY-роути → 410 (fallback).
            let facade_addr = kasa_api::DEFAULT_FACADE_ADDR.to_string();
            let facade = tauri::async_runtime::spawn(async move {
                if let Err(e) = kasa_api::serve(&facade_addr).await {
                    eprintln!("[kasa-api] фасад завершився з помилкою: {e}");
                }
            });
            app.manage(FacadeState { facade });

            // ── SIGTERM → graceful shutdown ─────────────────────────────
            // Дефолтний обробник SIGTERM вбиває процес без RunEvent::Exit,
            // тому shutdown-hook (фасад → flush → sidecar) не виконується.
            // Перехоплюємо SIGTERM і завершуємось через app.exit(0) —
            // це гарантує виконання RunEvent::Exit і нашого hook.
            #[cfg(unix)]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = signal(SignalKind::terminate())
                        .expect("не вдалося зареєструвати SIGTERM-хендлер");
                    sigterm.recv().await;
                    eprintln!("[kasa-pos] SIGTERM отримано — graceful shutdown");
                    handle.exit(0);
                });
            }

            Ok(())
        })
        // Реєстрація команд
        .invoke_handler(tauri::generate_handler![
            // ── Команди друку ──────────────────────────────────────────
            kasa_infrastructure::print::commands::print_image,
            kasa_infrastructure::print::commands::print_raster_image,
            kasa_infrastructure::print::commands::print_html,
            kasa_infrastructure::print::commands::get_printers,
            kasa_infrastructure::print::commands::open_cash_drawer,
            kasa_infrastructure::print::commands::get_system_info,
            kasa_infrastructure::print::commands::save_receipt_image,
            // ── Команди офлайн-режиму ─────────────────────────────────
            kasa_infrastructure::offline::commands::is_offline_available,
            kasa_infrastructure::offline::commands::get_unsynced_count,
            kasa_infrastructure::offline::commands::cache_products,
            kasa_infrastructure::offline::commands::log_frontend_error,
            kasa_infrastructure::offline::commands::get_cached_products,
            kasa_infrastructure::offline::commands::save_receipt_offline,
            kasa_infrastructure::offline::commands::get_unsynced_receipts,
            kasa_infrastructure::offline::commands::mark_receipt_synced,
            kasa_infrastructure::offline::commands::get_setting,
            kasa_infrastructure::offline::commands::set_setting,
            kasa_infrastructure::offline::commands::clear_product_cache,
            kasa_infrastructure::offline::commands::get_offline_stats,
            // ── Команди системної інтеграції ──────────────────────────
            commands::system::get_app_version,
            commands::system::get_platform,
            commands::system::check_online,
            commands::system::get_barcode_scanner_info,
            commands::system::get_usb_devices,
            commands::system::get_system_status,
            commands::system::get_keyboard_layout,
            commands::system::send_notification,
            // ── Команди підключених пристроїв (ваги, термінали) ─────────
            kasa_infrastructure::devices::get_available_ports,
            kasa_infrastructure::devices::get_devices,
            kasa_infrastructure::devices::save_device_config,
            kasa_infrastructure::devices::delete_device,
            kasa_infrastructure::devices::connect_device,
            kasa_infrastructure::devices::disconnect_device,
            kasa_infrastructure::devices::get_devices_status,
            kasa_infrastructure::devices::test_connection,
            kasa_infrastructure::devices::get_system_printers,
            kasa_infrastructure::devices::get_detected_devices,
            kasa_infrastructure::devices::get_scanners,
            kasa_infrastructure::devices::terminal_payment,
            kasa_infrastructure::devices::terminal_refund,
            kasa_infrastructure::devices::terminal_cancel,
            kasa_infrastructure::devices::terminal_ping,
        ])
        .build(tauri::generate_context!())
        .expect("Помилка запуску Kasa POS")
        .run(|app_handle, event| {
            // ── Shutdown-hook: зупинка фасаду → flush → kill sidecar ──
            if let tauri::RunEvent::Exit = event {
                let state = app_handle.state::<FacadeState>();
                // 1) Зупинка axum-фасаду (звільняє :8000).
                state.facade.abort();
                // 2) Flush черг офлайн-синхронізації. На етапі 0 черг немає;
                //    місце для sync-флашу на наступних етапах.
                // Python sidecar дезактивовано (етап 8) — нема чого зупиняти.
            }
        });
}
