//! ПРРО: sqlx-реалізація репозиторію (зміни + офлайн-черга + налаштування).
//! Таблиці: prro_shifts, prro_queue_items, prro_settings — DDL 1:1 Alembic
//! `578fd283a156_add_prro_settings_shifts_queue` (Python-еталон).

pub mod repository;
pub mod schema;

pub use repository::SqlxPrroRepository;
pub use schema::ensure_prro_schema;
