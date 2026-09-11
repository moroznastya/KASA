//! XML ПРРО — побудова повідомлень для фіскального сервера ДПС (API ФСКО)
//! та канонізація (C14N). Байт-ідентичний Python-еталону
//! `backend/.../prro/xml_builder.py` (спека prro_fix_spec_2026-09-08, A–J).
//!
//! СТРУКТУРА ПОВІДОМЛЕННЯ (ПРРО, 262576.docx):
//! ```xml
//! <?xml version="1.0" encoding="windows-1251"?>
//! <RQ V="1"><DAT FN=".." TN=".." ZN=".." DI=".." DT="0" V="1">
//!   <C T="0|1">…</C> | <Z …>…</Z> | <C T="108..112">…</C>
//!   <TS>YYYYMMDDhhmmss</TS>
//! </DAT><MAC ID="">{hex}</MAC></RQ>
//! ```
//!
//! - Одна пара `<DAT>…</DAT><MAC>…</MAC>` на повідомлення.
//! - Кодування windows-1251, XML-декларація обов'язкова.
//! - `<TS>` — ЛОКАЛЬНИЙ час YYYYMMDDhhmmss.
//! - MAC: лише атрибут ID (без DI/NT). Значення = hex (lowercase) sha256
//!   ПОВНОГО RQ попереднього Check (байти windows-1251). Перший документ
//!   ПРРО/після скидання ланцюга — значення порожнє. `<H>`-ланцюжка в тілі
//!   чеку НЕМАЄ — ланцюг контролюється самим <MAC>.
//! - Службові: T=111 → `<MAC></MAC>` (без ID/значення); T=112 → `<MAC>` без ID;
//!   офлайн-документи → `<MAC ID="{резервний №}">`.
//!
//! Канонічний вигляд (Додаток А СЗЗД): атрибути в алфавітному порядку, теги
//! завжди закриті (`<tag></tag>`), пробіли між тегами видаляються.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Коди типів чеку в `<C T="...">`.
pub const CHK_TYPE_SALE: &str = "0";
pub const CHK_TYPE_RETURN: &str = "1";
pub const CHK_TYPE_SERVICE: &str = "2";

/// Коди службових чеків з 01.10.2021 (sendChkV2).
pub const SERVICE_OPEN_SHIFT: &str = "108";
pub const SERVICE_OFFLINE: &str = "109";
pub const SERVICE_ONLINE: &str = "110";
pub const SERVICE_PING: &str = "111";
pub const SERVICE_RESERVE: &str = "112";

/// Всі допустимі типи службових чеків.
pub const SERVICE_TYPES: [&str; 5] = [
    SERVICE_OPEN_SHIFT,
    SERVICE_OFFLINE,
    SERVICE_ONLINE,
    SERVICE_PING,
    SERVICE_RESERVE,
];

/// XML-декларація (обов'язкова, windows-1251) — 1:1 Python `XML_DECLARATION`.
pub const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"windows-1251\"?>";

/// Розмір діапазону резервних номерів для T=112 (за зразком ДПС).
pub const DEFAULT_RESERVE_SIZE: i64 = 150;

#[derive(Debug, thiserror::Error)]
pub enum XmlBuilderError {
    #[error("Порожній XML-документ")]
    EmptyXml,
    #[error("Некоректний XML: {0}")]
    InvalidXml(String),
    #[error("Невідомий тип службового чеку: {0}. Допустимі значення: 108, 109, 110, 111, 112")]
    UnknownServiceType(String),
    #[error("Не вдалося визначити DI пакету даних: у <DAT> відсутній атрибут DI")]
    MissingDi,
    #[error("Невірне десяткове значення: {0}")]
    InvalidDecimal(String),
    #[error("Символ неможливо закодувати у windows-1251: {0}")]
    Cp1251(String),
}

/// Позиція чеку `<P>`.
#[derive(Debug, Clone, Default)]
pub struct ReceiptItem {
    pub code: Option<String>,
    pub barcode: Option<String>,
    pub name: String,
    pub quantity: String, // Decimal, грн
    pub price: String,    // Decimal, грн
    pub total: String,    // Decimal, грн
    pub tax_rate: String, // "0"|"1"|"2"|"-1"
}

/// Оплата `<M>`.
#[derive(Debug, Clone, Default)]
pub struct Payment {
    pub code: String,
    pub name: Option<String>,
    pub amount: String,
    pub change: Option<String>,
}

/// Знижка/націнка `<D>`/`<S>`.
#[derive(Debug, Clone, Default)]
pub struct Discount {
    pub kind: DiscountKind, // D — знижка, S — націнка
    pub tr: String,
    pub ty: String,
    pub percent: Option<String>,
    pub total: String,
    pub ni: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscountKind {
    #[default]
    Discount,
    Surcharge,
}

/// Податкова група (для вкладених `<TX>` у `<E>` або `<TXS>` у Z-звіті).
#[derive(Debug, Clone, Default)]
pub struct TaxGroup {
    pub tax: String,
    pub percent: Option<String>,
    pub total: Option<String>, // TXSM (копійки ×100 у значенні)
    pub dtpr: Option<String>,
    pub dtsm: Option<String>,
    pub tax_type: Option<String>,
    pub tax_algorithm: Option<String>,
    pub ts: Option<String>, // для Z-звіту
    pub tax_in: Option<String>,
    pub tax_out: Option<String>,
    pub dti: Option<String>,
    pub dto: Option<String>,
    pub smi: Option<String>,
    pub smo: Option<String>,
}

/// Підсумки чеку `<E>`.
#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub fiscal_number: Option<i64>, // NO
    pub total: String,              // SM
    pub se: Option<String>,         // SE (сума без ПДВ)
    pub tax_rate: String,           // TX
    pub tax_percent: Option<String>,
    pub tax_total: Option<String>, // TXSM
    pub dtpr: Option<String>,
    pub dtsm: Option<String>,
    pub tax_type: Option<String>,
    pub tax_algorithm: Option<String>,
    pub cashier: Option<i64>,      // CS
    pub tax_groups: Vec<TaxGroup>, // декілька груп → вкладені <TX>
}

/// Дані зміни для Z-звіту `<Z>`.
#[derive(Debug, Clone, Default)]
pub struct ShiftData {
    pub shift_number: i64,
    pub sales_count: i64,
    pub returns_count: i64,
    pub taxes: Vec<TaxGroup>,
    pub payments: Vec<ShiftPayment>,
    pub cash_io: Vec<ShiftPayment>,
    pub operations: Option<ShiftOperations>,
}

