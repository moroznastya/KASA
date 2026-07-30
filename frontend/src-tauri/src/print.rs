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
    #[error("Порт принтера не знайдено або відмовлено в доступі. Перевірте права (група dialout/lp).")]
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

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&port)?;

    file.write_all(data)?;
    file.flush()?;

    eprintln!("[RUST] ✓ Надіслано {} байтів безпосередньо на {}", data.len(), port);
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
pub fn print_escpos(data: &[u8], printer_name: Option<&str>, device_path: Option<&str>) -> Result<(), PrintError> {
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
// ─────────────────────────────────────────────────────────────────────────────

/// Конвертує grayscale зображення в ESC/POS NV raster image команди.
///
/// Пікселі з яскравістю < 128 вважаються чорними.
fn image_to_escpos_raster(img: &image::ImageBuffer<Luma<u8>, Vec<u8>>) -> Vec<u8> {
    let w = img.width();
    let h = img.height();
    let bpl = ((w + 7) / 8) as u32;  // bytes per line

    let mut data = vec![0x1B, 0x40]; // ESC @ — ініціалізація принтера

    // ── ОДИН заголовок GS v 0 для ВСЬОГО зображення ──────────────
    // Раніше було: для кожного рядка окремий заголовок (yL=0x01, yH=0x00)
    // Тепер: один заголовок з реальною висотою h
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

    // Подача паперу на 8 рядків перед обрізкою
    data.extend_from_slice(&[0x1B, 0x64, 0x08]); // ESC d 8
    // Обрізка паперу (GS V m)
    data.extend_from_slice(&[0x1D, 0x56, 0x00]); // GS V 0 — повна обрізка

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

/// Друк растрового зображення (PNG bytes → ESC/POS)
pub fn print_raster_image(
    image_data: Vec<u8>,
    printer_name: Option<&str>,
    device_path: Option<&str>,
) -> Result<(), PrintError> {
    eprintln!("[RUST] print_raster_image START: data.len={}", image_data.len());

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
    // ║  Масштабування PNG до максимальної ширини принтера             ║
    // ╚══════════════════════════════════════════════════════════════════╝
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

    eprintln!(
        "[RUST] raster image: {}x{} pixels{}",
        img.width(),
        img.height(),
        if w > PRINTER_MAX_WIDTH_DOTS {
            " (масштабовано)"
        } else {
            ""
        }
    );

    let _ = img.save("/tmp/kasa_raster_debug.png");

    let escpos_data = image_to_escpos_raster(&img);
    eprintln!(
        "[RUST] ESC/POS data: {} bytes ({} lines, bpl={})",
        escpos_data.len(),
        img.height(),
        (img.width() + 7) / 8
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
    let out = Command::new("lpstat")
        .arg("-e")
        .output()?;

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

        let err = PrintError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "файл не знайдено"));
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
        assert_eq!(gs_v0_count, 1,
            "Має бути РІВНО ОДИН заголовок GS v 0, знайдено {}", gs_v0_count);

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

    // ── Інтеграційні тести (залежать від наявності принтера) ──────────
    //
    // На розробницькій машині може бути налаштований принтер через CUPS.
    // Тести перевіряють, що print_raster_image не панікує та повертає
    // Ok або Err залежно від доступності принтера.

    #[test]
    fn test_print_raster_image_scale_oversized() {
        // PNG 512x100 → має масштабуватися до 384x75
        let png_bytes = create_test_png(512, 100, 0);
        assert!(png_bytes.len() > 100);

        let result = print_raster_image(png_bytes, None, None);
        // Може бути Ok (якщо є принтер) або Err (якщо немає)
        // Головне — не паніка
        eprintln!("print_raster_image(512x100) = {:?}", result);
    }

    #[test]
    fn test_print_raster_image_normal_size() {
        // PNG 200x50 — менше за ліміт, не масштабується
        let png_bytes = create_test_png(200, 50, 128);
        assert!(!png_bytes.is_empty());

        let result = print_raster_image(png_bytes, None, None);
        eprintln!("print_raster_image(200x50) = {:?}", result);
    }

    #[test]
    fn test_print_raster_image_empty_data() {
        // Пустий масив → має повернути помилку PNG
        let result = print_raster_image(vec![], None, None);
        match &result {
            Err(PrintError::General(msg)) => {
                assert!(msg.contains("PNG"), "Помилка має містити 'PNG': {}", msg);
            }
            other => panic!("Очікувалась PrintError::General, отримано: {:?}", other),
        }
    }
}
