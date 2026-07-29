// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Модуль апаратного друку (Нативний ESC/POS)
// ─────────────────────────────────────────────────────────────────────────────
//
// Підтримувані режими друку:
//   1. Прямий запис ESC/POS байтів на порт принтера (/dev/usb/lp*)
//   2. Fallback через системний диспетчер lp (CUPS)
//   3. Графічний друк (растеризація тексту → ESC/POS зображення)
//   4. HTML друк (Chrome headless → PNG → ESC/POS)
//
// Сумісність із принтером у китайському режимі:
//   Деякі принтери (Xprinter, POS-58) мають апаратний режим
//   "Chinese character: Yes", який ігнорує `ESC t`.
//   Щоб вийти з нього — надсилаємо FS . (0x1C, 0x2E) після ініціалізації.
//
// Кодування тексту:
//   - Для нативного ESC/POS друку (EscPosBuilder, print_receipt_native):
//     текст конвертується з UTF-8 у Windows-1251 (WPC1251, code page 73)
//   - Windows-1251 підтримує всі українські символи:
//     І=0xB2, і=0xB3, ї=0xBF, Ї=0xAE, є=0xBA, Є=0xBB, ґ=0xB4, Ґ=0xA5
//   - Після вимкнення китайського режиму активується WPC1251 (індекс 73)
//   - Для графічного друку (растеризація): використовується FreeType/ab_glyph
//     зі шрифтами, що підтримують кирилицю
//
// ─────────────────────────────────────────────────────────────────────────────

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use tempfile::NamedTempFile;
use thiserror::Error;
use encoding_rs::WINDOWS_1251;

// ── Крейтові залежності для графічного друку ────────────────────────────
use image::Luma;
use image::io::Reader as ImageReader;
use imageproc::drawing::draw_text_mut;
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};

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
//
// Термопринтери в Linux зазвичай доступні як:
//   /dev/usb/lp0   — USB принтер (основний)
//   /dev/usb/lp1   — другий USB принтер
//   /dev/lp0       — LPT (паралельний порт)
//   /dev/ttyUSB0   — USB-to-Serial адаптер
//
// Якщо жоден порт не знайдено — використовуємо lp як fallback.

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
    // Використовуємо tempfile: файл автоматично видаляється
    // при виході з області видимості (навіть при помилках)
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
// 3. НАТИВНИЙ ESC/POS БІЛДЕР (без графіки)
// ═════════════════════════════════════════════════════════════════════════
//
// Простий builder для формування ESC/POS команд.
// Для складних чеків (зі штрих-кодами тощо) використовуйте escpos::EscposBuilder.
//
// Кодування:
//   - Текст автоматично конвертується з UTF-8 у Windows-1251 (WPC1251)
//   - Команда FS . (0x1C, 0x2E) вимикає китайський режим
//   - Команда ESC t 73 активує WPC1251 (Windows-1251 Cyrillic)
//
// Windows-1251 підтримує всі українські символи:
//   І=0xB2, і=0xB3, ї=0xBF, Ї=0xAE, є=0xBA, Є=0xBB, ґ=0xB4, Ґ=0xA5

const ESC: u8 = 0x1B;
const FS: u8 = 0x1C;
const GS: u8 = 0x1D;
const LF: u8 = 0x0A;

/// Конвертує UTF-8 рядок у Windows-1251 (WPC1251).
///
/// Windows-1251 підтримує всі кириличні символи, включаючи українські:
///   І=0xB2, і=0xB3, ї=0xBF, Ї=0xAE, є=0xBA, Є=0xBB, ґ=0xB4, Ґ=0xA5
///
/// Символи, яких немає (наприклад, €), замінюються на '?' (0x3F).
fn to_windows_1251(text: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(text.len());
    for c in text.chars() {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        let (encoded, _, had_errors) = WINDOWS_1251.encode(s);
        if had_errors {
            result.push(0x3F); // '?' — заміна для символів не в Windows-1251
        } else {
            result.extend_from_slice(&encoded);
        }
    }
    result
}

/// Утиліта для формування команд принтера без використання графіки
pub struct EscPosBuilder {
    buffer: Vec<u8>,
}

impl EscPosBuilder {
    /// Створити новий builder з ініціалізацією:
    ///   ESC @  — ініціалізація
    ///   FS .   — вимкнення китайського режиму
    ///   ESC t 73 — WPC1251 (Windows-1251 Cyrillic)
    pub fn new() -> Self {
        let buf = vec![
            ESC, 0x40,  // ESC @ — ініціалізація
            FS,  0x2E,  // FS .  — вимкнути китайський режим
            ESC, 0x74, 73, // ESC t 73 — WPC1251 (Windows-1251 Cyrillic)
        ];
        Self { buffer: buf }
    }