#[derive(Debug, Clone, Default)]
pub struct ShiftPayment {
    pub code: String,
    pub name: Option<String>,
    pub smi: Option<String>,
    pub smo: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ShiftOperations {
    pub qp: i64,
    pub qs: Option<String>,
}

// ─── Утиліти: числа (1:1 Python decimal) ─────────────────────────────────────

fn _as_decimal(value: &str) -> Result<Decimal, XmlBuilderError> {
    Decimal::from_str(value).map_err(|e| XmlBuilderError::InvalidDecimal(format!("{value}: {e}")))
}

/// Сума в гривнях → копійки (×100, ROUND_HALF_UP) — 1:1 Python `_to_cents`.
pub fn to_cents(amount: &str) -> Result<i64, XmlBuilderError> {
    use rust_decimal::RoundingStrategy;
    let v = _as_decimal(amount)?;
    let scaled =
        (v * Decimal::from(100)).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
    scaled
        .to_i64()
        .ok_or_else(|| XmlBuilderError::InvalidDecimal(format!("{amount} → overflow")))
}

/// Кількість → тисячні частки (×1000, ROUND_HALF_UP) — 1:1 Python `_to_thousandths`.
pub fn to_thousandths(quantity: &str) -> Result<i64, XmlBuilderError> {
    use rust_decimal::RoundingStrategy;
    let v = _as_decimal(quantity)?;
    let scaled =
        (v * Decimal::from(1000)).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
    scaled
        .to_i64()
        .ok_or_else(|| XmlBuilderError::InvalidDecimal(format!("{quantity} → overflow")))
}

/// Відсоток у вигляді "00.00" (20 → "20.00") — 1:1 Python `_format_percent`.
pub fn format_percent(value: &str) -> Result<String, XmlBuilderError> {
    let dec = _as_decimal(value)?.round_dp(2);
    Ok(format!("{dec:.2}"))
}

// ─── Екранування XML ─────────────────────────────────────────────────────────

/// Екранує спеціальні символи XML у текстовому вмісті (1:1 Python `_esc_text`).
fn esc_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Екранує спеціальні символи XML у значенні атрибута (1:1 Python `_esc_attr`).
fn esc_attr(value: &str) -> String {
    esc_text(value).replace('"', "&quot;")
}

// ─── Канонічний вигляд (Додаток А СЗЗД 2.1.7) ───────────────────────────────

/// Мінімальний XML-розбір для канонізації: елементи + атрибути + текст.
/// Не повний XML-парсер — покриває структуру пакетів СЗЗД (без CDATA/коментарів).
fn parse_xml(xml: &str) -> Result<XmlNode, XmlBuilderError> {
    let bytes = xml.as_bytes();
    let mut pos = 0usize;

    // Пропустити XML-декларацію / коментарі / пробіли до кореня
    loop {
        // пропустити пробіли
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if bytes[pos..].starts_with(b"<?xml") {
            // до "?>"
            let end = find_sub(bytes, pos, b"?>")
                .ok_or_else(|| XmlBuilderError::InvalidXml("XML-декларація без закриття".into()))?;
            pos = end + 2;
        } else if bytes[pos..].starts_with(b"<!--") {
            let end = find_sub(bytes, pos, b"-->")
                .ok_or_else(|| XmlBuilderError::InvalidXml("коментар без закриття".into()))?;
            pos = end + 3;
        } else {
            break;
        }
    }

    let (node, next) = parse_element(bytes, pos)?;
    // після кореня — лише пробіли (дозволено)
    let _ = next;
    Ok(node)
}

fn find_sub(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

#[derive(Debug, Clone)]
struct XmlNode {
    tag: String,
    attrs: Vec<(String, String)>, // (ім'я, значення вже розкодоване)
    text: String,                 // текстовий вміст до першого дочірнього елемента
    children: Vec<XmlNode>,
    tails: Vec<String>, // tail після кожного дочірнього (у порядку)
}

fn parse_element(bytes: &[u8], mut pos: usize) -> Result<(XmlNode, usize), XmlBuilderError> {
    // очікуємо '<'
    if pos >= bytes.len() || bytes[pos] != b'<' {
        return Err(XmlBuilderError::InvalidXml(format!(
            "очікувався '<' на позиції {pos}"
        )));
    }
    pos += 1;
    // ім'я тега
    let name_start = pos;
    while pos < bytes.len()
        && (bytes[pos].is_ascii_alphanumeric() || matches!(bytes[pos], b'_' | b':' | b'-' | b'.'))
    {
        pos += 1;
    }
    let tag = std::str::from_utf8(&bytes[name_start..pos])
        .map_err(|_| XmlBuilderError::InvalidXml("тег не UTF-8".into()))?
        .to_string();
    if tag.is_empty() {
        return Err(XmlBuilderError::InvalidXml("порожнє ім'я тега".into()));
    }

    // атрибути
    let mut attrs: Vec<(String, String)> = Vec::new();
    loop {
        // пропустити пробіли
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            return Err(XmlBuilderError::InvalidXml(
                "несподіваний кінець тега".into(),
            ));
        }
        if bytes[pos] == b'>' {
            pos += 1;
            break;
        }
        if bytes[pos] == b'/' && pos + 1 < bytes.len() && bytes[pos + 1] == b'>' {
            // самозакривний <tag/>
            pos += 2;
            return Ok((
                XmlNode {
                    tag,
                    attrs,
                    text: String::new(),
                    children: vec![],
                    tails: vec![],
                },
                pos,
            ));
        }
        // ім'я атрибута
        let a_start = pos;
        while pos < bytes.len()
            && (bytes[pos].is_ascii_alphanumeric()
                || matches!(bytes[pos], b'_' | b':' | b'-' | b'.'))
        {
            pos += 1;
        }
        let a_name = std::str::from_utf8(&bytes[a_start..pos])
            .map_err(|_| XmlBuilderError::InvalidXml("атрибут не UTF-8".into()))?
            .to_string();
        // пропустити пробіли
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'=' {
            return Err(XmlBuilderError::InvalidXml(format!(
                "атрибут {a_name} без '='"
            )));
        }
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || (bytes[pos] != b'"' && bytes[pos] != b'\'') {
            return Err(XmlBuilderError::InvalidXml(format!(
                "атрибут {a_name} без лапок"
            )));
        }
        let quote = bytes[pos];
        pos += 1;
        let v_start = pos;
        while pos < bytes.len() && bytes[pos] != quote {
            pos += 1;
        }
        if pos >= bytes.len() {
            return Err(XmlBuilderError::InvalidXml(format!(
                "атрибут {a_name} без закриття"
            )));
        }
        let raw = std::str::from_utf8(&bytes[v_start..pos])
            .map_err(|_| XmlBuilderError::InvalidXml("значення атрибута не UTF-8".into()))?;
        pos += 1;
        attrs.push((a_name, xml_unescape(raw)?));
    }

    // текстовий вміст
    let mut children: Vec<XmlNode> = Vec::new();
    let mut tails: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut first_text = true;
    loop {
        // текст до наступного '<'
        let t_start = pos;
        while pos < bytes.len() && bytes[pos] != b'<' {
            pos += 1;
        }
        let raw_text = std::str::from_utf8(&bytes[t_start..pos])
            .map_err(|_| XmlBuilderError::InvalidXml("текст не UTF-8".into()))?;
        let decoded = xml_unescape(raw_text)?;
        if first_text {
            text = decoded;
            first_text = false;
        } else {
            tails.push(decoded);
        }
        if pos >= bytes.len() {
            return Err(XmlBuilderError::InvalidXml("немає закриття тега".into()));
        }
        if bytes[pos..].starts_with(b"</") {
            // закриття
            pos += 2;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            // ім'я закриття (ігноруємо — перевіримо)
            while pos < bytes.len()
                && (bytes[pos].is_ascii_alphanumeric()
                    || matches!(bytes[pos], b'_' | b':' | b'-' | b'.'))
            {
                pos += 1;
            }
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= bytes.len() || bytes[pos] != b'>' {
                return Err(XmlBuilderError::InvalidXml(
                    "некоректне закриття тега".into(),
                ));
            }
            pos += 1;
            return Ok((
                XmlNode {
                    tag,
                    attrs,
                    text,
                    children,
                    tails,
                },
                pos,
            ));
        }
        if bytes[pos..].starts_with(b"<!--") {
            let end = find_sub(bytes, pos, b"-->")
                .ok_or_else(|| XmlBuilderError::InvalidXml("коментар без закриття".into()))?;
            pos = end + 3;
            continue;
        }
        // дочірній елемент
        let (child, next) = parse_element(bytes, pos)?;
        children.push(child);
        pos = next;
    }
}

/// XML-unescape тексту/атрибутів (1:1 lxml: resolve_entities=False, але стандартні).
fn xml_unescape(s: &str) -> Result<String, XmlBuilderError> {
    if !s.contains('&') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let semi = after
            .find(';')
            .ok_or_else(|| XmlBuilderError::InvalidXml("незавершена entity".into()))?;
        let ent = &after[..=semi];
        let rep = match ent {
            "&amp;" => "&",
            "&lt;" => "<",
            "&gt;" => ">",
            "&quot;" => "\"",
            "&apos;" => "'",
            _ => {
                return Err(XmlBuilderError::InvalidXml(format!(
                    "невідома entity {ent}"
                )))
            }
        };
        out.push_str(rep);
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Рекурсивна серіалізація у канонічному вигляді СЗЗД (1:1 Python `_canonical_serialize`).
fn canonical_serialize(node: &XmlNode, out: &mut String) {
    // атрибути — в алфавітному порядку
    let mut sorted: BTreeMap<&str, &str> = BTreeMap::new();
    for (k, v) in &node.attrs {
        sorted.insert(k.as_str(), v.as_str());
    }
    let mut attrs = String::new();
    for (name, value) in sorted {
        let _ = write!(attrs, " {}=\"{}\"", name, esc_attr(value));
    }
    let _ = write!(out, "<{}{}>", node.tag, attrs);

    let text = if node.text.trim().is_empty() {
        ""
    } else {
        &node.text
    };

    if node.children.is_empty() {
        let _ = write!(out, "{}</{}>", text, node.tag);
        return;
    }

    out.push_str(text);
    for (i, child) in node.children.iter().enumerate() {
        canonical_serialize(child, out);
        let tail = node.tails.get(i).map(String::as_str).unwrap_or("");
        if !tail.trim().is_empty() {
            out.push_str(tail);
        }
    }
    let _ = write!(out, "</{}>", node.tag);
}

/// Канонічний вигляд XML пакету даних (Додаток А СЗЗД 2.1.7) — 1:1 Python `canonicalize`.
pub fn canonicalize(xml: &str) -> Result<String, XmlBuilderError> {
    if xml.trim().is_empty() {
        return Err(XmlBuilderError::EmptyXml);
    }
    let root = parse_xml(xml)?;
    let mut out = String::new();
    canonical_serialize(&root, &mut out);
    Ok(out)
}

// ─── Кодування / MAC / хеш-ланцюжок ────────────────────────────────────────

/// Кодує повне RQ-повідомлення у байти windows-1251 — 1:1 Python
/// `cp1251_bytes`. Саме ці байти підписуються (CAdES/XAdES) і саме над ними
/// обчислюється хеш-ланцюжок (MAC наступного Check). Символ поза
/// windows-1251 → помилка (краще явна помилка, ніж мовчазне спотворення).
pub fn cp1251_bytes(message: &str) -> Result<Vec<u8>, XmlBuilderError> {
    let (bytes, _, had_errors) = encoding_rs::WINDOWS_1251.encode(message);
    if had_errors {
        return Err(XmlBuilderError::Cp1251(
            "символ неможливо представити у windows-1251".to_string(),
        ));
    }
    Ok(bytes.into_owned())
}

/// Декодує байти windows-1251 у текст (зворотний до [`cp1251_bytes`]).
/// windows-1251 покриває всі 256 байтів — декодування не може не вдатись.
pub fn cp1251_decode(bytes: &[u8]) -> String {
    let (cow, _, _) = encoding_rs::WINDOWS_1251.decode(bytes);
    cow.into_owned()
}

/// Перетворює підписані байти у текст для зберігання (check_sign) — 1:1 Python
/// `signed_bytes_to_text`. XAdES — XML windows-1251 → текст windows-1251;
/// бінарний CAdES (ІІТ) — теж декодується у cp1251 (бієктивне кодування:
/// повторний cp1251_bytes повертає ті самі байти).
pub fn signed_bytes_to_text(signed: &[u8]) -> String {
    cp1251_decode(signed)
}

/// Зворотний до [`signed_bytes_to_text`]: текст check_sign → байти. Якщо текст
/// має префікс "b64:" — base64-декодування (бінарний CAdES), інакше —
/// повторне кодування у windows-1251. 1:1 Python `sync_offline_queue_use_case`.
pub fn signed_text_to_bytes(text: &str) -> Result<Vec<u8>, XmlBuilderError> {
    if let Some(b64) = text.strip_prefix("b64:") {
        use base64::Engine as _;
        return base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| XmlBuilderError::Cp1251(format!("base64 check_sign: {e}")));
    }
    cp1251_bytes(text)
}

/// MAC = hex (lowercase) sha256 XML ПОПЕРЕДНЬОГО Check — 1:1 Python
/// `compute_mac`. Хешується ПОВНИЙ RQ-документ попереднього повідомлення
/// у байтах windows-1251 (те, що «бачить» сервер через CAdES).
/// Для першого документа ПРРО (ланцюг порожній) значення порожнє ("").
pub fn compute_mac(message: &str) -> Result<String, XmlBuilderError> {
    let raw = cp1251_bytes(message)?;
    if raw.is_empty() {
        return Ok(String::new());
    }
    let digest = Sha256::digest(&raw);
    Ok(hex::encode(digest))
}

/// Аліас: назва, що описує призначення в коді ланцюга (1:1 Python `chain_hash`).
pub fn chain_hash(message: &str) -> Result<String, XmlBuilderError> {
    compute_mac(message)
}

/// Визначає тип службового чеку `<C T="...">` у канонічному `<DAT>` (або None).
/// 1:1 Python `_service_type_of`.
fn service_type_of(dat_xml: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"<C\b[^>]*\bT="(\d+)""#).expect("валідний регекс службового типу")
    });
    re.captures(dat_xml)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Будує тег `<MAC>` за зразками ДПС (262576.docx) — 1:1 Python `_mac_tag`:
