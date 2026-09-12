//! Ключі та предикати ролі інстанса в мережі ADR-0008 (хаб ↔ вузол).
//!
//! Роль інстанса вирішує НАЛАШТУВАННЯ ЙОГО ВЛАСНОЇ БД, а не прапорець чи
//! аргумент запуску: є `sync.hub_url` → це вузол із апстрімом; немає → хаб або
//! одиночна точка (нормальний стан). Так само це робить форвардер
//! (`hub_forwarder.rs`) і `/api/v1/sync/status`.
//!
//! Модуль живе в `infrastructure` (Foundation), бо предикат потрібен ДВОМ
//! шарам:
//!   * API (форвардер node→hub, `torgashka-api`);
//!   * репозиторій користувачів (`repositories/auth.rs`) — локальний маркер
//!     `users.sync_state` (ADR-0008 §7.1-D3): рядок, створений НА ВУЗЛІ, ще не
//!     підтверджений хабом (`pending_hub`), а на хабі/одиночній точці —
//!     канонічний (`confirmed`).
//!
//! Ключі оголошені ТУТ (єдине джерело назв); `hub_forwarder` реекспортує їх,
//! щоб не існувало двох літералів з однією назвою.

use sqlx::{PgPool, Row};

/// Ключ налаштування з URL хаба (у ВЛАСНІЙ БД вузла).
pub const HUB_URL_SETTING: &str = "sync.hub_url";
/// Ключ налаштування з токеном вузла для хаба.
pub const HUB_TOKEN_SETTING: &str = "sync.hub_token";

/// Останнє (за `updated_at`) активне значення налаштування.
///
/// Той самий запит, що `hub_forwarder::read_setting` (реекспорт-делегація): один
/// контракт «що вважати налаштованим» — непорожнє активне значення.
pub async fn setting(pool: &PgPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT value FROM system_settings \
         WHERE key = $1 AND is_active AND value IS NOT NULL AND value <> '' \
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|r| r.try_get::<Option<String>, _>("value").ok().flatten()))
}

/// Чи цей інстанс — вузол із хабом (є налаштування `sync.hub_url`).
///
/// Помилка читання трактується як «немає хаба»: без адреси форвардити нікуди, і
/// створення касира не мусить падати через недоступність налаштувань (маркер
/// тоді лишається `confirmed` — стан одиночної точки).
pub async fn hub_configured(pool: &PgPool) -> bool {
    matches!(setting(pool, HUB_URL_SETTING).await, Ok(Some(_)))
}

/// Прапорець політики входу для касира, ще не підтвердженого хабом
/// (ADR-0008 §10 №2, рішення Творця Б2 від 2026-09-16).
///
/// **За замовчуванням `false`** — вхід дозволено для `sync_state='pending_hub'`.
/// Це збереження offline-first (ADR §2.2): каса мусить працювати, коли хаба
/// немає; касир, створений у точці, не чекає на мережу.
///
/// `REQUIRE_HUB_CONFIRM_BEFORE_LOGIN=true|1|yes|on` вмикає **варіант C**
/// («блок входу до підтвердження хабом») — рішення стає ОБОРОТНИМ: щоб
/// перейти на варіант C, змінюється один прапорець, а не код.
pub fn require_hub_confirm_before_login() -> bool {
    match std::env::var("REQUIRE_HUB_CONFIRM_BEFORE_LOGIN") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}
