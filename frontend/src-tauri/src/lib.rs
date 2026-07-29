// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Desktop (Tauri)
// ─────────────────────────────────────────────────────────────────────────────

mod escpos;
mod print;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Нова команда: прямий ESC/POS друк
            print_receipt_escpos,
            // Старі команди (для сумісності)
            print_document,
            print_receipt,
            print_receipt_html,
            print_raster_image,
            get_printers,
            print_preview,
        ])
        .run(tauri::generate_context!())
        .expect("Помилка запуску Tauri");
}

// ─────────────────────────────────────────────────────────────────────────────
// Допоміжна функція: конвертує PrintError у String для Tauri
// ─────────────────────────────────────────────────────────────────────────────
fn map_err(e: print::PrintError) -> String {
    e.to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// НОВА КОМАНДА: Прямий друк ESC/POS
// ─────────────────────────────────────────────────────────────────────────────
//
// Фронтенд надсилає JSON із даними чеку.
// Rust генерує сирі ESC/POS байти та пише на порт принтера.
//
// Використання:
//   invoke('print_receipt_escpos', {
//     data: {
//       shop_name: "Калина",
//       shop_address: "вул. Центральна, 1",
//       ...,
//       items: [{ name: "Хліб", quantity: 1, price: 25.00, total: 25.00 }]
//     },
//     device_path: "/dev/usb/lp0"   // опціонально
//   })
//
#[tauri::command]
fn print_receipt_escpos(
    data: escpos::ReceiptPrintRequest,
    printer_name: Option<String>,
    device_path: Option<String>,
) -> Result<String, String> {
    eprintln!("[KASA] print_receipt_escpos: {} товарів, принтер={:?}, пристрій={:?}",
        data.items.len(), printer_name, device_path);

    // 1. Генеруємо ESC/POS байти
    let escpos_bytes = escpos::build_receipt_escpos(&data);
    eprintln!("[KASA] Згенеровано {} байтів ESC/POS", escpos_bytes.len());

    // Зберігаємо для діагностики
    let _ = std::fs::write("/tmp/kasa_last_escpos.bin", &escpos_bytes);

    // 2. Відправляємо на принтер
    print::print_escpos(&escpos_bytes, printer_name.as_deref(), device_path.as_deref())
        .map_err(map_err)?;

    Ok(format!("OK: надіслано {} байтів", escpos_bytes.len()))
}

// ─────────────────────────────────────────────────────────────────────────────
// СТАРІ КОМАНДИ (для сумісності)
// ─────────────────────────────────────────────────────────────────────────────

/// Друк HTML-документа (через CUPS/lp або wkhtmltopdf)
#[tauri::command]
fn print_document(html: String, printer_name: Option<String>) -> Result<String, String> {
    eprintln!("[KASA] print_document: printer={:?}, html_len={}", printer_name, html.len());
    print::print_html(&html, printer_name.as_deref()).map_err(map_err)?;
    Ok("OK".to_string())
}

/// Друк чека простим текстом (графічний режим ESC/POS)
#[tauri::command]
fn print_receipt(text: String, printer_name: Option<String>) -> Result<String, String> {
    eprintln!("[KASA] print_receipt: printer={:?}, text_len={}", printer_name, text.len());
    print::print_receipt_text(&text, printer_name.as_deref()).map_err(map_err)?;
    Ok("OK".to_string())
}

/// Друк HTML-чека (Chrome headless → PNG → ESC/POS, або fallback до текстового парсера)
#[tauri::command]
fn print_receipt_html(html: String, printer_name: Option<String>) -> Result<String, String> {
    eprintln!("[KASA] print_receipt_html: printer={:?}, html_len={}", printer_name, html.len());
    if html.len() > 300 {
        eprintln!("[KASA] HTML (first 300): {:?}", &html[..300]);
    } else {
        eprintln!("[KASA] HTML (full): {:?}", &html);
    }
    let result = print::print_receipt_html(&html, printer_name.as_deref()).map_err(map_err);
    match &result {
        Ok(_) => eprintln!("[KASA] print_receipt_html OK"),
        Err(e) => eprintln!("[KASA] print_receipt_html ERROR: {}", e),
    }
    result?;
    Ok("OK".to_string())
}

/// Друк растрового зображення (PNG bytes → ESC/POS)
#[tauri::command]
fn print_raster_image(image_data: Vec<u8>, printer_name: Option<String>) -> Result<String, String> {
    eprintln!("[KASA] print_raster_image: data_len={}, printer={:?}", image_data.len(), printer_name);
    let result = print::print_raster_image(image_data, printer_name.as_deref()).map_err(map_err);
    match &result {
        Ok(_) => eprintln!("[KASA] print_raster_image OK"),
        Err(e) => eprintln!("[KASA] print_raster_image ERROR: {}", e),
    }
    result?;
    Ok("OK".to_string())
}

/// Отримати список принтерів
#[tauri::command]
fn get_printers() -> Result<Vec<String>, String> {
    print::get_printers().map_err(map_err)
}

/// Попередній перегляд
#[tauri::command]
fn print_preview(html: String) -> Result<String, String> {
    print::print_preview(&html).map_err(map_err)?;
    Ok("OK".to_string())
}
