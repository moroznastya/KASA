pub mod commands;

// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Модуль апаратного друку (ESC/POS через порт/CUPS)
// ─────────────────────────────────────────────────────────────────────────────
//
// Легкий модуль для друку растрових зображень на ESC/POS термопринтерах.
//
// Формування чека відбувається на frontend (React → html2canvas → PNG),
// Rust отримує готове зображення (base64 або Vec<u8>) та:
//   1. Конвертує PNG в ESC/POS NV raster image формат
//   2. Надсилає байти на принтер (прямий запис у порт або через lp)
//
// Підтримувані методи друку:
//   1. Прямий запис ESC/POS байтів на порт принтера (/dev/usb/lp*)
//   2. Fallback через системний диспетчер lp (CUPS)
//
// Підтримка копій та обрізки (додано 31.07.2026):
//   - `copies`   — кількість копій чека (налаштування print_copies з БД)
//   - `auto_cut` — чи виконувати обрізку паперу GS V 0 (auto_cut_paper з БД)
// ─────────────────────────────────────────────────────────────────────────────

use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;
use thiserror::Error;

use image::Luma;

// ═════════════════════════════════════════════════════════════════════════
// ПОМИЛКИ ДРУКУ
// ═════════════════════════════════════════════════════════════════════════

#[derive(Error, Debug)]
pub enum PrintError {
    #[error(
        "Порт принтера не знайдено або відмовлено в доступі. Перевірте права (група dialout/lp)."
    )]
    PortNotFound,

    #[error("Помилка вводу/виводу: {0}")]
    Io(#[from] std::io::Error),

    #[error("Помилка системного диспетчера lp: {0}")]
    LpFailed(String),

    #[error("Помилка: {0}")]
    General(String),
}

