//! Kasa POS — Domain layer (етап 1 міграції).
//!
//! Чистий шар бізнес-логіки: сутності, value objects, доменні події,
//! контракти репозиторіїв. НЕ залежить від Tauri, БД чи апаратного забезпечення.
//!
//! Етап 1 — довідники READ: DTO відповідей (products, categories, suppliers)
//! та trait-контракт [`repos::ReadDirectories`].

pub mod dto;
pub mod repos;

/// Типізовані помилки доменного шару (thiserror).
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    /// Ще не реалізовано на поточному етапі міграції.
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

pub use dto::{BarcodeDto, CategoryDto, Page, ProductDto, ProductImageDto, SupplierDto};
pub use repos::{DirectoryError, ProductFilters, ReadDirectories};
