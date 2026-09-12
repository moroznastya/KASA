//! E9 (ADR-0008 §10 №8; рішення Творця 2026-09-16 — **варіант A**): протокол
//! сумісності major-версії схеми хаб↔вузол.
//!
//! Проблема (ADR §5 рядок 29): `schema_revision` — **локальна, технічна**
//! таблиця (кожна БД мігрує себе сама, DDL по мережі не їде). Отже несумісна
//! міграція хаба нікого не зупиняє: вузол зі старою схемою спокійно шле push,
//! хаб його приймає — і дані тихо розходяться. Рішення Творця (варіант A):
//! major-версія схеми зберігається в `schema_revision` (колонки `major`,
//! `minor`), а хаб **відхиляє push** від вузла з іншою major.
//!
//! Що тут (спільне для хаба й вузла, тому — в infrastructure, не в API):
//! * [`SCHEMA_MAJOR`]/[`SCHEMA_MINOR`] — версія схеми, яку очікує ЦЕЙ бінарник
//!   (джерело для запису в `schema_revision` і фолбек, якщо рядка немає);
//! * [`SCHEMA_MAJOR_HEADER`] — як вузол оголошує свою major у push;
//! * [`declared_major`] — розбір оголошеної версії з фолбеком для вузлів, що
//!   випущені ДО E9 (вони заголовка не шлють);
//! * [`schema_major`] — читання major З ВЛАСНОЇ БД інстанса (єдине джерело
//!   істини: те, що записала міграція, а не те, що «пам'ятає» бінарник).
//!
//! ЧОМУ ЗАГОЛОВОК, А НЕ ПОЛЕ В ТІЛІ: тіло `POST /api/v1/sync/push` — голий
//! масив агрегатів (`Vec<PushEnvelope>`), і додати в нього версію можна лише
//! зламавши форму (масив → об'єкт) для ВСІХ наявних клієнтів і тестів. Версія
//! протоколу — метадані рівня запиту, і ручка вже має цю ідіому: `X-Store-Id`
//! (скоуп), `X-Sync-Batch-Id` (штамп батча). Заголовок додається, нічого не
//! ламаючи, а його відсутність має явний, задокументований сенс (нижче).

/// major-версія схеми, яку очікує цей бінарник (E9, варіант A).
///
/// Піднімається РАЗОМ із несумісною зміною схеми (Alembic + DDL `ensure_schema`)
/// — саме це значення записується в `schema_revision.major` і саме його
/// порівнює хаб із тим, що оголосив вузол.
pub const SCHEMA_MAJOR: u32 = 1;

/// minor-версія схеми: сумісні (адитивні) зміни в межах тієї ж major.
/// На прийом push НЕ впливає — зберігається для діагностики й порядку викатки
/// (ADR §10 №8: «мінімально сумісна версія схеми»).
pub const SCHEMA_MINOR: u32 = 0;

/// HTTP-заголовок, яким вузол оголошує major-версію СВОЄЇ схеми у
/// `POST /api/v1/sync/push` (E9, рішення №2 обсягу).
pub const SCHEMA_MAJOR_HEADER: &str = "X-Schema-Major";

/// major, яку приписуємо запиту БЕЗ заголовка (і порожньому).
///
/// Протокол, що існував до E9, не мав поля версії — тобто це протокол
/// major-1. Наслідок для парку вузлів (те саме правило, що й для решти):
/// * хаб на major 1 + старий вузол → усе як раніше (жодного регресу, не
///   «падає мовчки»);
/// * хаб після несумісного підняття major + старий вузол → явна відмова 409
///   з обома версіями й інструкцією, а не тихе псування даних.
pub const UNVERSIONED_MAJOR: u32 = 1;

/// Оголошена вузлом major-версія схеми.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredMajor {
    /// Заголовок присутній і розібраний.
    Explicit(u32),
    /// Заголовка немає (вузол до E9) → [`UNVERSIONED_MAJOR`].
    AssumedUnversioned,
}

