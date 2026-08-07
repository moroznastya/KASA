//! Kasa POS — Domain layer (етап 0 міграції).
//!
//! Чистий шар бізнес-логіки: сутності, value objects, доменні події,
//! контракти репозиторіїв. НЕ залежить від Tauri, БД чи апаратного забезпечення.
//!
//! На етапі 0 — порожній каркас. Наповнення (money, barcode, quantity,
//! invoice, receipt, product, ledger_entry, aggregates, events, repos,
//! errors) — наступні етапи міграції згідно з docs/RUST_MIGRATION_PLAN.md §2.

/// Типізовані помилки доменного шару (thiserror).
///
/// Заповнюється на етапах 1+ разом із бізнес-логікою.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    /// Ще не реалізовано на поточному етапі міграції.
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}
