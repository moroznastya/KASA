//! Kasa POS — Application layer (етап 0 міграції).
//!
//! Use cases, порти (trait-контракти для інфраструктури), DTO.
//! Залежить ТІЛЬКИ від kasa-domain — жодних Tauri/БД/апаратних залежностей.
//!
//! На етапі 0 — порожній каркас. Наповнення (receipt, ledger, auth, invoice,
//! product, invoice_print, prro) — наступні етапи міграції згідно з
//! docs/RUST_MIGRATION_PLAN.md §2.

/// Порожній публічний тип, щоб крейт мав стабільний API каркаса.
pub struct ApplicationLayer;