impl DeclaredMajor {
    /// Числове значення major для порівняння з хабом.
    pub fn major(self) -> u32 {
        match self {
            DeclaredMajor::Explicit(m) => m,
            DeclaredMajor::AssumedUnversioned => UNVERSIONED_MAJOR,
        }
    }

    /// Чи версію приписано (заголовка не було) — хаб показує це у відмові,
    /// щоб «магія» фолбеку була видима оператору, а не прихована.
    pub fn is_assumed(self) -> bool {
        matches!(self, DeclaredMajor::AssumedUnversioned)
    }
}

/// Розібрати оголошений major із заголовка [`SCHEMA_MAJOR_HEADER`].
///
/// `Ok(AssumedUnversioned)` — заголовка немає/порожній (вузол до E9).
/// `Err` — заголовок є, але не є додатним цілим: це зіпсований запит, і
/// мовчки приписати йому версію було б тим самим «тихим» класом дефекту,
/// проти якого цей протокол і створений.
pub fn declared_major(raw: Option<&str>) -> Result<DeclaredMajor, String> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(DeclaredMajor::AssumedUnversioned);
    };
    match value.parse::<u32>() {
        Ok(major) if major > 0 => Ok(DeclaredMajor::Explicit(major)),
        _ => Err(format!(
            "заголовок {SCHEMA_MAJOR_HEADER} має бути додатним цілим числом \
             (major-версія схеми вузла), отримано: {value:?}"
        )),
    }
}

/// major-версія схеми ЦЬОГО інстанса — з його власної `schema_revision`
/// (джерело істини — те, що записала міграція/DDL, а не константа бінарника).
///
/// Фолбек [`SCHEMA_MAJOR`] — лише коли рядка/таблиці/колонки немає (часткова
/// або ще не мігрована БД): без фолбеку будь-який push у такій БД падав би
/// помилкою БД замість явного рішення про сумісність.
pub async fn schema_major(pool: &sqlx::PgPool) -> u32 {
    let row: Result<Option<i32>, sqlx::Error> =
        sqlx::query_scalar("SELECT major FROM public.schema_revision WHERE id = 1")
            .fetch_optional(pool)
            .await;
    match row {
        Ok(Some(major)) if major > 0 => major as u32,
        _ => SCHEMA_MAJOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_major_absent_or_blank_is_assumed_unversioned() {
        for raw in [None, Some(""), Some("   ")] {
            let d = declared_major(raw).expect("відсутній заголовок — не помилка");
            assert_eq!(d, DeclaredMajor::AssumedUnversioned);
            assert_eq!(d.major(), UNVERSIONED_MAJOR);
            assert!(d.is_assumed());
        }
    }

    #[test]
    fn declared_major_parses_explicit_value_and_trims() {
        for (raw, expected) in [("2", 2u32), (" 3 ", 3), ("1", 1), ("42", 42)] {
            let d = declared_major(Some(raw)).expect("валідне число");
            assert_eq!(d, DeclaredMajor::Explicit(expected), "raw={raw:?}");
            assert!(!d.is_assumed());
        }
    }

    /// Заголовок є, але сміття/нуль — це зіпсований запит, НЕ «вузол до E9»:
    /// приписати йому UNVERSIONED_MAJOR означало б прийняти дані наосліп.
    #[test]
    fn declared_major_garbage_is_error_not_assumption() {
        for raw in ["abc", "0", "-1", "1.5", "2x", "major"] {
            assert!(declared_major(Some(raw)).is_err(), "raw={raw:?}");
        }
    }

    #[test]
    fn schema_constants_are_baseline_major_one() {
        assert_eq!(SCHEMA_MAJOR, 1, "варіант A: базова major схеми — 1");
        assert_eq!(UNVERSIONED_MAJOR, SCHEMA_MAJOR);
        assert_eq!(SCHEMA_MAJOR_HEADER, "X-Schema-Major");
    }
}
