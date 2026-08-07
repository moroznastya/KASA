//! Kasa POS — Domain layer (етапи 1–2 міграції).
//!
//! Чистий шар бізнес-логіки: сутності, value objects, доменні події,
//! контракти репозиторіїв. НЕ залежить від Tauri, БД чи апаратного забезпечення.
//!
//! Етап 1 — довідники READ: DTO відповідей (products, categories, suppliers)
//! та trait-контракт [`repos::ReadDirectories`].
//!
//! Етап 2 — CRUD довідників + інвентаризація: write-порти
//! [`write::WriteDirectories`], вхідні структури та розрахунок націнки.

pub mod dto;
pub mod repos;
pub mod write;

/// Типізовані помилки доменного шару (thiserror).
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    /// Ще не реалізовано на поточному етапі міграції.
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

pub use dto::{
    BarcodeDto, CategoryDto, InventoryDto, InventoryItemDto, InventorySummaryDto, Page,
    ProductBriefDto, ProductDto, ProductImageDto, SupplierDto,
};
pub use repos::{DirectoryError, ProductFilters, ReadDirectories};
pub use write::{
    calc_markup, CategoryCreateInput, CategoryUpdateInput, InventoryCountsDto,
    InventoryCreateInput, InventoryItemInput, InventoryUpdateInput, ProductCreateInput,
    ProductUpdateInput, SupplierCreateInput, SupplierUpdateInput, WriteDirectories, WriteError,
};