impl From<String> for PrintError {
    fn from(s: String) -> Self {
        PrintError::General(s)
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 1. РОБОТА З АПАРАТНИМИ ПОРТАМИ
// ═════════════════════════════════════════════════════════════════════════

/// Пошук першого доступного порту термопринтера (Ubuntu/Linux)
fn find_printer_port(custom_path: Option<&str>) -> Option<String> {
    if let Some(path) = custom_path {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    let candidates = [
        "/dev/usb/lp0",
        "/dev/usb/lp1",
        "/dev/lp0",
        "/dev/ttyUSB0",
        "/dev/ttyUSB1",
        "/dev/usb/lp2",
    ];

    candidates
        .iter()
        .find(|&&path| std::path::Path::new(path).exists())
        .map(|&s| s.to_string())
}

/// Прямий запис ESC/POS байтів у файл пристрою
pub fn write_to_printer_port(data: &[u8], device_path: Option<&str>) -> Result<(), PrintError> {
    let port = find_printer_port(device_path).ok_or(PrintError::PortNotFound)?;

    let mut file = std::fs::OpenOptions::new().write(true).open(&port)?;

    file.write_all(data)?;
    file.flush()?;

    eprintln!(
        "[RUST] ✓ Надіслано {} байтів безпосередньо на {}",
        data.len(),
        port
    );
    Ok(())
}

// ═════════════════════════════════════════════════════════════════════════
// 2. FALLBACK ЧЕРЕЗ CUPS (lp)
// ═════════════════════════════════════════════════════════════════════════

/// Відправка сирих даних через системний диспетчер (якщо порт зайнятий)
pub fn print_raw_via_lp(data: &[u8], printer_name: Option<&str>) -> Result<(), PrintError> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(data)?;

    let mut cmd = Command::new("lp");
    if let Some(p) = printer_name {
        cmd.arg("-d").arg(p);
    }
    cmd.arg("-o").arg("raw");
    cmd.arg(temp_file.path());

    let out = cmd.output()?;

    if out.status.success() {
        eprintln!("[RUST] ✓ Надруковано через lp");
        Ok(())
    } else {
        Err(PrintError::LpFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Універсальна точка входу для друку з маршрутизацією
///
/// Спершу пробує прямий запис на порт принтера.
/// Якщо не вдалося — падає на lp -o raw.
pub fn print_escpos(
    data: &[u8],
    printer_name: Option<&str>,
    device_path: Option<&str>,
) -> Result<(), PrintError> {
    match write_to_printer_port(data, device_path) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("[RUST] Прямий друк не вдався ({}). Перехід на lp...", e);
            print_raw_via_lp(data, printer_name)
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 3. КОНВЕРТАЦІЯ ЗОБРАЖЕННЯ В ESC/POS РАСТР
// ═════════════════════════════════════════════════════════════════════════
//
// Використовує команду GS v 0 (Raster Bit Image) за специфікацією ESC/POS.
// Формат: GS v 0 m xL xH yL yH [d1...dk]
//   m = 0 — нормальний режим
//   xL, xH — ширина в байтах (bytes per line)
//   yL, yH — висота в точках
//   d1...dk — дані зображення (k = x + 256*xH) * (yL + 256*yH)
//
// ВАЖЛИВО (⚠️ виправлено 30.07.2026):
//   - Раніше заголовок GS v 0 генерувався для КОЖНОГО рядка окремо
//     (yL=0x01, yH=0x00), що призводило до неефективного друку.
//   - Виправлено: ОДИН заголовок для ВСЬОГО зображення з правильною висотою.
//
// ВАЖЛИВО (⚠️ додано 31.07.2026):
//   - Функцію розділено на `image_to_escpos_raster_block` (чистий блок
//     GS v 0 + дані) та `build_multi_copy_escpos` (збірка фінального
//     потоку з копіями/обрізкою), щоб налаштування print_copies та
//     auto_cut_paper з БД реально використовувались при друці.
// ─────────────────────────────────────────────────────────────────────────────

/// Конвертує grayscale зображення в ESC/POS raster block.
///
/// Повертає ТІЛЬКИ блок `GS v 0 m xL xH yL yH [d1...dk]` без ESC @,
/// подачі паперу та обрізки. Цей блок повторюється для кожної копії.
///
/// Пікселі з яскравістю < 128 вважаються чорними.
fn image_to_escpos_raster_block(img: &image::ImageBuffer<Luma<u8>, Vec<u8>>) -> Vec<u8> {
    let w = img.width();
    let h = img.height();
    let bpl = w.div_ceil(8); // bytes per line

    let mut data = Vec::new();

    // ── ОДИН заголовок GS v 0 для ВСЬОГО зображення ──────────────
    let xl = (bpl & 0xFF) as u8;
    let xh = ((bpl >> 8) & 0xFF) as u8;
    let yl = (h & 0xFF) as u8;
    let yh = ((h >> 8) & 0xFF) as u8;

    // GS v 0 m xL xH yL yH
    data.extend_from_slice(&[0x1D, 0x76, 0x30, 0x00, xl, xh, yl, yh]);

    // ── Всі рядки зображення послідовно ──────────────────────────
    for y in 0..h {
        for x in (0..w).step_by(8) {
            let mut byte = 0u8;
            for b in 0..8 {
                if x + b < w && img.get_pixel(x + b, y)[0] < 128 {
                    byte |= 1 << (7 - b);
                }
            }
            data.push(byte);
        }
    }

    data
}

/// Конвертує grayscale зображення в повний ESC/POS потік для ОДНІЄЇ копії.
///
/// Формат: ESC @ + GS v 0 + дані + ESC d 8 + GS V 0.
/// Збережено як еталонний формат однієї копії — використовується в тестах
/// та для довідки. Основний шлях друку тепер: `image_to_escpos_raster_block`
/// + `build_multi_copy_escpos` (з підтримкою copies / auto_cut).
#[allow(dead_code)] // використовується в тестах як еталон single-copy формату
fn image_to_escpos_raster(img: &image::ImageBuffer<Luma<u8>, Vec<u8>>) -> Vec<u8> {
    let mut data = vec![0x1B, 0x40]; // ESC @ — ініціалізація принтера
    data.extend_from_slice(&image_to_escpos_raster_block(img));
    // Подача паперу на 8 рядків перед обрізкою
    data.extend_from_slice(&[0x1B, 0x64, 0x08]); // ESC d 8
                                                 // Обрізка паперу (GS V m)
    data.extend_from_slice(&[0x1D, 0x56, 0x00]); // GS V 0 — повна обрізка

    data
}

/// Збирає фінальний ESC/POS потік для друку з копіями та обрізкою.
///
/// Логіка (за вимогою налаштувань print_copies / auto_cut_paper):
///   - `raster_block` генерується ОДИН раз (GS v 0 + дані зображення)
///   - ESC @ (ініціалізація) — тільки на початку
///   - raster-блок повторюється `copies` разів
///   - ESC d 8 (подача паперу) — після КОЖНОЇ копії
///   - GS V 0 (обрізка) — лише після останньої копії, якщо `auto_cut=true`
fn build_multi_copy_escpos(raster_block: &[u8], copies: u32, auto_cut: bool) -> Vec<u8> {
    // Обмеження copies: мінімум 1, максимум 100 (захист від copies=0 та DoS/OOM)
    let copies = copies.clamp(1, 100);

    let mut data = vec![0x1B, 0x40]; // ESC @ — ініціалізація принтера (один раз)

    for _ in 0..copies {
        // Raster-блок (GS v 0 + дані зображення) — повторюється для кожної копії
        data.extend_from_slice(raster_block);
        // Подача паперу після КОЖНОЇ копії
        data.extend_from_slice(&[0x1B, 0x64, 0x08]); // ESC d 8
    }

    // Обрізка паперу — лише після останньої копії, якщо увімкнено
    if auto_cut {
        data.extend_from_slice(&[0x1D, 0x56, 0x00]); // GS V 0 — повна обрізка
    }

    data
}

// ═════════════════════════════════════════════════════════════════════════
// 4. ДРУК РАСТРОВОГО ЗОБРАЖЕННЯ
// ═════════════════════════════════════════════════════════════════════════

/// Максимальна ширина друку для типових термопринтерів (в dots).
///
/// Якщо PNG ширший — автоматично масштабуємо до цієї ширини.
///
/// Співвідношення:
///   58mm папір → 384 dots (48mm друку) — Xprinter/POS-58
///   80mm папір → 576 dots (72mm друку) — Epson TM-T88
///   76mm папір → 512 dots (64mm друку)
const PRINTER_MAX_WIDTH_DOTS: u32 = 384;

/// Обчислення цільового розміру растру (dots) за фізичними розмірами етикетки.
///
/// Формула: `dots = мм * dpi / 25.4` (1 дюйм = 25.4 мм)
///   - `target_w` обмежується `PRINTER_MAX_WIDTH_DOTS` (384 для 58мм принтера)
///   - мінімум 1 dot (захист від `resize(0, ...)` → panic)
///
/// Приклад: 58×40мм @ 203dpi → (384, 320) dots = 48×40мм фізично
/// (ширина 48мм — фізичне обмеження 58мм принтера, висота ТОЧНО 40мм).
fn compute_label_target_size(width_mm: f64, height_mm: f64, dpi: u32) -> (u32, u32) {
    let dpi = dpi.max(1) as f64;
    let target_w = ((width_mm * dpi / 25.4).round() as u32).clamp(1, PRINTER_MAX_WIDTH_DOTS);
    let target_h = ((height_mm * dpi / 25.4).round() as u32).max(1);
    (target_w, target_h)
}

/// Друк растрового зображення (PNG bytes → ESC/POS)
///
/// # Параметри
/// - `image_data`   — байти PNG зображення
/// - `printer_name` — назва принтера для CUPS (опціонально)
/// - `device_path`  — шлях до пристрою (опціонально, напр. /dev/usb/lp0)
/// - `copies`       — кількість копій (default 1, мінімум 1)
/// - `auto_cut`     — виконувати обрізку паперу GS V 0 (default true)
/// - `width_mm`     — фізична ширина етикетки в мм (опціонально; термо-етикетки)
/// - `height_mm`    — фізична висота етикетки в мм (опціонально; термо-етикетки)
/// - `dpi`          — роздільна здатність принтера, dots/inch (опціонально; default 203)
///
/// # Масштабування
/// Якщо задані `width_mm`/`height_mm` (термо-етикетки) — ТОЧНИЙ друк за мм:
///   - `target_w = min(round(width_mm * dpi / 25.4), PRINTER_MAX_WIDTH_DOTS)`
///   - `target_h = round(height_mm * dpi / 25.4)`
///   - resize ЗАВЖДИ до (target_w, target_h) через `FilterType::Lanczos3`
///   - Приклад: 58×40мм @ 203dpi → (384, 320) dots = 48×40мм фізично
///     (ширина 48мм — фізичне обмеження 58мм принтера, висота ТОЧНО 40мм)
///
/// Якщо розміри НЕ задані (чеки) — стара логіка: масштаб до 384 dots
/// лише коли PNG ширший за `PRINTER_MAX_WIDTH_DOTS`.
///
/// # Зворотна сумісність
/// Виклики без `copies`/`auto_cut` працюють як раніше (copies=1, auto_cut=true) —
/// командний шар підставляє дефолти через `Option::unwrap_or`.
#[allow(clippy::too_many_arguments)] // API стабільне на етапі 0; рефакторинг у struct — наступні етапи
pub fn print_raster_image(
    image_data: Vec<u8>,
    printer_name: Option<&str>,
    device_path: Option<&str>,
    copies: u32,
    auto_cut: bool,
    width_mm: Option<f64>,
    height_mm: Option<f64>,
    dpi: Option<u32>,
) -> Result<(), PrintError> {
    let dpi = dpi.unwrap_or(203);
    eprintln!(
        "[RUST] print_raster_image START: data.len={}, copies={}, auto_cut={}, width_mm={:?}, height_mm={:?}, dpi={}",
        image_data.len(),
        copies,
        auto_cut,
        width_mm,
        height_mm,
        dpi
    );

    // Завантажуємо PNG та конвертуємо в grayscale
    let mut img = match image::load_from_memory(&image_data) {
        Ok(i) => i.to_luma8(),
        Err(e) => {
            return Err(PrintError::General(format!(
                "Не вдалося завантажити PNG: {}",
                e
            )))
        }
    };

    let w = img.width();
    let h = img.height();

    // ╔══════════════════════════════════════════════════════════════════╗
    // ║  Масштабування PNG                                              ║
    // ╚══════════════════════════════════════════════════════════════════╝
    //
    // РЕЖИМ 1 — Точний друк термо-етикеток (width_mm/height_mm задані):
    //   Цільовий розмір у dots розраховується З ФІЗИЧНИХ ММ та роздільної
    //   здатності принтера, resize виконується ЗАВЖДИ (Lanczos3):
    //     target_w = min(round(width_mm * dpi / 25.4), PRINTER_MAX_WIDTH_DOTS)
    //     target_h = round(height_mm * dpi / 25.4)
    //   Приклад: 58×40мм @ 203dpi → (384, 320) dots = 48×40мм фізично
    //   (висота ТОЧНО 40мм, ширина 48мм — фізичне обмеження 58мм принтера).
    //
    // РЕЖИМ 2 — Чеки (width_mm/height_mm НЕ задані):
    //   Стара логіка — масштабуємо до 384 dots лише якщо PNG ширший.
    match (width_mm, height_mm) {
        (Some(w_mm), Some(h_mm)) => {
            let (target_w, target_h) = compute_label_target_size(w_mm, h_mm, dpi);
            eprintln!(
                "[RUST] 📐 Етикетка {}x{}mm @ {}dpi → {}x{} dots (Lanczos3, точний друк)",
                w_mm, h_mm, dpi, target_w, target_h
            );
            img = image::imageops::resize(
                &img,
                target_w,
                target_h,
                image::imageops::FilterType::Lanczos3,
            );
        }
        _ => {
            // html2canvas може генерувати PNG шириною ~438px (58mm * 96DPI * 2),
            // а принтер 58mm підтримує максимум 384 dots. Масштабуємо зі
            // збереженням пропорцій, використовуючи Lanczos3 для чіткого тексту.
            if w > PRINTER_MAX_WIDTH_DOTS {
                let new_h = (h as f64 * PRINTER_MAX_WIDTH_DOTS as f64 / w as f64) as u32;
                eprintln!(
                    "[RUST] ⚠️  Масштабування з {}x{} до {}x{} (Lanczos3)",
                    w, h, PRINTER_MAX_WIDTH_DOTS, new_h
                );
                img = image::imageops::resize(
                    &img,
                    PRINTER_MAX_WIDTH_DOTS,
                    new_h,
                    image::imageops::FilterType::Lanczos3,
                );
            }
        }
    }

    eprintln!(
        "[RUST] raster image: {}x{} pixels{}",
        img.width(),
        img.height(),
        if w != img.width() || h != img.height() {
            " (масштабовано)"
        } else {
            ""
        }
    );

    let _ = img.save("/tmp/kasa_raster_debug.png");

    // ╔══════════════════════════════════════════════════════════════════╗
    // ║  Генерація raster-блоку ОДИН раз + збірка копій/обрізки        ║
    // ╚══════════════════════════════════════════════════════════════════╝
    // raster-дані конвертуємо один раз (image_to_escpos_raster_block),
    // потім build_multi_copy_escpos повторює блок `copies` разів,
    // додає ESC d 8 після кожної копії та GS V 0 (якщо auto_cut).
    let raster_block = image_to_escpos_raster_block(&img);
    eprintln!(
        "[RUST] raster block: {} bytes ({} lines, bpl={})",
        raster_block.len(),
        img.height(),
        img.width().div_ceil(8)
    );

    let escpos_data = build_multi_copy_escpos(&raster_block, copies, auto_cut);
    eprintln!(
        "[RUST] ESC/POS final: {} bytes, copies={}, auto_cut={}",
        escpos_data.len(),
        copies,
        auto_cut
    );

    // ╔══════════════════════════════════════════════════════════════════╗
    // ║  Маршрутизація: прямий порт → lp fallback                      ║
    // ╚══════════════════════════════════════════════════════════════════╝
    print_escpos(&escpos_data, printer_name, device_path)
}

// ═════════════════════════════════════════════════════════════════════════
// 5. ДОПОМІЖНІ ФУНКЦІЇ
// ═════════════════════════════════════════════════════════════════════════

/// Отримати список доступних принтерів
pub fn get_printers() -> Result<Vec<String>, PrintError> {
    let out = Command::new("lpstat").arg("-e").output()?;

    if !out.status.success() {
        return Err(PrintError::General("lpstat не знайдено".to_string()));
    }

    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

// ═════════════════════════════════════════════════════════════════════════
// ТЕСТИ
// ═════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_port() {
        let port = find_printer_port(None);
        eprintln!("Printer port: {:?}", port);
    }

    #[test]
    fn test_print_error_display() {
        let err = PrintError::PortNotFound;
        assert!(err.to_string().contains("Порт принтера не знайдено"));

        let err = PrintError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "файл не знайдено",
        ));
        assert!(err.to_string().contains("файл не знайдено"));

        let err = PrintError::LpFailed("lp: printer not found".to_string());
        assert!(err.to_string().contains("lp: printer not found"));

        let err = PrintError::General("test error".to_string());
        assert_eq!(err.to_string(), "Помилка: test error");
    }

    #[test]
    fn test_image_to_escpos_raster_one_header() {
        let img = image::ImageBuffer::from_pixel(100, 50, Luma([0u8]));
        let data = image_to_escpos_raster(&img);

        // Рівно 1 заголовок GS v 0
        let gs_v0_count = data.windows(3).filter(|w| w == &[0x1D, 0x76, 0x30]).count();
        assert_eq!(
            gs_v0_count, 1,
            "Має бути РІВНО ОДИН заголовок GS v 0, знайдено {}",
            gs_v0_count
        );

        let header_len: usize = 10;
        let footer_len: usize = 6;
        let bpl: usize = ((100 + 7) / 8) as usize;
        let raster_data_len: usize = bpl * 50;

        assert_eq!(data.len(), header_len + raster_data_len + footer_len);

        // Позиція заголовка
        assert_eq!(data[0], 0x1B);
        assert_eq!(data[1], 0x40);
        assert_eq!(data[2], 0x1D);
        assert_eq!(data[3], 0x76);
        assert_eq!(data[4], 0x30);
        assert_eq!(data[6], 13, "xL для 100px");
        assert_eq!(data[7], 0, "xH=0");
        assert_eq!(data[8], 50, "yL=50");
        assert_eq!(data[9], 0, "yH=0");
    }

    #[test]
    fn test_image_to_escpos_raster_last_pixel_included() {
        let img = image::ImageBuffer::from_pixel(1, 1, Luma([0u8]));
        let data = image_to_escpos_raster(&img);

        assert_eq!(data.len(), 17, "1x1: 2+8+1+3+3=17B");
        assert_eq!(data[10], 0x80, "Перший піксель → 0x80");

        // Footer
        assert_eq!(data[11], 0x1B);
        assert_eq!(data[12], 0x64);
        assert_eq!(data[13], 0x08);
        assert_eq!(data[14], 0x1D);
        assert_eq!(data[15], 0x56);
        assert_eq!(data[16], 0x00);
    }

    #[test]
    fn test_image_to_escpos_raster_odd_width() {
        let img = image::ImageBuffer::from_pixel(9, 1, Luma([0u8]));
        let data = image_to_escpos_raster(&img);

        assert_eq!(data[6], 2, "xL для 9px");
        assert_eq!(data[10], 0xFF, "Перший байт: всі 8 бітів чорні");
        assert_eq!(data[11], 0x80, "Другий байт: тільки 1 біт");
    }

    #[test]
    fn test_image_to_escpos_raster_white_image() {
        let img = image::ImageBuffer::from_pixel(16, 1, Luma([255u8]));
        let data = image_to_escpos_raster(&img);

        assert_eq!(data[10], 0x00, "Білий → 0x00");
        assert_eq!(data[11], 0x00, "Білий → 0x00");
    }

    #[test]
    fn test_image_to_escpos_raster_alternating() {
        let mut img = image::ImageBuffer::from_pixel(8, 1, Luma([255u8]));
        img.put_pixel(3, 0, Luma([0u8]));

        let data = image_to_escpos_raster(&img);
        assert_eq!(data[10], 0x10, "Піксель 3 → 0x10");
    }

    // ── Тести багатокопійної збірки (build_multi_copy_escpos) ──────────

    #[test]
    fn test_build_multi_copy_escpos_single_copy_auto_cut() {
        // copies=1, auto_cut=true → ESC @ + block + ESC d 8 + GS V 0
        // (еквівалент старого image_to_escpos_raster)
        let block = vec![0xAA, 0xBB];
        let data = build_multi_copy_escpos(&block, 1, true);

        assert_eq!(
            data,
            vec![0x1B, 0x40, 0xAA, 0xBB, 0x1B, 0x64, 0x08, 0x1D, 0x56, 0x00],
            "1 копія + обрізка: ESC@ + block + feed + cut"
        );
    }

    #[test]
    fn test_build_multi_copy_escpos_two_copies_auto_cut() {
        // copies=2, auto_cut=true → ESC @ + (block + feed) * 2 + cut
        let block = vec![0xAA, 0xBB];
        let data = build_multi_copy_escpos(&block, 2, true);

        assert_eq!(
            data,
            vec![
                0x1B, 0x40, // ESC @ — один раз на початку
                0xAA, 0xBB, 0x1B, 0x64, 0x08, // копія 1 + подача
                0xAA, 0xBB, 0x1B, 0x64, 0x08, // копія 2 + подача
                0x1D, 0x56, 0x00, // GS V 0 — обрізка після останньої
            ],
            "2 копії + обрізка: ESC@ + (block+feed)*2 + cut"
        );
    }

    #[test]
    fn test_build_multi_copy_escpos_two_copies_no_cut() {
        // copies=2, auto_cut=false → ESC @ + (block + feed) * 2, БЕЗ GS V 0
        let block = vec![0xAA, 0xBB];
        let data = build_multi_copy_escpos(&block, 2, false);

        assert_eq!(
            data,
            vec![
                0x1B, 0x40, // ESC @ — один раз на початку
                0xAA, 0xBB, 0x1B, 0x64, 0x08, // копія 1 + подача
                0xAA, 0xBB, 0x1B, 0x64,
                0x08, // копія 2 + подача
                      // НЕМАЄ GS V 0 — обрізка вимкнена
            ],
            "2 копії без обрізки: ESC@ + (block+feed)*2, без cut"
        );
    }

    #[test]
    fn test_build_multi_copy_escpos_zero_copies_clamped() {
        // copies=0 → примусово 1 копія (захист від помилок конфігурації)
        let block = vec![0xAA];
        let data = build_multi_copy_escpos(&block, 0, true);

        assert_eq!(
            data,
            vec![0x1B, 0x40, 0xAA, 0x1B, 0x64, 0x08, 0x1D, 0x56, 0x00],
            "copies=0 має бути клампнуто до 1 копії"
        );
    }

    #[test]
    fn test_build_multi_copy_escpos_copies_clamped_range() {
        // Перевірка обмеження copies діапазоном [1..100] — захист від DoS/OOM:
        //   copies=0 → 1 (мінімум), copies=1 → 1, copies=50 → 50, copies=1000 → 100 (максимум)
        // Блок [0xAA] — унікальний маркер: жоден службовий байт (ESC @, ESC d 8,
        // GS V 0) не містить 0xAA, тому кількість 0xAA у даних = кількість копій.
        let block = vec![0xAA];

        let cases = [
            (0u32, 1usize),      // менше мінімуму → кламп до 1
            (1u32, 1usize),      // норма
            (50u32, 50usize),    // норма в межах ліміту
            (1000u32, 100usize), // більше максимуму → кламп до 100 (захист від OOM)
        ];

        for (input, expected) in cases {
            let data = build_multi_copy_escpos(&block, input, true);
            let actual = data.iter().filter(|&&b| b == 0xAA).count();
            assert_eq!(
                actual, expected,
                "copies={} має бути клампнуто до {} (знайдено {} блоків)",
                input, expected, actual
            );
        }
    }

    #[test]
    fn test_build_multi_copy_escpos_three_copies_no_cut() {
        // copies=3, auto_cut=false → 3 блоки, 3 подачі, без обрізки
        let block = vec![0x01];
        let data = build_multi_copy_escpos(&block, 3, false);

        // ESC @ (2) + 3 * (block(1) + feed(3)) = 2 + 12 = 14
        assert_eq!(data.len(), 14, "3 копії без обрізки: 2 + 3*(1+3) = 14B");
        assert_eq!(data[0], 0x1B);
        assert_eq!(data[1], 0x40);
        // Перша копія
        assert_eq!(data[2], 0x01);
        assert_eq!(data[3], 0x1B);
        assert_eq!(data[4], 0x64);
        assert_eq!(data[5], 0x08);
        // Друга копія
        assert_eq!(data[6], 0x01);
        assert_eq!(data[9], 0x08);
        // Третя копія
        assert_eq!(data[10], 0x01);
        assert_eq!(data[13], 0x08);
        // Немає GS V 0 в кінці
        assert_ne!(data[13], 0x1D, "Останній байт не має бути частиною GS V 0");
    }

    // ── Допоміжна функція: створює PNG байти ──────────────────────────

    fn create_test_png(width: u32, height: u32, pixel_value: u8) -> Vec<u8> {
        use image::codecs::png::PngEncoder;
        use image::ImageEncoder;

        let mut png_bytes: Vec<u8> = Vec::new();
        let encoder = PngEncoder::new(&mut png_bytes);
        let buf: Vec<u8> = vec![pixel_value; (width * height) as usize];

        encoder
            .write_image(&buf, width, height, image::ExtendedColorType::L8)
            .expect("PNG encoding should work");

        png_bytes
    }

    // ── Тести точного масштабування за мм (термо-етикетки) ──────────────

    #[test]
    fn test_compute_label_target_size_58x40mm() {
        // 58×40мм @ 203dpi:
        //   target_w = min(round(58*203/25.4), 384) = min(464, 384) = 384
        //   target_h = round(40*203/25.4) = round(319.68) = 320
        assert_eq!(compute_label_target_size(58.0, 40.0, 203), (384, 320));
    }

    #[test]
    fn test_compute_label_target_size_40x30mm() {
        // 40×30мм @ 203dpi:
        //   target_w = round(40*203/25.4) = round(319.68) = 320
        //   target_h = round(30*203/25.4) = round(239.76) = 240
        assert_eq!(compute_label_target_size(40.0, 30.0, 203), (320, 240));
    }

    #[test]
    fn test_compute_label_target_size_30x20mm() {
        // 30×20мм @ 203dpi:
        //   target_w = round(30*203/25.4) = round(239.76) = 240
        //   target_h = round(20*203/25.4) = round(159.84) = 160
        assert_eq!(compute_label_target_size(30.0, 20.0, 203), (240, 160));
    }

    #[test]
    fn test_compute_label_target_size_wide_label_clamped() {
        // Дуже широка етикетка (100мм) → ширина ОБМЕЖУЄТЬСЯ 384 dots
        // (фізичне обмеження 58мм принтера), висота рахується точно
        let (tw, th) = compute_label_target_size(100.0, 40.0, 203);
        assert_eq!(tw, 384);
        assert_eq!(th, 320);
    }

    #[test]
    fn test_compute_label_target_size_tiny_min_clamp() {
        // Крихітна етикетка → мінімум 1 dot (захист від resize(0,...) panic)
        let (tw, th) = compute_label_target_size(0.01, 0.01, 203);
        assert!(tw >= 1 && th >= 1, "tw={}, th={}", tw, th);
    }

    // ── Інтеграційні тести (залежать від наявності принтера) ──────────
    //
    // На розробницькій машині може бути налаштований принтер через CUPS.
    // Тести перевіряють, що print_raster_image не панікує та повертає
    // Ok або Err залежно від доступності принтера.
    // Сигнатури оновлено: copies=1, auto_cut=true (зворотна сумісність).

    #[test]
    fn test_print_raster_image_scale_oversized() {
        // PNG 512x100 → має масштабуватися до 384x75
        let png_bytes = create_test_png(512, 100, 0);
        assert!(png_bytes.len() > 100);

        let result = print_raster_image(png_bytes, None, None, 1, true, None, None, None);
        // Може бути Ok (якщо є принтер) або Err (якщо немає)
        // Головне — не паніка
        eprintln!("print_raster_image(512x100) = {:?}", result);
    }

    #[test]
    fn test_print_raster_image_normal_size() {
        // PNG 200x50 — менше за ліміт, не масштабується
        let png_bytes = create_test_png(200, 50, 128);
        assert!(!png_bytes.is_empty());

        let result = print_raster_image(png_bytes, None, None, 1, true, None, None, None);
        eprintln!("print_raster_image(200x50) = {:?}", result);
    }

    #[test]
    fn test_print_raster_image_multi_copy() {
        // PNG 100x20, 2 копії, без обрізки — ланцюг має відпрацювати
        let png_bytes = create_test_png(100, 20, 0);
        let result = print_raster_image(png_bytes, None, None, 2, false, None, None, None);
        eprintln!(
            "print_raster_image(100x20, copies=2, auto_cut=false) = {:?}",
            result
        );
    }

    #[test]
    fn test_print_raster_image_empty_data() {
        // Пустий масив → має повернути помилку PNG
        let result = print_raster_image(vec![], None, None, 1, true, None, None, None);
        match &result {
            Err(PrintError::General(msg)) => {
                assert!(msg.contains("PNG"), "Помилка має містити 'PNG': {}", msg);
            }
            other => panic!("Очікувалась PrintError::General, отримано: {:?}", other),
        }
    }
}