/// - T=111 (ping): `<MAC></MAC>` — без ID, без значення;
/// - T=112 (резерв): `<MAC>{value}</MAC>` — без атрибута ID;
/// - решта: `<MAC ID="{mac_id}">{value}</MAC>` — ID="" для онлайн,
///   ID={резервний фіскальний номер} для офлайн-документів.
pub fn mac_tag(dat_xml: &str, mac_value: &str, mac_id: &str) -> String {
    match service_type_of(dat_xml).as_deref() {
        Some(SERVICE_PING) => "<MAC></MAC>".to_string(),
        Some(SERVICE_RESERVE) => format!("<MAC>{}</MAC>", esc_text(mac_value)),
        _ => format!(
            "<MAC ID=\"{}\">{}</MAC>",
            esc_attr(mac_id),
            esc_text(mac_value)
        ),
    }
}

/// Відтворює ПОВНЕ RQ-повідомлення зі збережених частин (для хешу ланцюга
/// документа з офлайн-черги) — 1:1 Python `reconstruct_full_rq`.
/// Детерміноване і байт-ідентичне `build_message`, якщо збережені ті самі
/// частини (xml_body=канонічний <DAT>, mac=значення <MAC> документа,
/// id_offline → ID тега MAC для офлайн-чеків).
pub fn reconstruct_full_rq(
    dat_xml: &str,
    mac_value: &str,
    mac_id: &str,
) -> Result<String, XmlBuilderError> {
    let dat_xml = canonicalize(dat_xml)?;
    let mac_value = if mac_value.is_empty() { "" } else { mac_value };
    Ok(format!(
        "{XML_DECLARATION}<RQ V=\"1\">{dat_xml}{}</RQ>",
        mac_tag(&dat_xml, mac_value, mac_id)
    ))
}

