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

/// Конвертує grayscale зображення в ESC/POS NV raster image команди.
///
/// Використовує GS v 0 (raster bit image) в нормальному режимі.
/// Пікселі з яскравістю < 128 вважаються чорними.
fn image_to_escpos_raster(img: &image::ImageBuffer<Luma<u8>, Vec<u8>>) -> Vec<u8> {
    let w = img.width();
    let h = img.height();
    let bpl = ((w + 7) / 8) as u32;
    let mut data = vec![0x1B, 0x40]; // ESC @

    for y in 0..h {
        let mut row = Vec::with_capacity(bpl as usize);
        for x in (0..w).step_by(8) {
            let mut byte = 0u8;
            for b in 0..8 {
                if x + b < w && img.get_pixel(x + b, y)[0] < 128 {
                    byte |= 1 << (7 - b);
                }
            }
            row.push(byte);
        }
        let xl = (bpl & 0xFF) as u8;
        let xh = ((bpl >> 8) & 0xFF) as u8;
        data.extend_from_slice(&[0x1D, 0x76, 0x30, 0x00, xl, xh, 0x01, 0x00]);
        data.extend_from_slice(&row);
    }

    // Подача паперу на 5 рядків перед обрізкою
    data.extend_from_slice(&[0x1B, 0x64, 0x08]); // ESC d 8 — feed 8 lines
    // Обрізка паперу (GS V m)
    data.extend_from_slice(&[0x1D, 0x56, 0x00]); // GS V 0 — cut paper
    data.push(0x0A);

    data
}

// ═════════════════════════════════════════════════════════════════════════
// 4. ДРУК РАСТРОВОГО ЗОБРАЖЕННЯ
// ═════════════════════════════════════════════════════════════════════════

/// Друк растрового зображення (PNG bytes → ESC/POS)
pub fn print_raster_image(image_data: Vec<u8>, printer_name: Option<&str>) -> Result<(), PrintError> {
    eprintln!("[RUST] print_raster_image START: data.len={}", image_data.len());

    let img = match image::load_from_memory(&image_data) {
        Ok(i) => i.to_luma8(),
        Err(e) => {
            return Err(PrintError::General(format!(
                "Не вдалося завантажити PNG: {}",
                e
            )))
        }
    };

    eprintln!("[RUST] raster image: {}x{}", img.width(), img.height());
    let _ = img.save("/tmp/kasa_raster_debug.png");

    let escpos_data = image_to_escpos_raster(&img);
    eprintln!("[RUST] ESC/POS data size: {} bytes", escpos_data.len());

    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&escpos_data)?;

    let result = print_file(temp_file.path(), printer_name, true);
    result
}

// ═════════════════════════════════════════════════════════════════════════
// 5. ДОПОМІЖНІ ФУНКЦІЇ
// ═════════════════════════════════════════════════════════════════════════

/// Відправка файлу на друк через lp
fn print_file(path: &std::path::Path, printer: Option<&str>, raw: bool) -> Result<(), PrintError> {
    let mut cmd = Command::new("lp");
    if let Some(p) = printer {
        cmd.arg("-d").arg(p);
    }
    if raw {
        cmd.arg("-o").arg("raw");
    }
    cmd.arg(path.to_str().unwrap_or(""));

    let out = cmd.output()?;

    if out.status.success() {
        Ok(())
    } else {
        Err(PrintError::LpFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

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
}