    /// Додати текст (автоматична конвертація UTF-8 → Windows-1251)
    pub fn add_text(&mut self, text: &str) -> &mut Self {
        let cp1251 = to_windows_1251(text);
        self.buffer.extend_from_slice(&cp1251);
        self
    }

    /// Додати сирі байти (без конвертації)
    pub fn add_raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buffer.extend_from_slice(bytes);
        self
    }

    /// Перевести рядок (LF)
    pub fn feed_line(&mut self) -> &mut Self {
        self.buffer.push(LF);
        self
    }

    /// Увімкнути/вимкнути жирний шрифт (ESC E n)
    pub fn set_bold(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x45, if enabled { 1 } else { 0 }]);
        self
    }

    /// Вирівнювання: 0 — ліворуч, 1 — центр, 2 — праворуч (ESC a n)
    pub fn set_alignment(&mut self, align: u8) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x61, align]);
        self
    }

    /// Обрізати папір (GS V m) — 0: повна, 1: часткова
    pub fn cut_paper(&mut self, partial: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[GS, 0x56, if partial { 0x01 } else { 0x00 }]);
        self
    }

    /// Подати n рядків (ESC d n)
    pub fn feed(&mut self, lines: u8) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x64, lines]);
        self
    }

    /// Отримати готові ESC/POS байти
    pub fn build(self) -> Vec<u8> {
        self.buffer
    }
}

/// Швидкий друк чека з рядків (нативний ESC/POS, без графіки)
///
/// Використовує EscPosBuilder для формування простого чека.
/// Для складних чеків зі штрих-кодами — використовуйте
/// `escpos::build_receipt_escpos()` + `print::print_escpos()`.
pub fn print_receipt_native(lines: Vec<String>, printer_name: Option<&str>, device_path: Option<&str>) -> Result<(), PrintError> {
    let mut builder = EscPosBuilder::new();

    // Заголовок
    builder
        .set_alignment(1)
        .set_bold(true)
        .add_text("Kasa POS\n")
        .set_bold(false)
        .add_text("--------------------------\n")
        .set_alignment(0);

    // Тіло чека
    for line in &lines {
        builder.add_text(line).feed_line();
    }

    // Підвал та відрізка
    builder
        .set_alignment(1)
        .add_text("--------------------------\n")
        .add_text("Дякуємо за покупку!\n")
        .feed(8)
        .cut_paper(false);

    let bytes = builder.build();
    print_escpos(&bytes, printer_name, device_path)
}

// ═════════════════════════════════════════════════════════════════════════
// 4. ГРАФІЧНИЙ ДРУК (растеризація тексту → ESC/POS зображення)
// ═════════════════════════════════════════════════════════════════════════

const RECEIPT_WIDTH: u32 = 756;
const MARGIN_LEFT: i32 = 3;
const MARGIN_TOP: i32 = 1;
const LINE_HEIGHT: i32 = 32;
const FONT_SIZE: f32 = 22.0;
const MAX_TEXT_WIDTH: i32 = RECEIPT_WIDTH as i32 - MARGIN_LEFT * 2 - 4;

// ─── Типи рядків для рендерингу ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum LineType { Normal, Header, Bold, Right, SolidSeparator, DashedSeparator }

#[derive(Debug, Clone)]
struct RenderLine { text: String, line_type: LineType }

// ─── Парсинг тексту в рядки рендерингу ─────────────────────────────────

fn parse_to_render_lines(lines: &[String]) -> Vec<RenderLine> {
    let mut rls: Vec<RenderLine> = Vec::new();
    for line in lines {
        let t = line.trim();
        if t == "__DASHED_SEP__" {
            rls.push(RenderLine { text: String::new(), line_type: LineType::DashedSeparator });
            continue;
        }
        if t == "__SOLID_SEP__" {
            rls.push(RenderLine { text: String::new(), line_type: LineType::SolidSeparator });
            continue;
        }
        if let Some(rest) = t.strip_prefix("__CENTER__") {
            let s = rest.trim().to_string();
            if !s.is_empty() { rls.push(RenderLine { text: s, line_type: LineType::Header }); }
            continue;
        }
        if let Some(rest) = t.strip_prefix("__RIGHT__") {
            let s = rest.trim().to_string();
            if !s.is_empty() { rls.push(RenderLine { text: s, line_type: LineType::Right }); }
            continue;
        }
        if let Some(rest) = t.strip_prefix("__BOLD__") {
            let s = rest.trim().to_string();
            if !s.is_empty() { rls.push(RenderLine { text: s, line_type: LineType::Bold }); }
            continue;
        }
        if !t.is_empty() {
            rls.push(RenderLine { text: t.to_string(), line_type: LineType::Normal });
        }
    }
    rls
}

// ─── Завантаження шрифту ───────────────────────────────────────────────

