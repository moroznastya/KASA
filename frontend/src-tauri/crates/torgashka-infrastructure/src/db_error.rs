//! Машинні коди помилок БД для тіл відповідей — БЕЗ сирого тексту PostgreSQL.
//!
//! ## Навіщо модуль існує
//! Обробники мусять віддавати клієнту СТАБІЛЬНИЙ машинний код
//! (`[DB_ERROR <sqlstate>]`), а не локалізований текст PostgreSQL: текст
//! залежить від `lc_messages`, розкриває схему й не придатний для автоматики.
//! Коди читає новий синк-шлях (`sync.rs`, `sync_receivers.rs`) та діагностика
//! джерел БД (`admin_db_sources.rs`).
//!
//! ## Звідки взялося
//! Функції перенесено з видаленого модуля перехоплення помилок read-only
//! репліки (E7, ADR-0008 §8 п.5). Разом із ним жили standby-специфічні речі
//! (типізована помилка «запис у репліку», маркер `[READ_ONLY_REPLICA]`,
//! метрики влучань, фунел `StorePool`): їх видалено разом із концепцією
//! «вузол read-only». Тут лишилися рівно дві функції, які НЕ були
//! standby-специфічними.

use sqlx::Error;

/// SQLSTATE помилки БД, якщо це `sqlx::Error::Database` (напр. `Some("23503")`).
///
/// Потрібен обробникам: дозволяє віддати клієнту СТАБІЛЬНИЙ машинний код
/// замість сирого тексту PostgreSQL. Для не-БД помилок (`Protocol`, `Io`,
/// `RowNotFound`, …) — `None`.
pub fn sqlstate_of(e: &Error) -> Option<String> {
    match e {
        Error::Database(db) => db.code().map(|c| c.to_string()),
        _ => None,
    }
}

/// Стабільний машинний код для тіл відповідей БЕЗ сирого тексту:
/// `[DB_ERROR <sqlstate>]`, а якщо SQLSTATE немає — `[DB_ERROR]`.
///
/// Свідомо без тексту PG: тіло відповіді не має розкривати ні запит, ні схему.
pub fn db_error_class(e: &Error) -> String {
    match sqlstate_of(e) {
        Some(code) => format!("[DB_ERROR {code}]"),
        None => "[DB_ERROR]".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;
    use std::error::Error as StdError;

    use sqlx::error::{DatabaseError, ErrorKind};

    /// Фейкова помилка БД: перевіряємо клас за КОДОМ (без справжнього сервера).
    #[derive(Debug)]
    struct FakeDb {
        /// `None` — фейкова помилка БД БЕЗ SQLSTATE.
        code: Option<&'static str>,
    }

    impl std::fmt::Display for FakeDb {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "db error: текст сервера (не має протікати в тіло)")
        }
    }
    impl StdError for FakeDb {}
    impl DatabaseError for FakeDb {
        fn message(&self) -> &str {
            "текст сервера (не має протікати в тіло)"
        }
        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.map(Cow::Borrowed)
        }
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }
    }

    /// Помилка БД із заданим SQLSTATE.
    fn db_err_msg(code: Option<&'static str>) -> Error {
        Error::Database(Box::new(FakeDb { code }))
    }

    #[test]
    fn sqlstate_and_class_have_no_pg_text() {
        let e = db_err_msg(Some("23503"));
        assert_eq!(sqlstate_of(&e).as_deref(), Some("23503"));
        assert_eq!(db_error_class(&e), "[DB_ERROR 23503]");
        assert!(!db_error_class(&e).contains("текст сервера"));
    }

    #[test]
    fn missing_sqlstate_is_bare() {
        let e = Error::Protocol("вигадана помилка протоколу (не PG)".to_string());
        assert_eq!(sqlstate_of(&e), None);
        assert_eq!(db_error_class(&e), "[DB_ERROR]");
    }

    #[test]
    fn db_error_without_code_is_bare() {
        let e = db_err_msg(None);
        assert_eq!(sqlstate_of(&e), None);
        assert_eq!(db_error_class(&e), "[DB_ERROR]");
    }
}
