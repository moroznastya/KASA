// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Tauri Команди друку
// ─────────────────────────────────────────────────────────────────────────────
//
// Містить Tauri-команди для другу чеків, звітів та інших документів.
// Використовує низькорівневі модулі `escpos` та `print`.
// ─────────────────────────────────────────────────────────────────────────────

use crate::escpos;
use crate::print;
use crate::utils::cash_drawer;

// ─────────────────────────────────────────────────────────────────────────────
// Структури даних для команд (відповідають escpos::ReceiptPrintRequest)
// ─────────────────────────────────────────────────────────────────────────────

/// Дані для друку чека через ESC/POS
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct ReceiptPrintData {
    pub shop_name: String,
    pub shop_address: String,
    pub tax_id: String,
    pub receipt_number: String,
    pub date: String,
    pub time: String,
    pub cashier: String,
    pub items: Vec<ReceiptItemData>,
    pub total: f64,
    pub payment_method: String,
    pub paid: f64,
    pub change: f64,
    pub footer: Option<String>,
    /// Номер оригінального чеку (для повернення)
    pub original_receipt_number: Option<String>,
    /// Причина повернення
    pub return_reason: Option<String>,
}

/// Товар у чеку
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct ReceiptItemData {
    pub barcode: Option<String>,
    pub name: String,
    pub quantity: f64,
    pub price: f64,
    pub total: f64,
}

/// Результат друку
#[derive(serde::Serialize, Clone)]
pub struct PrintResult {
    pub success: bool,
    pub message: String,
    pub bytes_sent: Option<usize>,
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

#[allow(dead_code)]
fn err_result(msg: String) -> PrintResult {
    PrintResult {
        success: false,
        message: msg,
        bytes_sent: None,
    }
}

/// Конвертувати ReceiptPrintData в escpos::ReceiptPrintRequest
fn to_escpos_request(data: &ReceiptPrintData) -> escpos::ReceiptPrintRequest {
    escpos::ReceiptPrintRequest {
        shop_name: data.shop_name.clone(),
        shop_address: data.shop_address.clone(),
        tax_id: data.tax_id.clone(),
        receipt_number: data.receipt_number.clone(),
        date: data.date.clone(),
        time: data.time.clone(),
        cashier: data.cashier.clone(),
        items: data.items.iter().map(|i| escpos::ReceiptItemData {
            barcode: i.barcode.clone(),
            name: i.name.clone(),
            quantity: i.quantity,
            price: i.price,
            total: i.total,
        }).collect(),
        total: data.total,
        payment_method: data.payment_method.clone(),
        paid: data.paid,
        change: data.change,
        footer: data.footer.clone(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Команди
// ─────────────────────────────────────────────────────────────────────────────

/// Прямий друк ESC/POS — генерує сирі байти та надсилає на принтер
#[tauri::command]
pub fn print_receipt_escpos(
    data: ReceiptPrintData,
    printer_name: Option<String>,
    device_path: Option<String>,
) -> Result<PrintResult, String> {
    eprintln!(
        "[KASA] print_receipt_escpos: {} товарів, принтер={:?}, пристрій={:?}",
        data.items.len(),
        printer_name,
        device_path
    );

    // Конвертуємо та генеруємо ESC/POS байти
    let request = to_escpos_request(&data);
    let escpos_bytes = escpos::build_receipt_escpos(&request);
    eprintln!("[KASA] Згенеровано {} байтів ESC/POS", escpos_bytes.len());

    // Зберігаємо для діагностики
    let _ = std::fs::write("/tmp/kasa_last_escpos.bin", &escpos_bytes);

    // Відправляємо на принтер
    print::print_escpos(
        &escpos_bytes,
        printer_name.as_deref(),
        device_path.as_deref(),
    )
    .map_err(map_print_err)?;

    Ok(ok_result(
        format!("OK: надіслано {} байтів", escpos_bytes.len()),
        Some(escpos_bytes.len()),
    ))
}

/// Друк HTML-документа (через CUPS/lp)
#[tauri::command]
pub fn print_document(html: String, printer_name: Option<String>) -> Result<PrintResult, String> {
    eprintln!(
        "[KASA] print_document: printer={:?}, html_len={}",
        printer_name,
        html.len()
    );
    print::print_html(&html, printer_name.as_deref()).map_err(map_print_err)?;
    Ok(ok_result("OK".to_string(), None))
}

/// Друк чека простим текстом (графічний режим ESC/POS)
#[tauri::command]
pub fn print_receipt(text: String, printer_name: Option<String>) -> Result<PrintResult, String> {
    eprintln!(
        "[KASA] print_receipt: printer={:?}, text_len={}",
        printer_name,
        text.len()
    );
    print::print_receipt_text(&text, printer_name.as_deref()).map_err(map_print_err)?;
    Ok(ok_result("OK".to_string(), None))
}

/// Друк HTML-чека (Chrome headless → PNG → ESC/POS або текстовий fallback)
#[tauri::command]
pub fn print_receipt_html(html: String, printer_name: Option<String>) -> Result<PrintResult, String> {
    eprintln!(
        "[KASA] print_receipt_html: printer={:?}, html_len={}",
        printer_name,
        html.len()
    );

    let result = print::print_receipt_html(&html, printer_name.as_deref()).map_err(map_print_err);
    match &result {
        Ok(_) => eprintln!("[KASA] print_receipt_html OK"),
        Err(e) => eprintln!("[KASA] print_receipt_html ERROR: {}", e),
    }
    result?;
    Ok(ok_result("OK".to_string(), None))
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

/// Попередній перегляд чека
#[tauri::command]
pub fn print_preview(html: String) -> Result<PrintResult, String> {
    print::print_preview(&html).map_err(map_print_err)?;
    Ok(ok_result("OK".to_string(), None))
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
