// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — ESC/POS генератор
// ─────────────────────────────────────────────────────────────────────────────
// Будує набір байтів за специфікацією ESC/POS для термопринтерів.
// Використовується для прямого друку чеків без посередників (Chrome, CUPS).
//
// Формат команд:
//   ESC = 0x1B, FS = 0x1C, GS = 0x1D, LF = 0x0A
//
// Сумісність із принтером у китайському режимі:
//   Деякі принтери (Xprinter, POS-58) мають апаратний режим
//   "Chinese character: Yes", який ігнорує `ESC t`.
//   Щоб вийти з нього — надсилаємо FS . (0x1C, 0x2E) після ініціалізації.
//
// Кодування:
//   - Усі текстові рядки конвертуються з UTF-8 у Windows-1251
//   - Windows-1251 підтримує всі кириличні символи, включаючи
//     українські І(0xB2), і(0xB3), ї(0xBF), Ї(0xAE), є(0xBA), Є(0xBB),
//     ґ(0xB4), Ґ(0xA5)
//   - Символів, відсутніх у Windows-1251, практично немає
//     (крім € та деяких рідкісних)
//   - Після вимкнення китайського режиму активується WPC1251 (індекс 73)
//
// Підтримувані принтери:
//   - Будь-які POS-термопринтери з ESC/POS (Epson, Star, Xprinter, тощо)
//   - Спеціально перевірено на Xprinter з "Chinese character: Yes"
// ─────────────────────────────────────────────────────────────────────────────

use encoding_rs::WINDOWS_1251;

const ESC: u8 = 0x1B;
const FS: u8 = 0x1C;
const GS: u8 = 0x1D;
const LF: u8 = 0x0A;

// ── Конвертація UTF-8 → Windows-1251 ──────────────────────────────────

/// Конвертує UTF-8 рядок у Windows-1251 (Cyrillic).
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

// ── ESC/POS Builder ─────────────────────────────────────────────────────

/// Будує ESC/POS команди крок за кроком
pub struct EscposBuilder {
    buf: Vec<u8>,
}

impl EscposBuilder {
    /// Створити новий builder
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Отримати готові байти
    pub fn build(self) -> Vec<u8> {
        self.buf
    }

    // ── Ініціалізація ─────────────────────────────────────────

