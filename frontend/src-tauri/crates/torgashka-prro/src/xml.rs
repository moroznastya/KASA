//! XML СЗЗД 2.1.7 — побудова пакетів даних ПРРО та канонізація (C14N).
//!
//! Байт-ідентичний Python-еталону `backend/.../prro/xml_builder.py`
//! (golden parity: вектори згенеровані з Python, зафіксовані в тестах).
//!
//! Структура повідомлення (розділ 5 протоколу СЗЗД 2.1.7):
//! ```xml
//! <RQ V="1"><DAT FN=".." TN=".." ZN=".." DI=".." V="1">
//!   <C T="0|1">…</C> | <Z …>…</Z> | <C T="108..112"><E N="1"/></C>
//!   <TS>YYYYMMDDhhmmss</TS>
//! </DAT><MAC DI=".." NT="..">Base64</MAC></RQ>
//! ```
//!
//! Канонічний вигляд (Додаток А): атрибути в алфавітному порядку, теги завжди
//! закриті (`<tag></tag>`), пробіли між тегами видаляються.
//! MAC = Base64(SHA-256(канонічний <DAT>)).

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

// ─── MAC ─────────────────────────────────────────────────────────────────────

/// MAC = Base64(SHA-256(канонічний <DAT>)) — 1:1 Python `compute_mac`.
pub fn compute_mac(dat_xml_canonical: &str, key: Option<&[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dat_xml_canonical.as_bytes());
    if let Some(k) = key {
        hasher.update(k);
    }
    let digest = hasher.finalize();
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(digest)
}

/// Дістає NO (номер операції) з XML чека — використовується для H1:
/// lastChk повертає XML останнього чека в data_sign; NO == local_number
/// (Totals.fiscal_number), тому за збігом NO ідентифікуємо "наш" чек.
/// 1:1 Python `extract_check_no`.
pub fn extract_check_no(xml: &str) -> Option<i64> {
    // <E ... NO="123" ...> — атрибут NO тега <E> (номер операції в зміні)
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"<E[^>]*NO="(\d+)""#).expect("валидний регекс NO")
    });
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

    fn next_mac_number(&mut self) -> i64 {
        self.mac_number += 1;
        self.mac_number
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
        prev_hash: Option<&str>, // B1: хеш (MAC) попереднього Check — тег <H> у <C>
    ) -> Result<String, XmlBuilderError> {
        let mut seq = 0i64;
        let mut next_n = || {
            seq += 1;
            seq
        };

        // B1: хеш попереднього Check (СЗЗД 2.1.7, тег <H> — службова інформація,
        // Base64; не друкується; крім ping T=111 та службових 108/109/110/112).
        // H — перша операція чеку (N=1), щоб Python/Rust були байт-ідентичні.
        let h_tag: String = match prev_hash {
            Some(h) if !h.is_empty() => {
                let n = next_n();
                format!("<H N=\"{n}\">{}</H>", esc_text(h))
            }
            _ => String::new(),
        };

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
        body.push_str(&h_tag);
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
    ) -> Result<String, XmlBuilderError> {
        if !SERVICE_TYPES.contains(&service_type) {
            return Err(XmlBuilderError::UnknownServiceType(
                service_type.to_string(),
            ));
        }
        let body = format!(r#"<C T="{service_type}"><E N="1"></E></C>"#);
        let di = self.next_packet_id();
        let dat_xml = self.build_dat(&body, ts, Some(di));
        canonicalize(&dat_xml)
    }

    /// Повне повідомлення <RQ>…</RQ> з <MAC> — 1:1 Python `build_message`.
    pub fn build_message(
        &mut self,
        dat_xml: &str,
        mac_value: Option<&str>,
        include_mac: bool,
    ) -> Result<String, XmlBuilderError> {
        let dat_xml = canonicalize(dat_xml)?;
        // DI з <DAT ... DI="...">
        let di = extract_di(&dat_xml).ok_or(XmlBuilderError::MissingDi)?;

        let mut parts = String::from("<RQ V=\"1\">");
        parts.push_str(&dat_xml);
        if include_mac {
            let mac = match mac_value {
                Some(m) => m.to_string(),
                None => compute_mac(&dat_xml, None),
            };
            let nt = self.next_mac_number();
            let _ = write!(
                parts,
                "<MAC DI=\"{di}\" NT=\"{nt}\">{}</MAC>",
                esc_text(&mac)
            );
        }
        parts.push_str("</RQ>");
        Ok(parts)
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
    fn compute_mac_matches_python() {
        // vector з Python compute_mac на канонічному <DAT> (див. golden vectors)
        let mac = compute_mac(
            r#"<DAT DI="1" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="0"><P C="120" CD="4820000000001" N="1" NM="Хліб" PRC="370" Q="370" SM="137" TX="1"></P><M N="2" NM="ГОТІВКА" RM="363" SM="500" T="0"></M><E DTPR="0.00" DTSM="0" FN="4538765845" N="3" NO="3" SE="114" SM="137" TS="20260807112601" TX="1" TXAL="0" TXPR="20.00" TXSM="23" TXTY="0"></E></C><TS>20260807112601</TS></DAT>"#,
            None,
        );
        assert_eq!(mac, "ts1jV7GpNqH3C28M4Sl8izXtergBzaeXVVSE3gQBYqc=");
    }

    #[test]
    fn hash_chain_inserts_prev_hash_tag() {
        // B1: 3 чеки поспіль — H(c1)→c2, H(c2)→c3 (тег <H> у <C>, СЗЗД 2.1.7).
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

        // c1: без попереднього → без <H>
        let c1 = b
            .build_receipt_xml("0", &items, &payments, &totals, ts, &[], None, None, None)
            .unwrap();
        assert!(!c1.contains("<H "), "c1 не має <H>: {c1}");
        let h1 = compute_mac(&c1, None);

        // c2: H(c1) = MAC(c1) — тег <H N="1"> у <C>
        let c2 = b
            .build_receipt_xml("0", &items, &payments, &totals, ts, &[], None, None, Some(&h1))
            .unwrap();
        assert!(
            c2.contains(&format!("<H N=\"1\">{h1}</H>")),
            "c2 має містити H(c1): {c2}"
        );
        let h2 = compute_mac(&c2, None);
        assert_ne!(h1, h2, "MAC c2 відрізняється від c1 (H змінює DAT)");

        // c3: H(c2) = MAC(c2)
        let c3 = b
            .build_receipt_xml("0", &items, &payments, &totals, ts, &[], None, None, Some(&h2))
            .unwrap();
        assert!(
            c3.contains(&format!("<H N=\"1\">{h2}</H>")),
            "c3 має містити H(c2): {c3}"
        );
        // послідовність N: H=1, P=2, M=3, E=4
        assert!(c3.contains("<P C=\"120\" N=\"2\""));
        assert!(c3.contains("<M N=\"3\""));
        assert!(c3.contains("<E DTPR=\"0.00\" DTSM=\"0\" FN=\"4538765845\" N=\"4\""));
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
