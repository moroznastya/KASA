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

// ── Точка входу ─────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Плагіни
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Реєстрація команд
        .invoke_handler(tauri::generate_handler![
            // ── Команди друку ──────────────────────────────────────────
            commands::print::print_image,
            commands::print::print_raster_image,
            commands::print::get_printers,
            commands::print::open_cash_drawer,
            commands::print::get_system_info,
            // ── Команди офлайн-режиму ─────────────────────────────────
            commands::offline::is_offline_available,
            commands::offline::get_unsynced_count,
            commands::offline::cache_products,
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
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Kasa POS");
}