    /// Ініціалізувати принтер (ESC @)
    pub fn init(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x40]);
        self
    }

    /// Вимкнути китайський режим (FS .)
    ///
    /// Деякі принтери мають апаратний режим "Chinese character: Yes"
    /// який ігнорує `ESC t`. FS . вимикає цей режим.
    pub fn disable_chinese_mode(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[FS, 0x2E]);
        self
    }

    /// Вибрати кодову сторінку (ESC t n)
    ///
    /// Для WPC1251 (Cyrillic) використовуйте n = 73.
    ///
    /// Поширені значення n:
    ///   11 (0x0B) — CP855 (Cyrillic #1)
    ///   14 (0x0E) — CP866 (Cyrillic #2, DOS)
    ///   17 (0x11) — PC866 (альтернативний)
    ///   73        — WPC1251 (Windows-1251 Cyrillic) — для Xprinter
    pub fn set_code_page(&mut self, page: u8) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x74, page]);
        self
    }

    // ── Текст (автоматична конвертація UTF-8 → Windows-1251) ─

    /// Додати рядок тексту з LF в кінці (UTF-8 → Windows-1251)
    pub fn text_line(&mut self, text: &str) -> &mut Self {
        let cp1251 = to_windows_1251(text);
        self.buf.extend_from_slice(&cp1251);
        self.buf.push(LF);
        self
    }

    /// Додати текст без LF (UTF-8 → Windows-1251)
    pub fn text(&mut self, text: &str) -> &mut Self {
        let cp1251 = to_windows_1251(text);
        self.buf.extend_from_slice(&cp1251);
        self
    }

    /// Додати символ (без конвертації — сирий байт)
    pub fn raw_byte(&mut self, byte: u8) -> &mut Self {
        self.buf.push(byte);
        self
    }

    /// Додати сирі байти (без конвертації)
    pub fn raw_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    /// Перевести рядок (LF)
    pub fn newline(&mut self) -> &mut Self {
        self.buf.push(LF);
        self
    }

    /// Пройти n рядків (ESC d n)
    pub fn feed(&mut self, lines: u8) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x64, lines]);
        self
    }

    // ── Вирівнювання ─────────────────────────────────────────

    /// Вирівнювання ліворуч (ESC a 0)
    pub fn align_left(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x61, 0x00]);
        self
    }

    /// Вирівнювання по центру (ESC a 1)
    pub fn align_center(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x61, 0x01]);
        self
    }

    /// Вирівнювання праворуч (ESC a 2)
    pub fn align_right(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x61, 0x02]);
        self
    }

    // ── Стилі тексту ─────────────────────────────────────────

    /// Увімкнути жирний (ESC E 1)
    pub fn bold_on(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x45, 0x01]);
        self
    }

    /// Вимкнути жирний (ESC E 0)
    pub fn bold_off(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x45, 0x00]);
        self
    }

    /// Подвійна висота (ESC ! 0x10)
    pub fn double_height_on(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x21, 0x10]);
        self
    }

    /// Звичайна висота (ESC ! 0x00)
    pub fn double_height_off(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x21, 0x00]);
        self
    }

    // ── Роздільники ──────────────────────────────────────────

    /// Суцільна лінія (─ * ширина)
    pub fn solid_line(&mut self, width: u8) -> &mut Self {
        for _ in 0..width {
            self.buf.push(0x2D); // '-'
        }
        self.buf.push(LF);
        self
    }

    /// Пунктирна лінія (- - -)
    pub fn dashed_line(&mut self, width: u8) -> &mut Self {
        for i in 0..width {
            if i % 3 == 0 {
                self.buf.push(0x2D); // '-'
            } else {
                self.buf.push(0x20); // ' '
            }
        }
        self.buf.push(LF);
        self
    }

    // ── Штрих-код (Code128) ──────────────────────────────────

    /// Надрукувати штрих-код Code128 (GS k 73 n d1...dn)
    /// Дані штрих-коду — ASCII, конвертація не потрібна
    pub fn barcode_code128(&mut self, data: &str) -> &mut Self {
        let bytes = data.as_bytes();
        let n = bytes.len() as u8;

        // GS k 73 (Code128)
        self.buf.extend_from_slice(&[GS, 0x6B, 0x4D, n]);
        self.buf.extend_from_slice(bytes);
        self.buf.push(LF);
        self
    }

    /// Надрукувати штрих-код EAN13 (GS k 67 n d1...d12)
    pub fn barcode_ean13(&mut self, data: &str) -> &mut Self {
        let bytes = data.as_bytes();
        let n = bytes.len() as u8;

        // GS k 67 (EAN13)
        self.buf.extend_from_slice(&[GS, 0x6B, 0x43, n]);
        self.buf.extend_from_slice(bytes);
        self.buf.push(LF);
        self
    }

    // ── Обрізка паперу ───────────────────────────────────────

    /// Обрізати папір (GS V m)
    /// m = 0 — повна обрізка, m = 1 — часткова
    pub fn cut(&mut self, partial: bool) -> &mut Self {
        if partial {
            self.buf.extend_from_slice(&[GS, 0x56, 0x01]);
        } else {
            self.buf.extend_from_slice(&[GS, 0x56, 0x00]);
        }
        self
    }

    // ── Відкриття грошової скриньки ──────────────────────────

    /// Відкрити грошову скриньку (ESC p m t)
    /// m = 0 (штифт 2), t = время в * 2 мс
    pub fn open_drawer(&mut self, pin: u8, time_msec: u8) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, 0x70, pin, time_msec]);
        self
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// Функції високого рівня для формування чеку
// ─────────────────────────────────────────────────────────────────────────────

/// Структура даних для друку чека
#[derive(serde::Deserialize, Debug)]
pub struct ReceiptPrintRequest {
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
}

