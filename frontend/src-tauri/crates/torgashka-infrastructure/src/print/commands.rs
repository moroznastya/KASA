// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди друку
// ─────────────────────────────────────────────────────────────────────────────
//
// Містить Tauri-команди для друку зображень (Print-as-Image).
// Решта формування чека виконується на frontend (React → html2canvas → PNG),
// а Rust отримує готове зображення та конвертує в ESC/POS растр.
//
// Підтримка копій та обрізки (додано 31.07.2026):
//   - `copies`   — Option<u32>, дефолт 1 (налаштування print_copies з БД)
//   - `auto_cut` — Option<bool>, дефолт true (auto_cut_paper з БД)
//   Старі виклики frontend без цих параметрів працюють як раніше.
// ─────────────────────────────────────────────────────────────────────────────

use crate::cash_drawer;
use crate::print;

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
    /// Шлях до пристрою (опціонально, напр. /dev/usb/lp0)
    pub device_path: Option<String>,
    /// Фізична ширина етикетки в мм (опціонально; для термо-етикеток).
    /// Якщо задані width_mm/height_mm — Rust масштабує PNG ТОЧНО під мм.
    pub width_mm: Option<f64>,
    /// Фізична висота етикетки в мм (опціонально; для термо-етикеток).
    pub height_mm: Option<f64>,
    /// Роздільна здатність принтера, dots/inch (опціонально; дефолт 203).
    pub dpi: Option<u32>,
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
///
/// # Параметри (опціональні, для зворотної сумісності)
/// - `copies`   — кількість копій (None → 1)
/// - `auto_cut` — обрізка паперу GS V 0 (None → true)
///
/// # Розміри етикетки (опціонально, для термо-етикеток)
/// - `data.width_mm`  — фізична ширина етикетки в мм
/// - `data.height_mm` — фізична висота етикетки в мм
/// - `data.dpi`       — роздільна здатність принтера (None → 203)
///
/// Якщо width_mm/height_mm задані — Rust масштабує PNG ТОЧНО під мм
/// (Lanczos3): 58×40мм @ 203dpi → (384, 320) dots = 48×40мм фізично.
/// Якщо НЕ задані (чеки) — стара логіка (масштаб до 384 лише якщо > 384).
#[tauri::command]
pub fn print_image(
    data: PrintImageData,
    copies: Option<u32>,
    auto_cut: Option<bool>,
) -> Result<PrintResult, String> {
    // Дефолти: copies=1, auto_cut=true (зворотна сумісність).
    // copies обмежується діапазоном [1..100] — захист від DoS/OOM (4 млрд копій).
    let copies = copies.unwrap_or(1).clamp(1, 100);
    let auto_cut = auto_cut.unwrap_or(true);

    eprintln!(
        "[TORGASHKA] print_image: base64_len={}, printer={:?}, device={:?}, copies={}, auto_cut={}, width_mm={:?}, height_mm={:?}, dpi={:?}",
        data.image_base64.len(),
        data.printer_name,
        data.device_path,
        copies,
        auto_cut,
        data.width_mm,
        data.height_mm,
        data.dpi
    );

    // Декодуємо Base64
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let image_bytes = engine
        .decode(&data.image_base64)
        .map_err(|e| format!("Помилка декодування Base64: {}", e))?;

    // Використовуємо print_raster_image з маршрутизацією (порт → lp)
    // width_mm/height_mm/dpi проброшуються для точного друку термо-етикеток
    print::print_raster_image(
        image_bytes,
        data.printer_name.as_deref(),
        data.device_path.as_deref(),
        copies,
        auto_cut,
        data.width_mm,
        data.height_mm,
        data.dpi,
    )
    .map_err(|e| e.to_string())?;

    Ok(PrintResult {
        success: true,
        message: "Зображення надруковано".to_string(),
        bytes_sent: None,
    })
}

/// Друк растрового зображення (PNG bytes → ESC/POS)
///
/// # Параметри (опціональні, для зворотної сумісності)
/// - `copies`   — кількість копій (None → 1)
/// - `auto_cut` — обрізка паперу GS V 0 (None → true)
#[tauri::command]
pub fn print_raster_image(
    image_data: Vec<u8>,
    printer_name: Option<String>,
    device_path: Option<String>,
    copies: Option<u32>,
    auto_cut: Option<bool>,
) -> Result<PrintResult, String> {
    // Дефолти: copies=1, auto_cut=true (зворотна сумісність).
    // copies обмежується діапазоном [1..100] — захист від DoS/OOM (4 млрд копій).
    let copies = copies.unwrap_or(1).clamp(1, 100);
    let auto_cut = auto_cut.unwrap_or(true);

    eprintln!(
        "[TORGASHKA] print_raster_image: data_len={}, printer={:?}, device={:?}, copies={}, auto_cut={}",
        image_data.len(),
        printer_name,
        device_path,
        copies,
        auto_cut
    );

    let result = print::print_raster_image(
        image_data,
        printer_name.as_deref(),
        device_path.as_deref(),
        copies,
        auto_cut,
        None, // width_mm — низькорівневий виклик без розмірів етикетки
        None, // height_mm
        None, // dpi (дефолт 203 у print::print_raster_image)
    )
    .map_err(map_print_err);

    match &result {
        Ok(_) => eprintln!("[TORGASHKA] print_raster_image OK"),
        Err(e) => eprintln!("[TORGASHKA] print_raster_image ERROR: {}", e),
    }

    result?;
    Ok(ok_result("OK".to_string(), None))
}