/// Дістає NO (номер операції) з XML чека — використовується для H1:
/// lastChk повертає XML останнього чека в data_sign; NO == local_number
/// (Totals.fiscal_number), тому за збігом NO ідентифікуємо "наш" чек.
/// 1:1 Python `extract_check_no`.
pub fn extract_check_no(xml: &str) -> Option<i64> {
    // <E ... NO="123" ...> — атрибут NO тега <E> (номер операції в зміні)
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re =
        RE.get_or_init(|| regex::Regex::new(r#"<E[^>]*NO="(\d+)""#).expect("валидний регекс NO"));
    re.captures(xml)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

// ─── Білдер XML ──────────────────────────────────────────────────────────────

/// Побудова XML-документів СЗЗД 2.1.7 — 1:1 Python `XmlBuilder`.
#[derive(Debug, Clone)]
pub struct XmlBuilder {
    rro_fn: String,
    tax_number: String,
    factory_number: String,
    rro_type: String,
    version: String,
    packet_id: i64,
    mac_number: i64,
}

impl XmlBuilder {
    pub fn new(
        rro_fn: impl Into<String>,
        tax_number: impl Into<String>,
        factory_number: impl Into<String>,
        rro_type: &str,
        version: &str,
        initial_packet_id: i64,
        initial_mac_number: i64,
    ) -> Self {
        Self {
            rro_fn: rro_fn.into(),
            tax_number: tax_number.into(),
            factory_number: factory_number.into(),
            rro_type: rro_type.to_string(),
            version: version.to_string(),
            packet_id: initial_packet_id,
            mac_number: initial_mac_number,
        }
    }

    pub fn rro_fn(&self) -> &str {
        &self.rro_fn
    }

    pub fn last_packet_id(&self) -> i64 {
        self.packet_id
    }

    pub fn last_mac_number(&self) -> i64 {
        self.mac_number
    }

    fn next_packet_id(&mut self) -> i64 {
        self.packet_id += 1;
        self.packet_id
    }

    /// Обгортає вміст у <DAT> (НЕканонічний) — 1:1 Python `_build_dat`.
    fn build_dat(&self, body_xml: &str, ts: &str, di: Option<i64>) -> String {
        let packet_id = di.unwrap_or_else(|| {
            // у Python DI інкрементиться у _build_dat; тут — окремий mutable self,
            // тому імітуємо через переданий di (caller викликає next_packet_id).
            self.packet_id + 1
        });
        let mut parts = vec![
            format!("<DAT FN=\"{}\"", esc_attr(&self.rro_fn)),
            format!("TN=\"{}\"", esc_attr(&self.tax_number)),
            format!("ZN=\"{}\"", esc_attr(&self.factory_number)),
            format!("DI=\"{packet_id}\""),
            format!("V=\"{}\"", esc_attr(&self.version)),
        ];
        if !self.rro_type.is_empty() && self.rro_type != "0" {
            parts.push(format!("DT=\"{}\"", esc_attr(&self.rro_type)));
        }
        format!("{}>{body_xml}<TS>{ts}</TS></DAT>", parts.join(" "))
    }

    /// Чек продажу/повернення — канонічний <DAT> (без <MAC>).
    #[allow(clippy::too_many_arguments)] // API 1:1 Python `build_receipt_xml`
    pub fn build_receipt_xml(
        &mut self,
        check_type: &str,
        items: &[ReceiptItem],
        payments: &[Payment],
        totals: &Totals,
        ts: &str, // YYYYMMDDhhmmss
        discounts: &[Discount],
        comment: Option<&str>,
        return_type: Option<&str>,
    ) -> Result<String, XmlBuilderError> {
        let mut seq = 0i64;
        let mut next_n = || {
            seq += 1;
            seq
        };

        // B1: хеш попереднього Check (СЗЗД 2.1.7, тег <H> — службова інформація,
        // Base64; не друкується; крім ping T=111 та службових 108/109/110/112).
        // H — перша операція чеку (N=1), щоб Python/Rust були байт-ідентичні.

        // Позиції продажу/повернення (<P>)
        let mut p_tags = String::new();
        for item in items {
            let n = next_n();
            let mut attrs = vec![format!("N=\"{n}\"")];
            if let Some(c) = &item.code {
                attrs.push(format!("C=\"{}\"", esc_attr(c)));
            }
            if let Some(bc) = &item.barcode {
                if !bc.is_empty() {
                    attrs.push(format!("CD=\"{}\"", esc_attr(bc)));
                }
            }
            attrs.push(format!("NM=\"{}\"", esc_attr(&item.name)));
            attrs.push(format!("SM=\"{}\"", to_cents(&item.total)?));
            attrs.push(format!("Q=\"{}\"", to_thousandths(&item.quantity)?));
            attrs.push(format!("PRC=\"{}\"", to_cents(&item.price)?));
            attrs.push(format!("TX=\"{}\"", esc_attr(&item.tax_rate)));
            let _ = write!(p_tags, "<P {}></P>", attrs.join(" "));
        }

        // Знижки/націнки (<D>/<S>)
        let mut d_tags = String::new();
        for disc in discounts {
            let n = next_n();
            let tag = match disc.kind {
                DiscountKind::Discount => "D",
                DiscountKind::Surcharge => "S",
            };
            let mut attrs = vec![
                format!("N=\"{n}\""),
                format!("TR=\"{}\"", esc_attr(&disc.tr)),
                format!("TY=\"{}\"", esc_attr(&disc.ty)),
            ];
            if let Some(p) = &disc.percent {
                attrs.push(format!("PR=\"{}\"", format_percent(p)?));
            }
            attrs.push(format!("SM=\"{}\"", to_cents(&disc.total)?));
            if let Some(ni) = disc.ni {
                attrs.push(format!("NI=\"{ni}\""));
            }
            let _ = write!(d_tags, "<{tag} {}></{tag}>", attrs.join(" "));
        }

        // Оплати (<M>)
        let mut m_tags = String::new();
        for pay in payments {
            let n = next_n();
            let mut attrs = vec![
                format!("N=\"{n}\""),
                format!("T=\"{}\"", esc_attr(&pay.code)),
            ];
            if let Some(name) = &pay.name {
                if !name.is_empty() {
                    attrs.push(format!("NM=\"{}\"", esc_attr(name)));
                }
            }
            attrs.push(format!("SM=\"{}\"", to_cents(&pay.amount)?));
            if let Some(ch) = &pay.change {
                // RM (решта) — лише коли решта > 0 (спека J; 1:1 Python)
                attrs.push(format!("RM=\"{}\"", to_cents(ch)?));
            }
            let _ = write!(m_tags, "<M {}></M>", attrs.join(" "));
        }

        // Коментар (<L>)
        let mut l_tags = String::new();
        if let Some(comment) = comment {
            let n = next_n();
            let _ = write!(l_tags, "<L N=\"{n}\">{}</L>", esc_text(comment));
        }

        // Закриття чеку (<E>)
        let e_n = next_n();
        let mut e_attrs = vec![format!("N=\"{e_n}\"")];
        if let Some(no) = totals.fiscal_number {
            e_attrs.push(format!("NO=\"{no}\""));
        }
        e_attrs.push(format!("SM=\"{}\"", to_cents(&totals.total)?));
        if let Some(se) = &totals.se {
            e_attrs.push(format!("SE=\"{}\"", to_cents(se)?));
        }
        e_attrs.push(format!("FN=\"{}\"", esc_attr(&self.rro_fn)));
        e_attrs.push(format!("TS=\"{ts}\""));

        let e_tag = if !totals.tax_groups.is_empty() {
            let mut tx_tags = String::new();
            for g in &totals.tax_groups {
                let mut g_attrs = vec![
                    format!("TX=\"{}\"", esc_attr(&g.tax)),
                    format!(
                        "TXPR=\"{}\"",
                        format_percent(g.percent.as_deref().unwrap_or("0"))?
                    ),
                    format!("TXSM=\"{}\"", to_cents(g.total.as_deref().unwrap_or("0"))?),
                    format!(
                        "DTPR=\"{}\"",
                        format_percent(g.dtpr.as_deref().unwrap_or("0"))?
                    ),
                    format!("DTSM=\"{}\"", to_cents(g.dtsm.as_deref().unwrap_or("0"))?),
                    format!(
                        "TXTY=\"{}\"",
                        esc_attr(g.tax_type.as_deref().unwrap_or("0"))
                    ),
                    format!(
                        "TXAL=\"{}\"",
                        esc_attr(g.tax_algorithm.as_deref().unwrap_or("0"))
                    ),
                ];
                let _ = write!(tx_tags, "<TX {}></TX>", g_attrs.join(" "));
                g_attrs.clear();
            }
            format!("<E {}>{tx_tags}</E>", e_attrs.join(" "))
        } else {
            e_attrs.push(format!("TX=\"{}\"", esc_attr(&totals.tax_rate)));
            if let Some(tp) = &totals.tax_percent {
                e_attrs.push(format!("TXPR=\"{}\"", format_percent(tp)?));
            }
            if let Some(tt) = &totals.tax_total {
                e_attrs.push(format!("TXSM=\"{}\"", to_cents(tt)?));
            }
            if let Some(dtpr) = &totals.dtpr {
                e_attrs.push(format!("DTPR=\"{}\"", format_percent(dtpr)?));
            }
            if let Some(dtsm) = &totals.dtsm {
                e_attrs.push(format!("DTSM=\"{}\"", to_cents(dtsm)?));
            }
            e_attrs.push(format!(
                "TXTY=\"{}\"",
                esc_attr(totals.tax_type.as_deref().unwrap_or("0"))
            ));
            e_attrs.push(format!(
                "TXAL=\"{}\"",
                esc_attr(totals.tax_algorithm.as_deref().unwrap_or("0"))
            ));
            if let Some(cs) = totals.cashier {
                e_attrs.push(format!("CS=\"{cs}\""));
            }
            format!("<E {}></E>", e_attrs.join(" "))
        };

        // RT — тільки для повернення (T="1")
        let mut c_attrs = vec![format!("T=\"{}\"", esc_attr(check_type))];
        if check_type == CHK_TYPE_RETURN {
            c_attrs.push(format!("RT=\"{}\"", esc_attr(return_type.unwrap_or("0"))));
        }

        let mut body = String::new();
        let _ = write!(body, "<C {}>", c_attrs.join(" "));
        body.push_str(&p_tags);
        body.push_str(&d_tags);
        body.push_str(&m_tags);
        body.push_str(&l_tags);
        body.push_str(&e_tag);
        body.push_str("</C>");

        let di = self.next_packet_id();
        let dat_xml = self.build_dat(&body, ts, Some(di));
        canonicalize(&dat_xml)
    }

    /// Z-звіт — канонічний <DAT> (без <MAC>).
    pub fn build_zreport_xml(
        &mut self,
        shift: &ShiftData,
        ts: &str,
    ) -> Result<String, XmlBuilderError> {
        // Підсумки по податках (<TXS>)
        let mut txs_tags = String::new();
        for tax in &shift.taxes {
            let mut attrs = vec![format!("TX=\"{}\"", esc_attr(&tax.tax))];
            if let Some(t) = &tax.ts {
                attrs.push(format!("TS=\"{}\"", esc_attr(t)));
            }
            if let Some(tp) = &tax.percent {
                attrs.push(format!("TXPR=\"{}\"", format_percent(tp)?));
            }
            if let Some(v) = &tax.tax_in {
                attrs.push(format!("TXI=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &tax.tax_out {
                attrs.push(format!("TXO=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &tax.dtpr {
                attrs.push(format!("DTPR=\"{}\"", format_percent(v)?));
            }
            if let Some(v) = &tax.dti {
                attrs.push(format!("DTI=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &tax.dto {
                attrs.push(format!("DTO=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &tax.tax_type {
                attrs.push(format!("TXTY=\"{}\"", esc_attr(v)));
            }
            if let Some(v) = &tax.tax_algorithm {
                attrs.push(format!("TXAL=\"{}\"", esc_attr(v)));
            }
            if let Some(v) = &tax.smi {
                attrs.push(format!("SMI=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &tax.smo {
                attrs.push(format!("SMO=\"{}\"", to_cents(v)?));
            }
            let _ = write!(txs_tags, "<TXS {}></TXS>", attrs.join(" "));
        }

        // Обороти по формах оплати (<M>)
        let mut m_tags = String::new();
        for pay in &shift.payments {
            let mut attrs = vec![format!("T=\"{}\"", esc_attr(&pay.code))];
            if let Some(name) = &pay.name {
                if !name.is_empty() {
                    attrs.push(format!("NM=\"{}\"", esc_attr(name)));
                }
            }
            if let Some(v) = &pay.smi {
                attrs.push(format!("SMI=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &pay.smo {
                attrs.push(format!("SMO=\"{}\"", to_cents(v)?));
            }
            let _ = write!(m_tags, "<M {}></M>", attrs.join(" "));
        }

        // Внесення/видачі (<IO>)
        let mut io_tags = String::new();
        for io in &shift.cash_io {
            let mut attrs = vec![format!("T=\"{}\"", esc_attr(&io.code))];
            if let Some(name) = &io.name {
                if !name.is_empty() {
                    attrs.push(format!("NM=\"{}\"", esc_attr(name)));
                }
            }
            if let Some(v) = &io.smi {
                attrs.push(format!("SMI=\"{}\"", to_cents(v)?));
            }
            if let Some(v) = &io.smo {
                attrs.push(format!("SMO=\"{}\"", to_cents(v)?));
            }
            let _ = write!(io_tags, "<IO {}></IO>", attrs.join(" "));
        }

        // Кількість чеків (<NC>)
        let nc_tag = format!(
            "<NC NI=\"{}\" NO=\"{}\"></NC>",
            shift.sales_count, shift.returns_count
        );

        // Операції переказу (<OP>)
        let mut op_tags = String::new();
        if let Some(op) = &shift.operations {
            let mut attrs = vec![format!("QP=\"{}\"", op.qp)];
            if let Some(qs) = &op.qs {
                attrs.push(format!("QS=\"{}\"", to_cents(qs)?));
            }
            let _ = write!(op_tags, "<OP {}></OP>", attrs.join(" "));
        }

        let mut z_body = String::new();
        z_body.push_str(&txs_tags);
        z_body.push_str(&m_tags);
        z_body.push_str(&io_tags);
        z_body.push_str(&nc_tag);
        z_body.push_str(&op_tags);
        let z_xml = format!("<Z NO=\"{}\">{z_body}</Z>", shift.shift_number);

        let di = self.next_packet_id();
        let dat_xml = self.build_dat(&z_xml, ts, Some(di));
        canonicalize(&dat_xml)
    }

    /// Службовий чек (108–112) — канонічний <DAT> (без <MAC>).
    pub fn build_service_check_xml(
        &mut self,
        service_type: &str,
        ts: &str,
        reserve_size: i64,
    ) -> Result<String, XmlBuilderError> {
        if !SERVICE_TYPES.contains(&service_type) {
            return Err(XmlBuilderError::UnknownServiceType(
                service_type.to_string(),
            ));
        }
        // Тіло за зразками ДПС (262576.docx), БЕЗ <E N="1"> (спека I):
        //   108/109/110/111: `<C T="..."></C>`;
        //   112: `<C T="112"><H SIZE="{size}"></H></C>`.
        let body = if service_type == SERVICE_RESERVE {
            format!(r#"<C T="{service_type}"><H SIZE="{reserve_size}"></H></C>"#)
        } else {
            format!(r#"<C T="{service_type}"></C>"#)
        };
        let di = self.next_packet_id();
        let dat_xml = self.build_dat(&body, ts, Some(di));
        canonicalize(&dat_xml)
    }
    /// Повне повідомлення <RQ>…</RQ> з <MAC> — 1:1 Python `build_message`.
    ///
    /// Формат (ПРРО, спека A):
    /// `<?xml version="1.0" encoding="windows-1251"?><RQ V="1"><DAT …>…<TS>…
    /// </TS></DAT><MAC …>{hex}</MAC></RQ>` — одна пара DAT/MAC.
    ///
    /// MAC-значення = hex sha256 XML ПОПЕРЕДНЬОГО Check (mac_value).
    /// None/"" — перший документ після скидання ланцюга.
    ///
    /// - mac_id: ID тега <MAC>: "" для онлайн; резервний фіскальний номер
    ///   для офлайн-документів (T=112/T=111 форму тега визначає білдер).
    /// - include_mac=false: тег <MAC> не додається (для сумісності).
    pub fn build_message(
        &self,
        dat_xml: &str,
        mac_value: Option<&str>,
        mac_id: &str,
        include_mac: bool,
    ) -> Result<String, XmlBuilderError> {
        let dat_xml = canonicalize(dat_xml)?;
        if !include_mac {
            return Ok(format!("{XML_DECLARATION}<RQ V=\"1\">{dat_xml}</RQ>"));
        }
        let mac_value = mac_value.unwrap_or("");
        reconstruct_full_rq(&dat_xml, mac_value, mac_id)
    }
}

/// Витягує DI з канонічного <DAT> — 1:1 Python `_DI_PATTERN`.
pub fn extract_di(dat_xml: &str) -> Option<String> {
    let open = dat_xml.find("<DAT")?;
    let rest = &dat_xml[open..];
    let di_pos = rest.find("DI=\"")?;
    let after = &rest[di_pos + 4..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_cents_matches_python() {
        assert_eq!(to_cents("1.37").unwrap(), 137);
        assert_eq!(to_cents("5.00").unwrap(), 500);
        assert_eq!(to_cents("3.63").unwrap(), 363);
        assert_eq!(to_cents("0.005").unwrap(), 1); // ROUND_HALF_UP
        assert_eq!(to_cents("0.004").unwrap(), 0);
        assert_eq!(to_cents("65.00").unwrap(), 6500);
    }

    #[test]
    fn to_thousandths_matches_python() {
        assert_eq!(to_thousandths("0.370").unwrap(), 370);
        assert_eq!(to_thousandths("2.000").unwrap(), 2000);
        assert_eq!(to_thousandths("1.500").unwrap(), 1500);
        assert_eq!(to_thousandths("1.0005").unwrap(), 1001); // HALF_UP
    }

    #[test]
    fn format_percent_matches_python() {
        assert_eq!(format_percent("20").unwrap(), "20.00");
        assert_eq!(format_percent("20.00").unwrap(), "20.00");
        assert_eq!(format_percent("0").unwrap(), "0.00");
        assert_eq!(format_percent("20.5").unwrap(), "20.50");
        assert_eq!(format_percent("-1").unwrap(), "-1.00");
    }

    #[test]
    fn canonicalize_removes_whitespace_and_sorts_attrs() {
        let out = canonicalize("<C T=\"0\">  <P N=\"1\" C=\"120\" NM=\"Хліб\"/> </C>").unwrap();
        assert_eq!(out, "<C T=\"0\"><P C=\"120\" N=\"1\" NM=\"Хліб\"></P></C>");
    }

    #[test]
    fn canonicalize_escapes_attrs() {
        let out = canonicalize("<C T=\"0\"><P NM=\"Кава &amp; Чай\"></P></C>").unwrap();
        assert_eq!(out, "<C T=\"0\"><P NM=\"Кава &amp; Чай\"></P></C>");
    }

    #[test]
    fn canonicalize_handles_xml_declaration() {
        let out = canonicalize(
            "<?xml version=\"1.0\" encoding=\"windows-1251\"?><C T=\"0\"><P N=\"1\"></P></C>",
        )
        .unwrap();
        assert_eq!(out, "<C T=\"0\"><P N=\"1\"></P></C>");
    }

    #[test]
    fn compute_mac_is_hex_sha256_of_full_rq_cp1251() {
        // MAC = hex(lower) sha256 ПОВНОГО RQ у байтах windows-1251 (спека A/D).
        // Вектор згенеровано з Python-еталона xml_builder.compute_mac.
        let message = "<?xml version=\"1.0\" encoding=\"windows-1251\"?><RQ V=\"1\"><DAT DI=\"1\" FN=\"4538765845\" TN=\"345612052809\" V=\"1\" ZN=\"АА57506761\"><C T=\"0\"></C><TS>20260827120000</TS></DAT><MAC ID=\"\"></MAC></RQ>";
        let mac = compute_mac(message).unwrap();
        assert_eq!(mac.len(), 64);
        assert!(mac.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            mac,
            "d49aeceec6bd80bc0afc080b09b5f0f50b23485298c510adc961e7d02bc7ede2"
        );
        // Порожнє повідомлення (перший документ, ланцюг порожній) — "".
        assert_eq!(compute_mac("").unwrap(), "");
    }

    #[test]
    fn message_chain_no_h_tag_mac_is_previous_rq_hash() {
        // D: <H>-ланцюжка в тілі чеку НЕМАЄ; ланцюг = <MAC> наступного чека =
        // hex sha256 повного RQ попереднього.
        let mut b = XmlBuilder::new("4538765845", "345612052809", "АА57506761", "0", "1", 0, 0);
        let items = [ReceiptItem {
            code: Some("120".into()),
            barcode: None,
            name: "Хліб".into(),
            quantity: "1".into(),
            price: "1.00".into(),
            total: "1.00".into(),
            tax_rate: "0".into(),
        }];
        let payments = [Payment {
            code: "0".into(),
            name: Some("ГОТІВКА".into()),
            amount: "1.00".into(),
            change: None,
        }];
        let totals = Totals {
            fiscal_number: Some(1),
            total: "1.00".into(),
            se: Some("1.00".into()),
            tax_rate: "0".into(),
            tax_percent: Some("20.00".into()),
            tax_total: Some("0.17".into()),
            dtpr: Some("0.00".into()),
            dtsm: Some("0".into()),
            tax_type: Some("0".into()),
            tax_algorithm: Some("0".into()),
            ..Default::default()
        };
        let ts = "20260827120000";

        // c1: перший документ — MAC порожній, тег <MAC ID=""></MAC>
        let dat1 = b
            .build_receipt_xml("0", &items, &payments, &totals, ts, &[], None, None)
            .unwrap();
        assert!(!dat1.contains("<H "), "тіло чеку не містить <H>: {dat1}");
        let m1 = b.build_message(&dat1, Some(""), "", true).unwrap();
        assert!(
            m1.starts_with("<?xml version=\"1.0\" encoding=\"windows-1251\"?><RQ V=\"1\">"),
            "декларація windows-1251 обов'язкова: {m1}"
        );
        assert!(
            m1.contains("<MAC ID=\"\"></MAC>"),
            "перший документ: <MAC ID=\"\"> порожній: {m1}"
        );
        let mac1 = compute_mac(&m1).unwrap();

        // c2: MAC(c2) = hash повного RQ(c1) — без тега <H> у <C>
        let dat2 = b
            .build_receipt_xml("0", &items, &payments, &totals, ts, &[], None, None)
            .unwrap();
        assert!(!dat2.contains("<H "), "тіло чеку не містить <H>: {dat2}");
        let m2 = b.build_message(&dat2, Some(&mac1), "", true).unwrap();
        assert!(
            m2.contains(&format!("<MAC ID=\"\">{mac1}</MAC>")),
            "MAC(c2)=hex sha256 RQ(c1): {m2}"
        );
        let mac2 = compute_mac(&m2).unwrap();
        assert_ne!(mac1, mac2, "новий документ змінює ланцюг");
    }

    #[test]
    fn service_checks_follow_dps_samples() {
        // I: службові за зразками 262576.docx: без <E N="1">; 112 -> <H SIZE>;
        // 111 -> <MAC></MAC> (без ID/значення); 112 -> <MAC> без ID.
        let mut b = XmlBuilder::new("4538765845", "345612052809", "АА57506761", "0", "1", 0, 0);
        let ts = "20260827120000";
        let chain = "d49aeceec6bd80bc0afc080b09b5f0f50b23485298c510adc961e7d02bc7ede2";

        let dat108 = b.build_service_check_xml("108", ts, 150).unwrap();
        assert_eq!(
            dat108,
            "<DAT DI=\"1\" FN=\"4538765845\" TN=\"345612052809\" V=\"1\" ZN=\"АА57506761\"><C T=\"108\"></C><TS>20260827120000</TS></DAT>"
        );
        let m108 = b.build_message(&dat108, Some(chain), "", true).unwrap();
        assert!(m108.contains(&format!("<MAC ID=\"\">{chain}</MAC>")));

        let dat111 = b.build_service_check_xml("111", ts, 150).unwrap();
        let m111 = b.build_message(&dat111, Some(chain), "", true).unwrap();
        assert!(
            m111.contains("<MAC></MAC>"),
            "ping: <MAC></MAC> без ID/значення: {m111}"
        );
        assert!(!m111.contains("<MAC ID"));

        let dat112 = b.build_service_check_xml("112", ts, 150).unwrap();
        assert!(
            dat112.contains("<C T=\"112\"><H SIZE=\"150\"></H></C>"),
            "112: <H SIZE=\"150\">: {dat112}"
        );
        let m112 = b.build_message(&dat112, Some(chain), "", true).unwrap();
        assert!(
            m112.contains(&format!("<MAC>{chain}</MAC>")),
            "112: <MAC> без ID: {m112}"
        );
        assert!(!m112.contains("<MAC ID"));
    }

    #[test]
    fn extract_di_works() {
        let dat = r#"<DAT DI="42" FN="1"></DAT>"#;
        assert_eq!(extract_di(dat).as_deref(), Some("42"));
    }
}

// ─── Парсер підсумків чеку (для Z-звіту) ────────────────────────────────────
// 1:1 Python `parse_receipt_xml_totals` (xml_builder.py).

/// Податкова група чеку з `<E>`/`<TX>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReceiptTax {
    pub percent: rust_decimal::Decimal,
    pub tax_total: rust_decimal::Decimal,
    pub smi: rust_decimal::Decimal,
}

/// Підсумкові дані чеку — 1:1 dict Python `parse_receipt_xml_totals`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReceiptTotals {
    /// T з `<C>`: "0" — продаж, "1" — повернення.
    pub check_type: String,
    /// Сума чеку, грн (SM з `<E>`).
    pub total: rust_decimal::Decimal,
    /// Оплати: (код T, сума грн).
    pub payments: Vec<(String, rust_decimal::Decimal)>,
    /// Податкові групи: (код TX, дані).
    pub taxes: Vec<(String, ReceiptTax)>,
}

fn parse_attrs(tag_body: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let bytes = tag_body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // пропустити пробіли
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // ім'я атрибута
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = &tag_body[name_start..i];
        // пропустити пробіли до '='
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1; // '='
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            continue;
        }
        i += 1; // відкриваюча лапка
        let val_start = i;
        while i < bytes.len() && bytes[i] != b'"' {
            i += 1;
        }
        let value = &tag_body[val_start..i];
        if i < bytes.len() {
            i += 1; // закриваюча лапка
        }
        attrs.push((name.to_string(), value.to_string()));
    }
    attrs
}

/// Збирає тіла тегів `<tag ...>` (без вкладеності) у заданому фрагменті.
fn collect_tag_bodies(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag} ");
    let mut result = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = xml[search_from..].find(&open) {
        let abs = search_from + pos;
        let after_name = abs + open.len();
        let end = xml[after_name..]
            .find('>')
            .map(|e| after_name + e)
            .unwrap_or(xml.len());
        result.push(xml[after_name..end].to_string());
        search_from = end + 1;
    }
    result
}

/// Розбирає канонічний XML чеку `<DAT><C>…</C><TS>…</TS></DAT>` — 1:1
/// Python `parse_receipt_xml_totals` (джерело — фактично відправлений XML).
pub fn parse_receipt_xml_totals(dat_xml: &str) -> Result<ReceiptTotals, XmlBuilderError> {
    if dat_xml.trim().is_empty() {
        return Err(XmlBuilderError::InvalidDecimal("Порожній XML чеку".into()));
    }
    // <C ...>...</C> — прямий дочірній <DAT>; беремо перший.
    let c_start = dat_xml.find("<C ").ok_or_else(|| {
        XmlBuilderError::InvalidDecimal("У пакеті даних відсутній тег <C>".into())
    })?;
    let c_end = dat_xml[c_start..]
        .find("</C>")
        .map(|e| c_start + e)
        .unwrap_or(dat_xml.len());
    let c_body = &dat_xml[c_start..c_end];

    let c_attrs = parse_attrs(&c_body[2..]);
    let check_type = c_attrs
        .iter()
        .find(|(k, _)| k == "T")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "0".to_string());

    let mut total = rust_decimal::Decimal::ZERO;
    let mut payments: Vec<(String, rust_decimal::Decimal)> = Vec::new();
    let mut turnover: Vec<(String, rust_decimal::Decimal)> = Vec::new();
    let mut taxes: Vec<(String, ReceiptTax)> = Vec::new();

    // Оплати (<M>) — у межах <C>
    for m in collect_tag_bodies(c_body, "M") {
        let attrs = parse_attrs(&m);
        let code = attrs
            .iter()
            .find(|(k, _)| k == "T")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "0".into());
        let sm = attrs
            .iter()
            .find(|(k, _)| k == "SM")
            .map(|(_, v)| v.as_str())
            .unwrap_or("0");
        let amount = parse_cents(sm)?;
        if let Some(e) = payments.iter_mut().find(|(k, _)| *k == code) {
            e.1 += amount;
        } else {
            payments.push((code, amount));
        }
    }

    // Позиції продажу/повернення (<P>) — обіг по податкових групах
    for p in collect_tag_bodies(c_body, "P") {
        let attrs = parse_attrs(&p);
        let tx = attrs
            .iter()
            .find(|(k, _)| k == "TX")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "0".into());
        let sm = attrs
            .iter()
            .find(|(k, _)| k == "SM")
            .map(|(_, v)| v.as_str())
            .unwrap_or("0");
        let amount = parse_cents(sm)?;
        if let Some(e) = turnover.iter_mut().find(|(k, _)| *k == tx) {
            e.1 += amount;
        } else {
            turnover.push((tx, amount));
        }
    }

    // Закриття чеку (<E>) та податкові групи (<TX>)
    for e in collect_tag_bodies(c_body, "E") {
        let e_attrs = parse_attrs(&e);
        total += parse_cents(
            e_attrs
                .iter()
                .find(|(k, _)| k == "SM")
                .map(|(_, v)| v.as_str())
                .unwrap_or("0"),
        )?;

        let tx_tags = collect_tag_bodies(&e, "TX");
        if !tx_tags.is_empty() {
            for tx in tx_tags {
                let attrs = parse_attrs(&tx);
                let code = attrs
                    .iter()
                    .find(|(k, _)| k == "TX")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| "0".into());
                let percent = attrs
                    .iter()
                    .find(|(k, _)| k == "TXPR")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("0");
                let tax_total = parse_cents(
                    attrs
                        .iter()
                        .find(|(k, _)| k == "TXSM")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("0"),
                )?;
                taxes.push((
                    code,
                    ReceiptTax {
                        percent: percent_dec(percent)?,
                        tax_total,
                        smi: rust_decimal::Decimal::ZERO,
                    },
                ));
            }
        } else {
            let code = e_attrs
                .iter()
                .find(|(k, _)| k == "TX")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "0".into());
            let percent = e_attrs
                .iter()
                .find(|(k, _)| k == "TXPR")
                .map(|(_, v)| v.as_str())
                .unwrap_or("0");
            let tax_total = parse_cents(
                e_attrs
                    .iter()
                    .find(|(k, _)| k == "TXSM")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("0"),
            )?;
            taxes.push((
                code,
                ReceiptTax {
                    percent: percent_dec(percent)?,
                    tax_total,
                    smi: rust_decimal::Decimal::ZERO,
                },
            ));
        }
    }

    // Додаємо обіг по кожній податковій групі (SMI для Z-звіту) — 1:1 Python
    for (code, tax) in taxes.iter_mut() {
        if let Some((_, t)) = turnover.iter().find(|(k, _)| k == code) {
            tax.smi = *t;
        }
    }

    Ok(ReceiptTotals {
        check_type,
        total,
        payments,
        taxes,
    })
}

/// Копійки ("10000") → гривні Decimal ("100.00") — 1:1 Python `/100`.
fn parse_cents(value: &str) -> Result<rust_decimal::Decimal, XmlBuilderError> {
    let cents = rust_decimal::Decimal::from_str(value)
        .map_err(|e| XmlBuilderError::InvalidDecimal(format!("{value}: {e}")))?;
    Ok(cents / rust_decimal::Decimal::from(100))
}

/// Відсоток ("20.00") → Decimal — 1:1 Python `Decimal(...)`.
fn percent_dec(value: &str) -> Result<rust_decimal::Decimal, XmlBuilderError> {
    rust_decimal::Decimal::from_str(value)
        .map_err(|e| XmlBuilderError::InvalidDecimal(format!("{value}: {e}")))
}