#[derive(serde::Deserialize, Debug)]
pub struct ReceiptItemData {
    pub barcode: Option<String>,
    pub name: String,
    pub quantity: f64,
    pub price: f64,
    pub total: f64,
}

/// Безпечне обрізання рядка по символах (UTF-8 safe)
fn truncate_utf8(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}..", truncated)
    } else {
        s.to_string()
    }
}

/// Розраховує оптимальну кількість порожніх рядків перед обрізкою паперу.
///
/// Формула: max(3, min(20, items_count / 3 + 2))
///
/// - Для порожнього або малого чека (0 товарів): 3 рядки (мінімум)
/// - Для середнього чека (10 товарів): 10/3 + 2 ≈ 5 рядків
/// - Для великого чека (50 товарів): 50/3 + 2 ≈ 18 рядків
/// - Максимум: 20 рядків
///
/// # Аргументи
/// * `items_count` - кількість товарів у чеку
///
/// # Повертає
/// Кількість порожніх рядків (ESC d n) для подачі перед обрізкою.
pub fn calculate_receipt_height(items_count: usize) -> u8 {
    let base = (items_count / 3) + 2;
    if base > 20 {
        20
    } else if base < 3 {
        3
    } else {
        base as u8
    }
}

/// Сформувати повний набір ESC/POS байтів для чека
pub fn build_receipt_escpos(data: &ReceiptPrintRequest) -> Vec<u8> {
    let mut e = EscposBuilder::new();
    let line_width: u8 = 32; // символів для 58мм стрічки

    // ── Ініціалізація ───────────────────────────────────────
    // ESC @       — ініціалізація принтера
    // FS .        — вимкнути китайський режим (для Xprinter)
    // ESC t 73    — WPC1251 (Windows-1251 Cyrillic)
    e.init();
    e.disable_chinese_mode();
    e.set_code_page(73); // WPC1251 (Windows-1251 Cyrillic)

    // ── ЧЕК № ───────────────────────────────────────────────
    e.text_line(&format!("ЧЕК № {}", data.receipt_number));
    e.newline();

    // ── Магазин ─────────────────────────────────────────────
    e.bold_on();
    e.text_line(&data.shop_name);
    e.bold_off();
    if !data.shop_address.is_empty() {
        e.text_line(&data.shop_address);
    }
    if !data.tax_id.is_empty() {
        e.text_line(&format!("ЄДРПОУ: {}", data.tax_id));
    }
    e.newline();

    // ── Дата / касир ────────────────────────────────────────
    e.text_line(&format!("{}  {}", data.date, data.time));
    if !data.cashier.is_empty() {
        e.text_line(&format!("Касир: {}", data.cashier));
    }
    e.newline();

    // ── Роздільник ──────────────────────────────────────────
    e.dashed_line(line_width);

    // ── Товари ──────────────────────────────────────────────
    for item in &data.items {
        // Штрих-код (якщо є)
        if let Some(barcode) = &item.barcode {
            if !barcode.is_empty() {
                e.text_line(&format!("[{}]", barcode));
            }
        }

        // Рядок товару: назва + кількість + ціна + сума
        let max_name_chars: usize = 18;
        let name = truncate_utf8(&item.name, max_name_chars);

        let qty_str = format_qty(item.quantity);
        let price_str = format_price(item.price);
        let total_str = format_price(item.total);

        let line = format!("{:<18} {:>4} {:>6} {:>6}", name, qty_str, price_str, total_str);
        e.text_line(&line);
    }

    // ── Роздільник ──────────────────────────────────────────
    e.dashed_line(line_width);

    // ── Підсумок ────────────────────────────────────────────
    e.newline();
    e.text(&format!("СУМА:"));
    e.align_right();
    e.text_line(&format!("{} грн", format_price(data.total)));
    e.align_left();
    e.newline();

    // ── Оплата ─────────────────────────────────────────────
    e.text_line(&data.payment_method);
    e.text_line(&format!("Оплачено: {} грн", format_price(data.paid)));
    if data.change > 0.0 {
        e.text_line(&format!("Решта: {} грн", format_price(data.change)));
    }
    e.newline();

    // ── Роздільник ─────────────────────────────────────────
    e.dashed_line(line_width);

    // ── Футер ──────────────────────────────────────────────
    e.align_center();
    e.text_line("Дякуємо за покупку!");
    if let Some(footer) = &data.footer {
        if !footer.is_empty() {
            e.newline();
            e.text_line(footer);
        }
    }

    // ── Відступи та обрізка ────────────────────────────────
    e.align_left();
    // Фіксована подача 8 рядків перед обрізкою
    e.feed(8); // feed(8) — гарантує, що текст не обріжеться
    e.cut(true); // часткова обрізка

    e.build()
}