fn load_font() -> Result<FontVec, PrintError> {
    let paths = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            let data = std::fs::read(p)
                .map_err(|e| PrintError::General(format!("Помилка читання {}: {}", p, e)))?;
            let font = FontVec::try_from_vec(data)
                .map_err(|e| PrintError::General(format!("{:?}", e)))?;
            return Ok(font);
        }
    }
    Err(PrintError::General("Не знайдено шрифту з кирилицею".to_string()))
}

// ─── Вимірювання тексту ────────────────────────────────────────────────

fn measure_text_width(text: &str, font: &FontVec, scale: PxScale) -> i32 {
    let fr = font.as_scaled(scale);
    let mut tw = 0f32;
    for c in text.chars() {
        tw += fr.h_advance(fr.glyph_id(c));
    }
    tw as i32
}

fn wrap_text(text: &str, font: &FontVec, scale: PxScale, max_w: i32) -> Vec<String> {
    let mut wrapped: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() { continue; }
        if measure_text_width(t, font, scale) <= max_w {
            wrapped.push(t.to_string());
            continue;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        let mut cl = String::new();
        let mut cw = 0i32;
        for w in words {
            let ww = measure_text_width(&format!("{} ", w), font, scale);
            if cw + ww > max_w && !cl.is_empty() {
                wrapped.push(cl.trim_end().to_string());
                cl = format!("{} ", w);
                cw = ww;
            } else {
                cl.push_str(w);
                cl.push(' ');
                cw += ww;
            }
        }
        if !cl.trim().is_empty() {
            wrapped.push(cl.trim_end().to_string());
        }
    }
    wrapped
}

// ─── Малювання роздільників ────────────────────────────────────────────

fn draw_separator(img: &mut image::ImageBuffer<Luma<u8>, Vec<u8>>, y: i32, w: u32, dashed: bool) {
    let wi = w as i32;
    let x1 = MARGIN_LEFT;
    let x2 = wi - MARGIN_LEFT;
    if dashed {
        let mut x = x1;
        while x < x2 {
            if x + 2 <= x2 {
                for dx in 0..2 {
                    img.put_pixel((x + dx) as u32, y as u32, Luma([0u8]));
                }
            }
            x += 4;
        }
    } else {
        for x in x1..x2 {
            img.put_pixel(x as u32, y as u32, Luma([0u8]));
        }
    }
}

// ─── Рендеринг зображення чека ─────────────────────────────────────────

fn render_receipt_image(
    text: &str, font: &FontVec, w: u32, font_size: f32, lh: i32,
) -> Result<image::ImageBuffer<Luma<u8>, Vec<u8>>, PrintError> {
    let scale = PxScale::from(font_size);
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let rls = parse_to_render_lines(&lines);

    let mut expanded: Vec<RenderLine> = Vec::new();
    for rl in &rls {
        match rl.line_type {
            LineType::Normal | LineType::Header | LineType::Bold | LineType::Right => {
                for wl in wrap_text(&rl.text, font, scale, MAX_TEXT_WIDTH) {
                    expanded.push(RenderLine { text: wl, line_type: rl.line_type.clone() });
                }
            }
            _ => { expanded.push(rl.clone()); }
        }
    }

    let ch = expanded.len() as i32 * lh;
    let h = (ch + MARGIN_TOP + 4).max(50) as u32;
    let mut img = image::ImageBuffer::from_pixel(w, h, Luma([255u8]));
    let mut y = MARGIN_TOP;

    for rl in &expanded {
        match rl.line_type {
            LineType::Normal => {
                draw_text_mut(&mut img, Luma([0u8]), MARGIN_LEFT, y, scale, font, &rl.text);
            }
            LineType::Header | LineType::Bold => {
                let tw = measure_text_width(&rl.text, font, scale);
                let x = ((w as i32 - tw) / 2).max(0);
                draw_text_mut(&mut img, Luma([0u8]), x, y, scale, font, &rl.text);
            }
            LineType::Right => {
                let tw = measure_text_width(&rl.text, font, scale);
                let x = (w as i32 - MARGIN_LEFT - tw).max(MARGIN_LEFT);
                draw_text_mut(&mut img, Luma([0u8]), x, y, scale, font, &rl.text);
            }
            LineType::SolidSeparator => { draw_separator(&mut img, y + lh / 2, w, false); }
            LineType::DashedSeparator => { draw_separator(&mut img, y + lh / 2, w, true); }
        }
        y += lh;
    }

    let ah = (y + 2) as u32;
    Ok(image::imageops::crop(&mut img, 0, 0, w, ah.min(h)).to_image())
}

// ─── Конвертація зображення в ESC/POS растр ────────────────────────────

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

// ─── Друк тексту через растеризацію ────────────────────────────────────