// ─────────────────────────────────────────────────────────────────────────────
// 🖨️ ДРУК HTML (A4) — НАТИВНИЙ ДРУК ЧЕРЕЗ СИСТЕМНИЙ ДІАЛОГ (webkit2gtk)
// ─────────────────────────────────────────────────────────────────────────────
//
// Вирішує проблему html2canvas → PNG → ESC/POS:
//   - html2canvas НЕ підтримує CSS Grid і page-break → сітка цінників
//     ламалась, багатосторінкові документи не розбивались на сторінки.
//   - webkit2gtk рендерить HTML нативно: Grid, SVG, шрифти, page-break
//     працюють коректно.
//
// РЕАЛІЗАЦІЯ (Tauri v2):
//   Tauri v2 НЕ має WebviewUrl::Html — замість нього використовується
//   кастомний URI-протокол `torgashka-print://` (реєструється ОДИН раз у lib.rs
//   через Builder::register_uri_scheme_protocol):
//     1. print_html зберігає HTML у реєстрі PRINT_HTML_REGISTRY під
//        унікальним токеном (uuid).
//     2. Створюється WebviewWindow з URL `torgashka-print://localhost/{token}/` —
//        webkit2gtk запитує цей URL, протокол-хендлер повертає HTML.
//     3. Після завантаження (on_page_load → Finished) через webview.eval()
//        викликається window.print() → СИСТЕМНИЙ діалог друку.
//     4. window.print() блокує JS-потік webview; після закриття діалогу
//        виконується window.close() → вікно закривається.
//     5. Watchdog (60 с) гарантовано закриває вікно та чистить реєстр.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Mutex;

/// Реєстр HTML-документів для кастомного протоколу `torgashka-print://`.
///
/// Ключ — унікальний токен (uuid), значення — HTML-документ для друку.
/// Використання токена в URL (`torgashka-print://localhost/{token}/`) унеможливлює
/// гонки при послідовних/одночасних друках — кожне вікно отримує СВІЙ HTML.
static PRINT_HTML_REGISTRY: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Зберегти HTML у реєстрі під токеном
pub(crate) fn store_print_html(token: &str, html: String) {
    if let Ok(mut map) = PRINT_HTML_REGISTRY.lock() {
        map.insert(token.to_string(), html);
    }
}

/// Отримати HTML з реєстру за токеном (для протокол-хендлера)
pub fn take_print_html(token: &str) -> String {
    PRINT_HTML_REGISTRY
        .lock()
        .map(|map| map.get(token).cloned().unwrap_or_default())
        .unwrap_or_default()
}

/// Видалити HTML з реєстру (після завершення друку)
pub(crate) fn remove_print_html(token: &str) {
    if let Ok(mut map) = PRINT_HTML_REGISTRY.lock() {
        map.remove(token);
    }
}

/// Дані для друку HTML-документа (A4)
#[derive(serde::Deserialize, Clone, Debug)]
pub struct PrintHtmlData {
    /// Повний HTML-документ для друку (з <html>, CSS-стилями)
    pub html: String,
    /// Назва принтера (підказка для системного діалогу; опціонально)
    pub printer_name: Option<String>,
}