// ── Допоміжні функції форматування ─────────────────────

fn format_qty(n: f64) -> String {
    if n == n.trunc() {
        format!("{}", n as i64)
    } else {
        format!("{:.2}", n)
    }
}

fn format_price(n: f64) -> String {
    format!("{:.2}", n)
}

// ── Тести ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_1251_conversion() {
        // Перевіряємо, що кирилиця конвертується в Windows-1251
        let text = "ТЕСТ 001";
        let cp1251 = to_windows_1251(text);
        // Windows-1251: Т=0xD2, Е=0xC5, С=0xD1, Т=0xD2
        assert_eq!(cp1251[0], 0xD2, "Т byte mismatch");
        assert_eq!(cp1251[1], 0xC5, "Е byte mismatch");
        assert_eq!(cp1251[2], 0xD1, "С byte mismatch");
        assert_eq!(cp1251[3], 0xD2, "Т byte mismatch");
        eprintln!("Windows-1251 bytes: {:02X?}", cp1251);
    }

    #[test]
    fn test_code_page_command() {
        let mut e = EscposBuilder::new();
        e.init();
        e.disable_chinese_mode();
        e.set_code_page(73); // WPC1251
        let data = e.build();

        // Після ESC @ має бути FS . потім ESC t 73
        assert_eq!(data[0], ESC, "Expected ESC @");
        assert_eq!(data[1], 0x40);
        assert_eq!(data[2], FS, "Expected FS .");
        assert_eq!(data[3], 0x2E);
        assert_eq!(data[4], ESC, "Expected ESC t");
        assert_eq!(data[5], 0x74);
        assert_eq!(data[6], 73, "Code page should be 73 (WPC1251)");
    }

    #[test]
    fn test_text_is_windows_1251() {
        let mut e = EscposBuilder::new();
        e.text_line("Привет");
        let data = e.build();

        // Windows-1251: П=0xCF, р=0xF0, и=0xE8, в=0xE2, е=0xE5, т=0xF2
        assert_eq!(data[0], 0xCF, "П byte mismatch");
        assert_eq!(data[1], 0xF0, "р byte mismatch");
        assert_eq!(data[2], 0xE8, "и byte mismatch");
        assert_eq!(data[3], 0xE2, "в byte mismatch");
        assert_eq!(data[4], 0xE5, "е byte mismatch");
        assert_eq!(data[5], 0xF2, "т byte mismatch");
        assert_eq!(data[6], LF);
    }

    #[test]
    fn test_basic_escpos() {
        let mut e = EscposBuilder::new();
        e.init();
        e.disable_chinese_mode();
        e.set_code_page(73);
        e.text_line("ЧЕК № 0001");
        e.bold_on();
        e.text_line("МАГАЗИН");
        e.bold_off();
        e.dashed_line(32);
        e.align_center();
        e.text_line("Дякуємо!");
        e.cut(true);

        let data = e.build();
        assert!(!data.is_empty());
        assert_eq!(data[0], ESC);
        assert_eq!(data[1], 0x40);
    }

    #[test]
    fn test_utf8_truncation() {
        let long_name = "Хліб житній з висівками";
        let truncated = truncate_utf8(long_name, 18);
        assert!(!truncated.contains('�'));
        println!("Original length: {} chars", long_name.chars().count());
        println!("Truncated: '{}'", truncated);
    }

    #[test]
    fn test_windows_1251_receipt() {
        let req = ReceiptPrintRequest {
            shop_name: "Калина".to_string(),
            shop_address: "вул. Центральна, 1".to_string(),
            tax_id: "12345678".to_string(),
            receipt_number: "0001".to_string(),
            date: "28.07.2026".to_string(),
            time: "14:30".to_string(),
            cashier: "Іван".to_string(),
            items: vec![
                ReceiptItemData {
                    barcode: Some("4821234567890".to_string()),
                    name: "Хліб білий".to_string(),
                    quantity: 2.0,
                    price: 25.00,
                    total: 50.00,
                },
                ReceiptItemData {
                    barcode: None,
                    name: "Молоко".to_string(),
                    quantity: 1.0,
                    price: 32.00,
                    total: 32.00,
                },
            ],
            total: 82.00,
            payment_method: "Готівка".to_string(),
            paid: 100.00,
            change: 18.00,
            footer: Some("Повернення згідно закону".to_string()),
        };

        let escpos = build_receipt_escpos(&req);

        eprintln!("ESC/POS (first 100 bytes): {:02X?}", &escpos[..100.min(escpos.len())]);

        // Перевіряємо наявність FS . (вимкнення китайського режиму)
        assert!(escpos.windows(2).any(|w| w == &[FS, 0x2E]),
            "Не знайдено команду FS . (вимкнення китайського режиму)");

        // Перевіряємо наявність ESC t 73 (WPC1251)
        assert!(escpos.windows(3).any(|w| w == &[ESC, 0x74, 73]),
            "Не знайдено команду ESC t 73 (WPC1251)");

        // Перевіряємо, що є обрізка в кінці
        assert!(escpos.ends_with(&[0x1D, 0x56, 0x01]));
    }

    #[test]
    fn test_ukrainian_names_safe() {
        // Тест з довгими українськими назвами — не має панікувати
        let req = ReceiptPrintRequest {
            shop_name: "Продукти".to_string(),
            shop_address: "".to_string(),
            tax_id: "".to_string(),
            receipt_number: "9999".to_string(),
            date: "28.07.2026".to_string(),
            time: "15:00".to_string(),
            cashier: "Мар'яна".to_string(),
            items: vec![
                ReceiptItemData {
                    barcode: Some("4821111111111".to_string()),
                    name: "Сир кисломолочний 9% «Селянський»".to_string(),
                    quantity: 1.0,
                    price: 89.50,
                    total: 89.50,
                },
                ReceiptItemData {
                    barcode: None,
                    name: "Яйця курячі відбірні (10 шт)".to_string(),
                    quantity: 2.0,
                    price: 45.00,
                    total: 90.00,
                },
            ],
            total: 179.50,
            payment_method: "Готівка".to_string(),
            paid: 200.00,
            change: 20.50,
            footer: None,
        };

        // Не має панікувати
        let escpos = build_receipt_escpos(&req);
        assert!(!escpos.is_empty());

        // Перевіряємо наявність команди WPC1251
        assert!(escpos.windows(3).any(|w| w == &[ESC, 0x74, 73]),
            "Не знайдено команду ESC t 73 (WPC1251)");

        eprintln!("ESC/POS total size: {} bytes", escpos.len());
    }

    #[test]
    fn test_windows_1251_ascii_passthrough() {
        // ASCII символи мають передаватися без змін
        let text = "ABC123";
        let cp1251 = to_windows_1251(text);
        assert_eq!(cp1251, b"ABC123");
    }

    #[test]
    fn test_ukrainian_chars_in_windows_1251() {
        // Windows-1251 підтримує всі українські символи:
        //   І=0xB2, ї=0xBF, є=0xBA, ґ=0xB4
        let text = "Іван їсть євро ґанок";
        let cp1251 = to_windows_1251(text);
        // 'І' (U+0406) -> 0xB2 in Windows-1251
        assert_eq!(cp1251[0], 0xB2, "І in Windows-1251 should be 0xB2");
        // 'ї' (U+0457) -> 0xBF
        assert_eq!(cp1251[5], 0xBF, "ї in Windows-1251 should be 0xBF");
        // 'є' (U+0454) -> 0xBA
        assert_eq!(cp1251[10], 0xBA, "є in Windows-1251 should be 0xBA");
        // 'ґ' (U+0491) -> 0xB4
        assert_eq!(cp1251[15], 0xB4, "ґ in Windows-1251 should be 0xB4");
        eprintln!("'Іван їсть євро ґанок' в Windows-1251: {:02X?}", cp1251);
    }

    #[test]
    fn test_calculate_receipt_height() {
        // Мінімум: 0 товарів -> 3
        assert_eq!(calculate_receipt_height(0), 3);
        assert_eq!(calculate_receipt_height(1), 3);
        assert_eq!(calculate_receipt_height(3), 3);
        assert_eq!(calculate_receipt_height(4), 3); // 4/3+2=3.33 -> 3

        // Середній чек: 10 товарів -> 10/3+2 ≈ 5
        assert_eq!(calculate_receipt_height(10), 5); // 10/3+2=5.33 -> 5

        // Великий чек: 50 товарів -> 50/3+2 ≈ 18
        let h = calculate_receipt_height(50);
        assert!(h > 5, "50 товарів має дати >5 рядків, отримано {}", h);

        // Максимум: 100 товарів -> 20 (cap)
        assert_eq!(calculate_receipt_height(100), 20);
    }

    #[test]
    fn test_build_receipt_uses_feed_8() {
        // Перевіряємо, що build_receipt_escpos використовує feed(8) (фіксований)
        let req = ReceiptPrintRequest {
            shop_name: "Тест".to_string(),
            shop_address: "".to_string(),
            tax_id: "".to_string(),
            receipt_number: "1".to_string(),
            date: "01.01.2026".to_string(),
            time: "12:00".to_string(),
            cashier: "".to_string(),
            items: vec![
                ReceiptItemData {
                    barcode: None,
                    name: "Товар 1".to_string(),
                    quantity: 1.0,
                    price: 10.00,
                    total: 10.00,
                },
                ReceiptItemData {
                    barcode: None,
                    name: "Товар 2".to_string(),
                    quantity: 1.0,
                    price: 20.00,
                    total: 20.00,
                },
            ],
            total: 30.00,
            payment_method: "Готівка".to_string(),
            paid: 30.00,
            change: 0.00,
            footer: None,
        };
        let escpos = build_receipt_escpos(&req);

        // Перевіряємо, що є ESC d 8 (feed 8)
        assert!(escpos.windows(3).any(|w| w == &[ESC, 0x64, 8]),
            "Чек має містити ESC d 8 (feed 8), знайдено: {:?}",
            escpos.windows(3).filter(|w| w[0] == ESC && w[1] == 0x64).collect::<Vec<_>>()
        );

        // Перевіряємо, що більший чек теж має feed(8), не більше
        let mut items_big = Vec::new();
        for i in 0..50 {
            items_big.push(ReceiptItemData {
                barcode: None,
                name: format!("Товар {}", i + 1),
                quantity: 1.0,
                price: 10.00,
                total: 10.00,
            });
        }
        let req_big = ReceiptPrintRequest {
            shop_name: "Тест".to_string(),
            shop_address: "".to_string(),
            tax_id: "".to_string(),
            receipt_number: "2".to_string(),
            date: "01.01.2026".to_string(),
            time: "12:00".to_string(),
            cashier: "".to_string(),
            items: items_big,
            total: 500.00,
            payment_method: "Готівка".to_string(),
            paid: 500.00,
            change: 0.00,
            footer: None,
        };
        let escpos_big = build_receipt_escpos(&req_big);

        // Великий чек теж має feed(8)
        assert!(escpos_big.windows(3).any(|w| w == &[ESC, 0x64, 8]),
            "Великий чек має містити ESC d 8 (feed 8)");
    }
}
