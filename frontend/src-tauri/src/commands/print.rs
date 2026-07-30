// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Tauri Команди друку
// ─────────────────────────────────────────────────────────────────────────────
//
// Містить Tauri-команди для друку зображень (Print-as-Image).
// Решта формування чека виконується на frontend (React → html2canvas → PNG),
// а Rust отримує готове зображення та конвертує в ESC/POS растр.
// ─────────────────────────────────────────────────────────────────────────────

use crate::print;
use crate::utils::cash_drawer;

// ─────────────────────────────────────────────────────────────────────────────
// Структури даних
// ─────────────────────────────────────────────────────────────────────────────

/// Результат друку
#[derive(serde::Serialize, Clone)]
pub struct PrintResult {
    pub success: bool,
    pub message: String,
    pub bytes_sent: Option<usize>,
}

/// Дані для друку зображення (Print-as-Image) з Base64 рядка
#[derive(serde::Deserialize, Clone, Debug)]
pub struct PrintImageData {
    /// Base64-рядок зображення (PNG, без префіксу data:image/png;base64,)
    pub image_base64: String,
    /// Назва принтера (опціонально)
    pub printer_name: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Допоміжні функції
// ─────────────────────────────────────────────────────────────────────────────

fn map_print_err(e: print::PrintError) -> String {
    e.to_string()
}

fn ok_result(msg: String, bytes: Option<usize>) -> PrintResult {
    PrintResult {
        success: true,
        message: msg,
        bytes_sent: bytes,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Команди
// ─────────────────────────────────────────────────────────────────────────────

/// Друк зображення з Base64 рядка (Print-as-Image).
///
/// React рендерить чек, конвертує в Base64 PNG і надсилає в Rust.
/// Base64 має бути без префіксу `data:image/png;base64,` — тільки чистий Base64.
#[tauri::command]
pub fn print_image(data: PrintImageData) -> Result<PrintResult, String> {
    eprintln!(
        "[KASA] print_image: base64_len={}, printer={:?}",
        data.image_base64.len(),
        data.printer_name
    );

    // Декодуємо Base64
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let image_bytes = engine
        .decode(&data.image_base64)
        .map_err(|e| format!("Помилка декодування Base64: {}", e))?;

    // Використовуємо існуючу функцію print_raster_image
    print::print_raster_image(image_bytes, data.printer_name.as_deref())
        .map_err(|e| e.to_string())?;

    Ok(PrintResult {
        success: true,
        message: "Зображення надруковано".to_string(),
        bytes_sent: None,
    })
}

/// Друк растрового зображення (PNG bytes → ESC/POS)
#[tauri::command]
pub fn print_raster_image(
    image_data: Vec<u8>,
    printer_name: Option<String>,
) -> Result<PrintResult, String> {
    eprintln!(
        "[KASA] print_raster_image: data_len={}, printer={:?}",
        image_data.len(),
        printer_name
    );
    let result =
        print::print_raster_image(image_data, printer_name.as_deref()).map_err(map_print_err);
    match &result {
        Ok(_) => eprintln!("[KASA] print_raster_image OK"),
        Err(e) => eprintln!("[KASA] print_raster_image ERROR: {}", e),
    }
    result?;
    Ok(ok_result("OK".to_string(), None))
}

/// Отримати список доступних принтерів
#[tauri::command]
pub fn get_printers() -> Result<Vec<String>, String> {
    print::get_printers().map_err(map_print_err)
}

/// Відкрити грошову скриньку
#[tauri::command]
pub fn open_cash_drawer(device_path: Option<String>) -> Result<PrintResult, String> {
    cash_drawer::open_cash_drawer(device_path.as_deref())
        .map(|_| ok_result("Скриньку відкрито".to_string(), None))
        .map_err(|e| e.to_string())
}

/// Отримати інформацію про систему
#[tauri::command]
pub fn get_system_info() -> Result<serde_json::Value, String> {
    let info = serde_json::json!({
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "hostname": hostname(),
        "username": whoami(),
    });
    Ok(info)
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}