/// Друк чека текстом через графічний режим ESC/POS
pub fn print_receipt_text(text: &str, printer_name: Option<&str>) -> Result<(), PrintError> {
    eprintln!("[RUST] print_receipt_text START: text.len={}, printer={:?}", text.len(), printer_name);

    let font = load_font()?;
    let img = render_receipt_image(text, &font, RECEIPT_WIDTH, FONT_SIZE, LINE_HEIGHT)?;

    eprintln!("[RUST] image rendered: {}x{}", img.width(), img.height());
    let _ = img.save("/tmp/kasa_print_debug.png");

    let escpos_data = image_to_escpos_raster(&img);
    eprintln!("[RUST] ESC/POS data size: {} bytes", escpos_data.len());

    // Використовуємо tempfile замість UUID
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&escpos_data)?;

    eprintln!("[RUST] calling print_file...");
    let result = print_file(temp_file.path(), printer_name, true);
    eprintln!("[RUST] print_file result: {:?}", result);
    result
}

// ═════════════════════════════════════════════════════════════════════════
// 5. HTML ДРУК (Chrome headless → PNG → ESC/POS)
// ═════════════════════════════════════════════════════════════════════════

// ─── Перевірка, чи встановлено інструмент ──────────────────────────────

fn is_tool_installed(tool: &str) -> bool {
    Command::new("which")
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ─── Chrome headless → PNG ─────────────────────────────────────────────

fn chrome_html_to_png(html: &str, output_path: &PathBuf) -> Result<(), PrintError> {
    let chrome = if is_tool_installed("google-chrome") {
        "google-chrome"
    } else if is_tool_installed("chromium-browser") {
        "chromium-browser"
    } else if is_tool_installed("chromium") {
        "chromium"
    } else {
        return Err(PrintError::General(
            "Chrome/Chromium не знайдено. Встановіть chromium-browser.".to_string(),
        ));
    };

    let html_path = output_path.with_extension("html");
    
    // Замінюємо фіксовану ширину в mm на 100% (для сумісності)
    let html_fixed = html
        .replace(r#"width: 48mm"#, r#"width: 100%"#)
        .replace(r#"width:48mm"#, r#"width:100%"#)
        .replace(r#"width: 58mm"#, r#"width: 100%"#)
        .replace(r#"width:58mm"#, r#"width:100%"#)
        .replace(r#"max-width: 48mm"#, r#"max-width: 100%"#)
        .replace(r#"max-width:48mm"#, r#"max-width:100%"#);
    
    // Додаємо CSS для збільшення шрифтів і прибирання відступів
    let enhanced_html = html_fixed.replace(
        "</head>",
        r#"<style>
  html, body {
    margin: 0;
    padding: 0;
    width: 100%;
    overflow: hidden;
  }
  @page {
    margin: 0;
    padding: 0;
  }
</style>
</head>"#,
    );
    
    std::fs::write(&html_path, &enhanced_html)?;

    let html_url = format!("file://{}", html_path.display());

    let result = Command::new(chrome)
        .arg("--headless")
        .arg("--disable-gpu")
        .arg("--no-sandbox")
        .arg(format!("--screenshot={}", output_path.display()))
        .arg("--window-size=1008,10000")
        .arg("--hide-scrollbars")
        .arg("--default-background-color=FFFFFFFF")
        .arg(&html_url)
        .output()?;

    let _ = std::fs::remove_file(&html_path);

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(PrintError::General(format!(
            "Chrome помилка: {}",
            stderr.trim()
        )));
    }

    if !output_path.exists() {
        return Err(PrintError::General(
            "Chrome не створив файл зображення".to_string(),
        ));
    }

    // Обрізаємо зайві білі пікселі знизу та зверху
    if let Ok(mut img) = ImageReader::open(output_path)
        .map_err(|e| PrintError::General(format!("Не вдалося відкрити PNG для обрізки: {}", e)))
        .and_then(|reader| reader.decode().map_err(|e| PrintError::General(format!("Не вдалося декодувати PNG: {}", e))))
        .map(|img| img.to_luma8())
    {
        let h = img.height();
        let w = img.width();
        
        // Шукаємо останній рядок знизу, де є хоча б один темний піксель
        let mut last_non_white_row = h as i32 - 1;
        for y in (0..h).rev() {
            let mut has_dark = false;
            for x in 0..w {
                if img.get_pixel(x, y)[0] < 240 {
                    has_dark = true;
                    break;
                }
            }
            if has_dark {
                last_non_white_row = y as i32;
                break;
            }
        }
        
        // Шукаємо перший рядок зверху, де є хоча б один темний піксель
        let mut first_non_white_row = 0i32;
        for y in 0..h {
            let mut has_dark = false;
            for x in 0..w {
                if img.get_pixel(x, y)[0] < 240 {
                    has_dark = true;
                    break;
                }
            }
            if has_dark {
                first_non_white_row = y as i32;
                break;
            }
        }
        
        // Відступити 1px зверху (мінімальний відступ)
        let crop_y = first_non_white_row.max(0) as u32;
        let crop_height = (last_non_white_row + 5).max(100) as u32;
        let crop_h = (crop_height - crop_y).max(100);
        
        if crop_y > 0 || crop_h < h {
            let cropped = image::imageops::crop(&mut img, 0, crop_y, w, crop_h).to_image();
            cropped.save(output_path)
                .map_err(|e| PrintError::General(format!("Не вдалося зберегти обрізаний PNG: {}", e)))?;
        }
    }

    Ok(())
}

// ─── Парсинг HTML в рядки (fallback, якщо Chrome недоступний) ──────────

fn parse_html_to_lines(html: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();

    let mut alignment: &str = "left";
    let mut bold = false;

    let mut in_flex = false;
    let mut flex_columns: Vec<String> = Vec::new();
    let mut flex_alignments: Vec<&str> = Vec::new();
    let mut flex_span_text = String::new();

    while i < len {
        let c = chars[i];

        if c == '<' && i + 3 < len && chars[i + 1] == '!' && chars[i + 2] == '-' && chars[i + 3] == '-' {
            let mut j = i + 4;
            while j + 2 < len {
                if chars[j] == '-' && chars[j + 1] == '-' && chars[j + 2] == '>' {
                    i = j + 3;
                    break;
                }
                j += 1;
            }
            if j + 2 >= len { i = len; }
            continue;
        }

        if c == '<' {
            let mut tag = String::new();
            let mut j = i + 1;
            while j < len && chars[j] != '>' {
                tag.push(chars[j]);
                j += 1;
            }
            if j < len {
                let tag_lower = tag.to_lowercase();
                let tag_trimmed = tag_lower.trim();

                if tag_trimmed.starts_with("hr") || tag_trimmed.starts_with("/hr") {
                    flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                    flush_text(&mut result, &mut current, alignment, bold);
                    if tag_lower.contains("dashed") {
                        result.push("__DASHED_SEP__".to_string());
                    } else {
                        result.push("__SOLID_SEP__".to_string());
                    }
                    i = j + 1;
                    continue;
                }

                if tag_trimmed.starts_with('/') {
                    let tag_name = tag_trimmed
                        .trim_start_matches('/')
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    match tag_name {
                        "p" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                        | "body" | "html" => {
                            flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                            flush_text(&mut result, &mut current, alignment, bold);
                        }
                        "div" => {
                            if in_flex {
                                flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                                in_flex = false;
                            } else {
                                flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                                flush_text(&mut result, &mut current, alignment, bold);
                            }
                        }
                        "strong" | "b" => {
                            flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                            flush_text(&mut result, &mut current, alignment, bold);
                            bold = false;
                        }
                        "span" => {
                            if in_flex {
                                flex_span_text = flex_span_text.trim().to_string();
                                if !flex_span_text.is_empty() {
                                    flex_columns.push(flex_span_text.clone());
                                    flex_alignments.push(alignment);
                                    flex_span_text.clear();
                                }
                                alignment = "left";
                            }
                        }
                        _ => {}
                    }
                    i = j + 1;
                    continue;
                }

                if tag_trimmed.starts_with("br") {
                    if in_flex {
                        flex_span_text.push(' ');
                    } else {
                        flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                        flush_text(&mut result, &mut current, alignment, bold);
                    }
                    i = j + 1;
                    continue;
                }

                if tag_trimmed.starts_with("p ") || tag_trimmed == "p" || tag_trimmed.starts_with("tr") {
                    flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                    flush_text(&mut result, &mut current, alignment, bold);
                    i = j + 1;
                    continue;
                }

                if tag_trimmed.starts_with("strong") || tag_trimmed.starts_with("b ") || tag_trimmed == "b" {
                    bold = true;
                    i = j + 1;
                    continue;
                }

                if tag_trimmed.starts_with('h') && tag_trimmed.len() > 1 {
                    let second = tag_trimmed.chars().nth(1).unwrap_or(' ');
                    if second >= '1' && second <= '6' {
                        flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                        flush_text(&mut result, &mut current, alignment, bold);
                        alignment = "center";
                        i = j + 1;
                        continue;
                    }
                }

                if tag_trimmed.starts_with("div") || tag_trimmed.starts_with("span")
                    || tag_trimmed.starts_with("td") || tag_trimmed.starts_with("th")
                    || tag_trimmed.starts_with("li")
                {
                    if tag_trimmed.starts_with("div") && tag_lower.contains("display: flex") {
                        flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                        flush_text(&mut result, &mut current, alignment, bold);
                        in_flex = true;
                        flex_columns.clear();
                        flex_alignments.clear();
                        alignment = "left";
                    } else if tag_trimmed.starts_with("span") {
                        if in_flex {
                            flex_span_text = flex_span_text.trim().to_string();
                            if !flex_span_text.is_empty() {
                                flex_columns.push(flex_span_text.clone());
                                flex_alignments.push(alignment);
                                flex_span_text.clear();
                            }
                            if tag_lower.contains("text-align:") {
                                if tag_lower.contains("center") {
                                    alignment = "center";
                                } else if tag_lower.contains("right") {
                                    alignment = "right";
                                } else {
                                    alignment = "left";
                                }
                            } else {
                                alignment = "left";
                            }
                        } else if tag_lower.contains("text-align:") {
                            if tag_lower.contains("center") {
                                flush_text(&mut result, &mut current, alignment, bold);
                                alignment = "center";
                            } else if tag_lower.contains("right") {
                                flush_text(&mut result, &mut current, alignment, bold);
                                alignment = "right";
                            }
                        }
                    } else if tag_trimmed.starts_with("div") && tag_lower.contains("text-align:") {
                        if tag_lower.contains("center") {
                            flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                            flush_text(&mut result, &mut current, alignment, bold);
                            alignment = "center";
                        } else if tag_lower.contains("right") {
                            flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
                            flush_text(&mut result, &mut current, alignment, bold);
                            alignment = "right";
                        }
                    }
                    i = j + 1;
                    continue;
                }

                i = j + 1;
                continue;
            }
        }

        if in_flex {
            flex_span_text.push(c);
        } else {
            current.push(c);
        }
        i += 1;
    }

    flush_flex(&mut flex_columns, &mut flex_alignments, &mut flex_span_text, &mut result);
    if !current.trim().is_empty() {
        flush_text(&mut result, &mut current, "left", bold);
    }

    let mut cleaned: Vec<String> = Vec::new();
    for line in &result {
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() { continue; }
        let mut compact = String::new();
        let mut prev_space = false;
        for ch in trimmed.chars() {
            if ch == ' ' {
                if !prev_space { compact.push(' '); prev_space = true; }
            } else {
                compact.push(ch);
                prev_space = false;
            }
        }
        cleaned.push(compact);
    }
    cleaned
}

fn flush_flex(
    flex_columns: &mut Vec<String>,
    flex_alignments: &mut Vec<&str>,
    flex_span_text: &mut String,
    result: &mut Vec<String>,
) {
    let last_text = flex_span_text.trim().to_string();
    flex_span_text.clear();
    if !last_text.is_empty() {
        flex_columns.push(last_text);
        flex_alignments.push("left");
    }

    if flex_columns.is_empty() { return; }

    fn char_count(s: &str) -> usize { s.chars().count() }

    let mut line = String::new();
    let total_width = 32;

    if flex_columns.len() == 2 {
        let left = &flex_columns[0];
        let right = &flex_columns[1];
        let left_chars = char_count(left);
        let right_chars = char_count(right);
        let spaces = (total_width as i32 - left_chars as i32 - right_chars as i32).max(1) as usize;
        line = format!("{}{}{}", left, " ".repeat(spaces), right);
    } else if flex_columns.len() == 4 {
        let c0 = &flex_columns[0];
        let c1 = &flex_columns[1];
        let c2 = &flex_columns[2];
        let c3 = &flex_columns[3];

        let c0_chars = char_count(c0);
        let c1_chars = char_count(c1);
        let c2_chars = char_count(c2);
        let c3_chars = char_count(c3);

        let w3 = c3_chars.max(5);
        let w2 = c2_chars.max(5);
        let w1 = c1_chars.max(1);
        let w0 = (total_width as i32 - w1 as i32 - w2 as i32 - w3 as i32 - 3).max(1) as usize;

        let name = if c0_chars > w0 {
            let truncated: String = c0.chars().take(w0.max(3) - 2).collect();
            format!("{}..", truncated)
        } else {
            c0.clone()
        };

        let name_chars = char_count(&name);
        line = format!(
            "{}{}{}{}{}{}{}",
            &name,
            " ".repeat(w0 - name_chars),
            " ".repeat(w1 - c1_chars), &c1,
            " ".repeat(w2 - c2_chars), &c2,
            &c3
        );
    } else {
        let mut parts = Vec::new();
        for (i, col) in flex_columns.iter().enumerate() {
            let align = flex_alignments.get(i).copied().unwrap_or("left");
            match align {
                "right" => parts.push(format!("{:>8}", col)),
                "center" => parts.push(format!("{:^8}", col)),
                _ => parts.push(col.clone()),
            }
        }
        line = parts.join(" ");
    }

    if !line.is_empty() { result.push(line); }
    flex_columns.clear();
    flex_alignments.clear();
}

fn flush_text(result: &mut Vec<String>, current: &mut String, alignment: &str, bold: bool) {
    let text = current.trim().to_string();
    current.clear();
    if text.is_empty() { return; }
    let prefix = match alignment {
        "center" => "__CENTER__",
        "right" => "__RIGHT__",
        _ => if bold { "__BOLD__" } else { "" },
    };
    if !prefix.is_empty() {
        result.push(format!("{}{}", prefix, text));
    } else {
        result.push(text);
    }
}

// ─── Друк HTML-чека ────────────────────────────────────────────────────

/// Друк HTML-чека (Chrome headless → PNG → ESC/POS, або fallback до текстового парсера)
pub fn print_receipt_html(html: &str, printer_name: Option<&str>) -> Result<(), PrintError> {
    let debug_text = {
        let truncated: String = html.chars().take(500).collect();
        format!(
            "print_receipt_html called! html_len={}, printer={:?}\nHTML (first 500 chars):\n{}",
            html.len(),
            printer_name,
            truncated
        )
    };
    let _ = std::fs::write("/tmp/kasa_print_debug.txt", &debug_text);

    eprintln!(
        "[RUST] print_receipt_html START: html_len={}, printer={:?}",
        html.len(),
        printer_name
    );

    // ─── Спроба: Chrome headless ──────────────────────────────────
    let temp_dir = std::env::temp_dir();
    let png_path = temp_dir.join(format!("kasa_receipt_{}.png", uuid::Uuid::new_v4()));

    let chrome_result = chrome_html_to_png(html, &png_path);

    match chrome_result {
        Ok(()) => {
            eprintln!("[RUST] Chrome rendered OK: {:?}", png_path);

            let img = ImageReader::open(&png_path)
                .map_err(|e| PrintError::General(format!("Не вдалося відкрити PNG: {}", e)))?
                .decode()
                .map_err(|e| PrintError::General(format!("Не вдалося декодувати PNG: {}", e)))?
                .to_luma8();

            eprintln!("[RUST] Chrome raw image: {}x{}", img.width(), img.height());
            let _ = img.save("/tmp/kasa_print_chrome.png");

            let escpos_data = image_to_escpos_raster(&img);
            eprintln!("[RUST] ESC/POS data size: {} bytes", escpos_data.len());

            let mut temp_file = NamedTempFile::new()?;
            temp_file.write_all(&escpos_data)?;

            let result = print_file(temp_file.path(), printer_name, true);
            let _ = std::fs::remove_file(&png_path);

            match &result {
                Ok(_) => eprintln!("[RUST] print_receipt_html (Chrome) OK"),
                Err(e) => eprintln!("[RUST] print_receipt_html (Chrome) ERROR: {}", e),
            }
            return result;
        }
        Err(e) => {
            eprintln!(
                "[RUST] Chrome not available ({}), falling back to text parser",
                e
            );
            let _ = std::fs::write(
                "/tmp/kasa_chrome_fallback.txt",
                &format!("Chrome error: {}\nFalling back to text parser", e),
            );
        }
    }

    // ─── Спроба 2: Вбудований парсер HTML → текст ──────────────────
    eprintln!("[RUST] Using text parser fallback");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let lines = parse_html_to_lines(html);
        let text = lines.join("\n");
        text
    }));

    match result {
        Ok(text) => {
            eprintln!("[RUST] parse_html_to_lines OK: text.len={}", text.len());
            print_receipt_text(&text, printer_name)
        }
        Err(_) => {
            let _ = std::fs::write(
                "/tmp/kasa_print_error.txt",
                format!("PANIC in parse_html_to_lines\nhtml_len={}", html.len()),
            );
            eprintln!("[RUST] PANIC in parse_html_to_lines");
            Err(PrintError::General(
                "Помилка парсингу HTML (паніка)".to_string(),
            ))
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 6. ДРУК HTML-ДОКУМЕНТА
// ═════════════════════════════════════════════════════════════════════════

/// Друк HTML-документа (через CUPS/lp або wkhtmltopdf)
pub fn print_html(html: &str, printer_name: Option<&str>) -> Result<(), PrintError> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(html.as_bytes())?;

    let result = if is_tool_installed("wkhtmltopdf") {
        let pdf_path = std::env::temp_dir().join(format!("kasa_print_{}.pdf", uuid::Uuid::new_v4()));
        let pdf_result = Command::new("wkhtmltopdf")
            .arg(temp_file.path())
            .arg(&pdf_path)
            .output()?;

        if pdf_result.status.success() {
            let r = print_file(&pdf_path, printer_name, false);
            let _ = std::fs::remove_file(&pdf_path);
            r
        } else {
            let stderr = String::from_utf8_lossy(&pdf_result.stderr);
            if stderr.contains("Failed to load") || stderr.contains("Error") {
                print_file(temp_file.path(), printer_name, false)
            } else {
                Err(PrintError::General(format!(
                    "Помилка wkhtmltopdf: {}",
                    stderr.trim()
                )))
            }
        }
    } else {
        print_file(temp_file.path(), printer_name, false)
    };

    result
}

// ═════════════════════════════════════════════════════════════════════════
// 7. ДРУК РАСТРОВОГО ЗОБРАЖЕННЯ
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
// 8. ДОПОМІЖНІ ФУНКЦІЇ
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

/// Попередній перегляд у браузері
pub fn print_preview(html: &str) -> Result<(), PrintError> {
    let p = std::env::temp_dir().join("kasa_print_preview.html");
    let mut f = std::fs::File::create(&p)?;
    f.write_all(html.as_bytes())?;
    drop(f);

    Command::new("xdg-open")
        .arg(&p)
        .output()?;

    Ok(())
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
    fn test_windows_1251_conversion() {
        // Перевіряємо, що кирилиця конвертується в Windows-1251
        let text = "Привет"; // all basic cyrillic chars are in Windows-1251
        let cp1251 = to_windows_1251(text);
        // Windows-1251: П=0xCF, р=0xF0, и=0xE8, в=0xE2, е=0xE5, т=0xF2
        assert_eq!(cp1251[0], 0xCF, "П byte mismatch");
        assert_eq!(cp1251[1], 0xF0, "р byte mismatch");
        assert_eq!(cp1251[2], 0xE8, "и byte mismatch");
        assert_eq!(cp1251[3], 0xE2, "в byte mismatch");
        assert_eq!(cp1251[4], 0xE5, "е byte mismatch");
        assert_eq!(cp1251[5], 0xF2, "т byte mismatch");
        eprintln!("Windows-1251 bytes: {:02X?}", cp1251);
    }

    #[test]
    fn test_windows_1251_ukrainian_chars_preserved() {
        // Windows-1251 ПІДТРИМУЄ всі українські символи — вони НЕ заміняються на '?'
        let text = "Іван їсть євро ґанок";
        let cp1251 = to_windows_1251(text);
        // 'І' (U+0406) -> 0xB2 in Windows-1251
        assert_eq!(cp1251[0], 0xB2, "І in Windows-1251 should be 0xB2, NOT '?'");
        eprintln!("'Іван їсть євро ґанок' в Windows-1251: {:02X?}", cp1251);
    }

    #[test]
    fn test_escpos_builder_with_init_sequence() {
        let mut b = EscPosBuilder::new();
        // new() вже містить ESC @ + FS . + ESC t 73
        b.set_alignment(1)
            .set_bold(true)
            .add_text("Kasa POS\n")
            .set_bold(false)
            .add_text("Дякуємо!\n")
            .cut_paper(false);

        let data = b.build();
        assert!(!data.is_empty());

        // Перші 2 байти: ESC @ — ініціалізація
        assert_eq!(data[0], ESC, "Missing ESC @ init");
        assert_eq!(data[1], 0x40);
        // Байти 2-3: FS . — вимкнення китайського режиму
        assert_eq!(data[2], FS, "Missing FS .");
        assert_eq!(data[3], 0x2E);
        // Байти 4-6: ESC t 73 — WPC1251
        assert_eq!(data[4], ESC, "Missing ESC t");
        assert_eq!(data[5], 0x74);
        assert_eq!(data[6], 73, "Code page should be 73 (WPC1251)");

        // "Дя" має бути в Windows-1251: Д=0xC4, я=0xEF
        let found = data.windows(2).any(|w| w == &[0xC4, 0xFF]);
        assert!(found, "Не знайдено 'Дя' в Windows-1251. Bytes: {:02X?}", &data[..data.len().min(30)]);
    }

    #[test]
    fn test_windows_1251_in_print_builder() {
        // Новий тест: підтверджує, що EscPosBuilder використовує WPC1251
        let mut b = EscPosBuilder::new();

        // Додаємо українські символи
        b.add_text("Іван їсть євро ґанок");

        let data = b.build();
        assert!(!data.is_empty());

        // Перевіряємо ініціалізацію з WPC1251
        assert_eq!(data[0], ESC, "Missing ESC @ init");
        assert_eq!(data[4], ESC, "Missing ESC t");
        assert_eq!(data[5], 0x74);
        assert_eq!(data[6], 73, "Code page should be 73 (WPC1251)");

        // Перевіряємо, що українські символи збережено (не '?')
        // 'І' = 0xB2 в Windows-1251
        assert!(data[7..].contains(&0xB2), "І (0xB2) має бути в даних, але не знайдено");

        eprintln!("EscPosBuilder with Ukrainian chars: {:02X?}", &data[7..]);
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
