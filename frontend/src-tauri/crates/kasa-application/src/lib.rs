//! Kasa POS — Application layer (етап 1 міграції).
//!
//! Use cases, порти (trait-контракти для інфраструктури), DTO.
//! Залежить ТІЛЬКИ від kasa-domain — жодних Tauri/БД/апаратних залежностей.
//!
//! Етап 1 — довідники READ: [`services::readdirs::ReadDirectoryService`].

pub mod services;

pub use services::debtors::DebtorServiceFacade;
pub use services::documents::DocumentsServiceFacade;
pub use services::pos::PosServiceFacade;
pub use services::readdirs::{ReadDirectoryService, ServiceError};
pub use services::write::WriteService;
