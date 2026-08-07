//! Kasa POS — Application layer (етап 1 міграції).
//!
//! Use cases, порти (trait-контракти для інфраструктури), DTO.
//! Залежить ТІЛЬКИ від kasa-domain — жодних Tauri/БД/апаратних залежностей.
//!
//! Етап 1 — довідники READ: [`services::readdirs::ReadDirectoryService`].

pub mod services;

pub use services::readdirs::{ReadDirectoryService, ServiceError};
