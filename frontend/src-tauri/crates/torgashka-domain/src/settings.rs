//! Settings-валідатор (етап 6): 1:1 з Python `settings_value_validator.py`.
//!
//! Захищає від збереження некоректних значень через PUT /settings/{key}:
//!   - price_tag_width/height, label_width/height → int, 10..200
//!   - price_tag_gap, label_gap                     → int, 0..20
//!   - price_tag_margin                             → int, 0..50
//!   - print_copies                                 → int, 1..100
//!   - barcode_type                                 → whitelist ["code128", "qr"]
//!   - auto_cut_paper, show_logo                    → bool ("true"/"false"/"1"/"0")
//!
//! Значення нормалізується перед зберіганням (int → str, bool → "true"/"false").
//! Помилка → Err(українське повідомлення) → HTTP 422 (як Python).

/// Цілочисельні ключі: key → (мінімум, максимум).
const INT_RANGE_RULES: &[(&str, i64, i64)] = &[
    ("price_tag_width", 10, 200),
    ("price_tag_height", 10, 200),
    ("label_width", 10, 200),
    ("label_height", 10, 200),
    ("price_tag_gap", 0, 20),
    ("label_gap", 0, 20),
    ("price_tag_margin", 0, 50),
    ("print_copies", 1, 100),
];

/// Булеві ключі ("true"/"false"/"1"/"0" → "true"/"false").
const BOOL_KEYS: &[&str] = &["auto_cut_paper", "show_logo"];

/// Whitelist значень.
const WHITELIST_RULES: &[(&str, &[&str])] = &[("barcode_type", &["code128", "qr"])];

/// Відомий список ключів друку (для визначення module='printing').
pub const PRINTING_KEYS: &[&str] = &[
    "printer_name",
    "print_font_family",
    "default_template_type",
    "print_copies",
    "auto_cut_paper",
    "show_logo",
    "return_receipt_template_type",
    "receipt_print_copies",
    "report_print_copies",
    "price_tag_fields",
    "price_tag_width",
    "price_tag_height",
    "label_fields",
    "label_width",
    "label_height",
    "price_tag_gap",
    "label_gap",
    "price_tag_margin",
    "barcode_type",
    "price_tag_template_id",
    "label_template_id",
];

/// Валідує та нормалізує значення налаштування за ключем.
///
/// Повертає Ok(нормалізоване значення) або Err(повідомлення для 422 detail).
pub fn validate_and_normalize_setting_value(
    key: &str,
    value: Option<&str>,
) -> Result<Option<String>, String> {
    // None дозволено — зберігається як NULL (без валідації).
    let Some(raw) = value else {
        return Ok(None);
    };
    let stripped = raw.trim();

    // ── Цілочисельні налаштування ─────────────────────────────────────────
    if let Some(&(_, min_v, max_v)) = INT_RANGE_RULES.iter().find(|(k, _, _)| *k == key) {
        let int_value: i64 = stripped.parse().map_err(|_| {
            format!("Налаштування '{key}' має бути цілим числом від {min_v} до {max_v}.")
        })?;
        if !(min_v..=max_v).contains(&int_value) {
            return Err(format!(
                "Налаштування '{key}' має бути в діапазоні від {min_v} до {max_v}."
            ));
        }
        return Ok(Some(int_value.to_string()));
    }

    // ── Булеві налаштування ───────────────────────────────────────────────
    if BOOL_KEYS.contains(&key) {
        let normalized = stripped.to_lowercase();
        if normalized == "true" || normalized == "1" {
            return Ok(Some("true".to_string()));
        }
        if normalized == "false" || normalized == "0" {
            return Ok(Some("false".to_string()));
        }
        return Err(format!(
            "Налаштування '{key}' має бути булевим значенням: true, false, 1 або 0."
        ));
    }

    // ── Whitelist значень ─────────────────────────────────────────────────
    if let Some(&(_, allowed)) = WHITELIST_RULES.iter().find(|(k, _)| *k == key) {
        if !allowed.contains(&stripped) {
            let allowed_str = allowed
                .iter()
                .map(|a| format!("'{a}'"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "Налаштування '{key}' має бути одним із значень: {allowed_str}."
            ));
        }
        return Ok(Some(stripped.to_string()));
    }

    // ── Інші ключі — без обмежень ─────────────────────────────────────────
    Ok(Some(raw.to_string()))
}

/// Визначає модуль налаштування автоматично за ключем (Python _determine_module).
pub fn determine_module(key: &str) -> String {
    if key.starts_with("price_tag_")
        || key.starts_with("label_")
        || key.starts_with("print_")
        || PRINTING_KEYS.contains(&key)
    {
        "printing".to_string()
    } else {
        "general".to_string()
    }
}

/// Визначає тип значення автоматично (Python _determine_value_type).
pub fn determine_value_type(value: Option<&str>) -> String {
    let Some(v) = value else {
        return "string".to_string();
    };
    let stripped = v.trim();
    if stripped.eq_ignore_ascii_case("true") || stripped.eq_ignore_ascii_case("false") {
        return "boolean".to_string();
    }
    if stripped
        .trim_start_matches('-')
        .chars()
        .all(|c| c.is_ascii_digit())
        && !stripped.is_empty()
    {
        return "number".to_string();
    }
    if stripped.starts_with('[') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(stripped) {
            if parsed.is_array() {
                return "string".to_string();
            }
        }
    }
    "string".to_string()
}

/// Людино-зрозуміла назва ключа (Python _humanize_key).
pub fn humanize_key(key: &str) -> String {
    let mut chars = key.chars();
    let mut out = String::new();
    if let Some(first) = chars.next() {
        out.push(first.to_ascii_uppercase());
    }
    out.push_str(&key[first_byte_len(key)..].replace('_', " "));
    out
}

fn first_byte_len(s: &str) -> usize {
    s.chars().next().map(|c| c.len_utf8()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_rules_normalize_and_range() {
        assert_eq!(
            validate_and_normalize_setting_value("print_copies", Some("5")).unwrap(),
            Some("5".into())
        );
        assert!(validate_and_normalize_setting_value("print_copies", Some("5000")).is_err());
        assert!(validate_and_normalize_setting_value("price_tag_width", Some("-5")).is_err());
        assert!(validate_and_normalize_setting_value("price_tag_width", Some("abc")).is_err());
    }

    #[test]
    fn bool_rules() {
        assert_eq!(
            validate_and_normalize_setting_value("auto_cut_paper", Some("1")).unwrap(),
            Some("true".into())
        );
        assert_eq!(
            validate_and_normalize_setting_value("show_logo", Some("FALSE")).unwrap(),
            Some("false".into())
        );
        assert!(validate_and_normalize_setting_value("auto_cut_paper", Some("maybe")).is_err());
    }

    #[test]
    fn whitelist() {
        assert_eq!(
            validate_and_normalize_setting_value("barcode_type", Some("qr")).unwrap(),
            Some("qr".into())
        );
        assert!(validate_and_normalize_setting_value("barcode_type", Some("pdf417")).is_err());
    }

    #[test]
    fn module_and_value_type() {
        assert_eq!(determine_module("price_tag_width"), "printing");
        assert_eq!(determine_module("company_name"), "general");
        assert_eq!(determine_value_type(Some("true")), "boolean");
        assert_eq!(determine_value_type(Some("42")), "number");
        assert_eq!(determine_value_type(Some("[\"1\",\"10\"]")), "string");
        assert_eq!(determine_value_type(Some("text")), "string");
    }
}
