//! Таблиця кодів статусів фіскального сервера ДПС (-1..-16) →
//! (символьне ім'я, український опис). Єдине джерело істини для Rust.
//! 1:1 Python `backend/app/application/use_cases/prro/status_codes.py`.
//!
//! Джерело кодів: `docs/scr/_site_text.txt` (рядки 586-601, 646-661, 675-679),
//! `docs/scr/site.html:1041` («-13 ERROR_NOT_REGISTERED_RRO не зареєстровано ПРРО»).

/// Код статусу ДПС: числовий код + символьне ім'я + зрозумілий опис.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpsStatusCode {
    pub code: i32,
    pub name: &'static str,
    pub description_uk: &'static str,
}

/// Усі відомі коди статусів ДПС.
pub const DPS_STATUS_CODES: &[DpsStatusCode] = &[
    DpsStatusCode {
        code: 1,
        name: "OK",
        description_uk: "Успішно",
    },
    DpsStatusCode {
        code: -1,
        name: "ERROR_VEREFY",
        description_uk: "Помилка перевірки даних",
    },
    DpsStatusCode {
        code: -2,
        name: "ERROR_CHECK",
        description_uk: "Помилка перевірки чека",
    },
    DpsStatusCode {
        code: -3,
        name: "ERROR_SAVE",
        description_uk: "Помилка збереження даних на сервері ДПС",
    },
    DpsStatusCode {
        code: -4,
        name: "ERROR_UNKNOWN",
        description_uk: "Невідома помилка фіскального сервера",
    },
    DpsStatusCode {
        code: -5,
        name: "ERROR_TYPE",
        description_uk: "Неправильний тип чека",
    },
    DpsStatusCode {
        code: -6,
        name: "ERROR_NOT_PREV_ZREPORT",
        description_uk: "Не знайдено попередній Z-звіт",
    },
    DpsStatusCode {
        code: -7,
        name: "ERROR_XML",
        description_uk: "Помилка формування XML",
    },
    DpsStatusCode {
        code: -8,
        name: "ERROR_XML_DATE",
        description_uk: "Помилка дати у XML",
    },
    DpsStatusCode {
        code: -9,
        name: "ERROR_XML_CHK",
        description_uk: "Помилка чека у XML",
    },
    DpsStatusCode {
        code: -10,
        name: "ERROR_XML_ZREPORT",
        description_uk: "Помилка Z-звіту у XML",
    },
    DpsStatusCode {
        code: -11,
        name: "ERROR_OFFLINE_168",
        description_uk: "Пристрій працює офлайн (понад 168 годин)",
    },
    DpsStatusCode {
        code: -12,
        name: "ERROR_BAD_HASH_PREV",
        description_uk: "Невірний хеш попереднього чека",
    },
    DpsStatusCode {
        code: -13,
        name: "ERROR_NOT_REGISTERED_RRO",
        description_uk: "ПРРО не зареєстровано",
    },
    DpsStatusCode {
        code: -14,
        name: "ERROR_NOT_REGISTERED_SIGNER",
        description_uk: "Підписувача не зареєстровано",
    },
    DpsStatusCode {
        code: -15,
        name: "ERROR_NOT_OPEN_SHIFT",
        description_uk: "Зміну не відкрито",
    },
    DpsStatusCode {
        code: -16,
        name: "ERROR_OFFLINE_ID",
        description_uk: "Пристрій офлайн (не отримано ідентифікатор)",
    },
];

/// Запис статусу за кодом.
pub fn dps_status(code: i32) -> Option<&'static DpsStatusCode> {
    DPS_STATUS_CODES.iter().find(|s| s.code == code)
}

/// Ім'я коду статусу: `ERROR_SAVE` / `OK`, для невідомих — `STATUS_{n}`.
pub fn status_name(status: i32) -> String {
    match dps_status(status) {
        Some(s) => s.name.to_string(),
        // Форматування без алокації зайвого: STATUS_{-13}
        None if status < 0 => format!("STATUS_{}", status.abs()),
        None => format!("STATUS_{status}"),
    }
}

/// Опис статусу українською або None для невідомого коду.
pub fn status_description_uk(status: i32) -> Option<&'static str> {
    dps_status(status).map(|s| s.description_uk)
}

/// Повний текст статусу для користувача:
/// `status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)`.
/// Для невідомого коду: `status=999 (STATUS_999: невідомий статус)`.
pub fn status_error_text(status: i32) -> String {
    match dps_status(status) {
        Some(s) => format!("status={} ({}: {})", s.code, s.name, s.description_uk),
        None => format!("status={status} (STATUS_{status}: невідомий статус)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_name_maps_all_known_codes() {
        assert_eq!(status_name(1), "OK");
        assert_eq!(status_name(-1), "ERROR_VEREFY");
        assert_eq!(status_name(-3), "ERROR_SAVE");
        assert_eq!(status_name(-12), "ERROR_BAD_HASH_PREV");
        assert_eq!(status_name(-13), "ERROR_NOT_REGISTERED_RRO");
        assert_eq!(status_name(-16), "ERROR_OFFLINE_ID");
    }

    #[test]
    fn status_name_unknown() {
        assert_eq!(status_name(999), "STATUS_999");
        assert_eq!(status_name(-999), "STATUS_999");
    }

    #[test]
    fn description_uk_known_codes() {
        assert_eq!(status_description_uk(-13), Some("ПРРО не зареєстровано"));
        assert_eq!(status_description_uk(-15), Some("Зміну не відкрито"));
        assert_eq!(
            status_description_uk(-12),
            Some("Невірний хеш попереднього чека")
        );
        assert_eq!(status_description_uk(1), Some("Успішно"));
    }

    #[test]
    fn description_uk_unknown() {
        assert_eq!(status_description_uk(999), None);
    }

    #[test]
    fn status_error_text_includes_name_and_description() {
        let t = status_error_text(-13);
        assert_eq!(
            t,
            "status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)"
        );
        assert!(t.contains("ERROR_NOT_REGISTERED_RRO"));
        assert!(t.contains("ПРРО не зареєстровано"));
    }

    #[test]
    fn status_error_text_unknown() {
        let t = status_error_text(999);
        assert!(t.starts_with("status=999 (STATUS_999:"));
        assert!(t.contains("невідомий статус"));
    }

    #[test]
    fn table_covers_full_dps_range() {
        // docs/scr/_site_text.txt: -1..-16 + OK=1
        for code in -16..=-1 {
            assert!(
                dps_status(code).is_some(),
                "код ДПС {code} відсутній у таблиці"
            );
        }
        assert!(dps_status(1).is_some());
    }
}
