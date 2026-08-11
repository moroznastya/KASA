//! Спільні десеріалізатори дат для вхідних JSON.
//!
//! Фронтенд надсилає `new Date(...).toISOString()` — рядок з UTC-суфіксом
//! (`2026-08-10T09:00:00.000Z`) або з часовим зсувом (`+03:00`).
//! Rust `NaiveDateTime` не приймає такі рядки напряму → 422.
//! Ці функції нормалізують вхід: відкидають суфікс і обрізають
//! наносекунди (serde `%.f` приймає до 9 цифр, але NaiveDateTime має
//! наносекундну точність — залишаємо як є, головне прибрати `Z`/offset).

use chrono::NaiveDateTime;
use serde::{Deserialize, Deserializer};

/// Розбирає рядок дати: ISO з `Z`/offset або без — у `NaiveDateTime`.
pub fn parse_naive_dt(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Відрізаємо UTC-суфікс 'Z' / 'z'.
    let s = s.trim_end_matches('Z').trim_end_matches('z');
    // Відрізаємо часовий зсув виду +03:00 / +0300 / -05:30.
    let s = strip_tz_offset(s);
    let fmts = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
    ];
    for f in fmts {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
            return Some(dt);
        }
        if f == "%Y-%m-%d" {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, f) {
                return d.and_hms_opt(0, 0, 0);
            }
        }
    }
    None
}

fn strip_tz_offset(s: &str) -> &str {
    // Шукаємо '+' або '-' на позиції >= 10 (після 'YYYY-MM-DD').
    let bytes = s.as_bytes();
    let mut cut = s.len();
    for (i, &b) in bytes.iter().enumerate() {
        if i >= 10 && (b == b'+' || b == b'-') {
            // 'T' або пробіл між датою і часом обов'язковий до зсуву.
            cut = i;
            break;
        }
    }
    &s[..cut]
}

/// `NaiveDateTime` з JSON (обов'язкове поле).
pub fn de_naive_dt<'de, D>(d: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    parse_naive_dt(&s)
        .ok_or_else(|| serde::de::Error::custom(format!("invalid datetime format: '{s}'")))
}

/// `Option<NaiveDateTime>` з JSON (null / відсутнє → None).
pub fn de_opt_naive_dt<'de, D>(d: D) -> Result<Option<NaiveDateTime>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(d)?;
    match opt {
        None => Ok(None),
        Some(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }
            parse_naive_dt(s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid datetime format: '{s}'")))
        }
    }
}