/// Друк HTML-документа НАТИВНО через системний діалог друку (webkit2gtk).
///
/// # Алгоритм
///   1. HTML зберігається в реєстрі під унікальним токеном (uuid).
///   2. Створюється невелике WebviewWindow (Tauri v2 API) з URL
///      `torgashka-print://localhost/{token}/` — кастомний протокол, який
///      повертає HTML (нативний рендер webkit2gtk: Grid, SVG, page-break).
///   3. Після повного завантаження (`on_page_load` → Finished) через
///      `webview.eval()` викликається `window.print()` — системний діалог друку.
///   4. `window.print()` блокує JS-потік webview, поки діалог відкритий;
///      після закриття діалогу виконується `window.close()` — вікно закривається.
///   5. Watchdog-потік (60 с) гарантовано закриває вікно та чистить реєстр,
///      якщо вікно не закрилось самостійно (помилка print-діалогу тощо).
///
/// # Примітка про printer_name
/// Системний діалог дозволяє обрати принтер вручну; `printer_name` наразі
/// використовується як підказка/лог. Silent-друк на конкретний принтер БЕЗ
/// діалогу — майбутнє розширення через GtkPrintOperation (FFI, webkit2gtk-sys).
#[tauri::command]
pub fn print_html(app: tauri::AppHandle, data: PrintHtmlData) -> Result<PrintResult, String> {
    // Унікальний токен — дозволяє друкувати кілька документів поспіль
    let token = uuid::Uuid::new_v4().to_string();
    let label = format!("print-html-{}", token);

    eprintln!(
        "[TORGASHKA] print_html: html_len={}, printer={:?}, window='{}'",
        data.html.len(),
        data.printer_name,
        label
    );

    // ── 1. Зберігаємо HTML у реєстрі для протоколу torgashka-print:// ────────
    store_print_html(&token, data.html);

    // ── 2. Формуємо URL кастомного протоколу ─────────────────────────────
    // webkit2gtk зробить запит на torgashka-print://localhost/{token}/ —
    // хендлер (зареєстрований у lib.rs) поверне HTML з реєстру за токеном.
    let url = tauri::Url::parse(&format!("torgashka-print://localhost/{}/index.html", token))
        .map_err(|e| format!("Не вдалося сформувати URL друку: {}", e))?;

    // ── 3. Створюємо WebviewWindow з HTML через кастомний протокол ───────
    let window =
        tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::CustomProtocol(url))
            .title("Torgashka — Друк A4")
            .inner_size(800.0, 600.0)
            .center()
            .decorations(false)
            .resizable(false)
            .skip_taskbar(true)
            .visible(true)
            .on_page_load(|webview, payload| {
                use tauri::webview::PageLoadEvent;
                if payload.event() == PageLoadEvent::Finished {
                    // Невелика затримка (200мс) — дочекатись остаточного layout
                    // (шрифти, зображення, CSS Grid).
                    //
                    // window.print() блокує JS-потік webkit, поки діалог друку
                    // відкритий, тому window.close() виконається одразу ПІСЛЯ
                    // завершення друку (або закриття діалогу користувачем).
                    let js =
                "setTimeout(() => { try { window.print(); } finally { window.close(); } }, 200);";
                    if let Err(e) = webview.eval(js) {
                        eprintln!("[TORGASHKA] print_html: помилка eval window.print(): {}", e);
                    }
                }
            })
            .build()
            .map_err(|e| {
                remove_print_html(&token);
                format!("Не вдалося створити вікно друку: {}", e)
            })?;

    // ── 4. Watchdog: гарантоване закриття вікна та очищення реєстру ──────
    // Якщо print-діалог не відкрився або JS window.close() не спрацював —
    // вікно закриється через 60 секунд (захист від "висячих" вікон),
    // а запис HTML буде видалено з реєстру.
    let watchdog_window = window.clone();
    let watchdog_token = token.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let _ = watchdog_window.close();
        remove_print_html(&watchdog_token);
    });

    eprintln!(
        "[TORGASHKA] ✅ print_html: вікно '{}' створено, діалог друку викликається",
        label
    );

    Ok(PrintResult {
        success: true,
        message: "HTML відправлено в системний діалог друку (webkit2gtk)".to_string(),
        bytes_sent: Some(take_print_html(&token).len()),
    })
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

// ═════════════════════════════════════════════════════════════════════════════
// 🖼️ Збереження зображення чека на диск (для дебагу/перевірки)
// ═════════════════════════════════════════════════════════════════════════════

/// Зберегти PNG-зображення чека на диск у ~/Downloads/.
///
/// Приймає чистий Base64 (без префіксу).
/// Повертає повний шлях до збереженого файлу.
#[tauri::command]
pub fn save_receipt_image(image_base64: String) -> Result<String, String> {
    use base64::Engine;

    // Декодуємо Base64
    let engine = base64::engine::general_purpose::STANDARD;
    let image_bytes = engine
        .decode(&image_base64)
        .map_err(|e| format!("Помилка декодування Base64: {}", e))?;

    // Визначаємо шлях — ~/Downloads/
    let downloads_dir = dirs_next::download_dir()
        .or_else(dirs_next::home_dir)
        .ok_or_else(|| "Не вдалося визначити домашню директорію".to_string())?;

    // Генеруємо ім'я файлу з timestamp
    let now = chrono::Local::now();
    let filename = format!("torgashka_receipt_{}.png", now.format("%Y%m%d_%H%M%S"));
    let filepath = downloads_dir.join(&filename);

    // Зберігаємо файл
    std::fs::write(&filepath, &image_bytes).map_err(|e| format!("Помилка запису файлу: {}", e))?;

    eprintln!("[TORGASHKA] ✅ Збережено чек: {:?}", filepath);

    // Повертаємо шлях
    Ok(filepath.to_string_lossy().to_string())
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
