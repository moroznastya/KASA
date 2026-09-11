//! Сервіси application-шару (етап 1 — довідники READ).
//!
//! Тонкі фасади над портами [`torgashka_domain::ReadDirectories`]:
//! валідація/нормалізація вхідних даних + делегування репозиторію.
//! Залежить ТІЛЬКИ від torgashka-domain.

use torgashka_domain::{
    CategoryDto, DirectoryError, Page, ProductDto, ProductFilters, ReadDirectories, SupplierDto,
};
use uuid::Uuid;

/// Помилки application-шару (обгортка доменних).
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Directory(#[from] DirectoryError),
    #[error(transparent)]
    Write(#[from] torgashka_domain::WriteError),
}

/// Фасад читання довідників. Параметризується реалізацією [`ReadDirectories`].
pub struct ReadDirectoryService<R> {
    repo: R,
}

impl<R: ReadDirectories> ReadDirectoryService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Список товарів з фільтрами (пагінація вбудована у фільтри).
    pub async fn list_products(
        &self,
        filters: &ProductFilters,
    ) -> Result<Page<ProductDto>, ServiceError> {
        Ok(self.repo.list_products(filters).await?)
    }

    /// Список категорій з пагінацією.
    pub async fn list_categories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<CategoryDto>, ServiceError> {
        Ok(self.repo.list_categories(page, size).await?)
    }

    /// Список постачальників з пагінацією.
    pub async fn list_suppliers(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<SupplierDto>, ServiceError> {
        Ok(self.repo.list_suppliers(page, size).await?)
    }

    // ─── Етап 2: читання за ID (CRUD) ──────────────────────────────────────
    pub async fn get_product(&self, id: Uuid) -> Result<ProductDto, ServiceError> {
        Ok(self.repo.get_product(id).await?)
    }

    pub async fn get_product_by_barcode(&self, barcode: &str) -> Result<ProductDto, ServiceError> {
        Ok(self.repo.get_product_by_barcode(barcode).await?)
    }

    pub async fn get_category(&self, id: Uuid) -> Result<CategoryDto, ServiceError> {
        Ok(self.repo.get_category(id).await?)
    }

    pub async fn get_supplier(&self, id: Uuid) -> Result<SupplierDto, ServiceError> {
        Ok(self.repo.get_supplier(id).await?)
    }

    pub async fn list_all_suppliers(&self) -> Result<Vec<SupplierDto>, ServiceError> {
        Ok(self.repo.list_all_suppliers().await?)
    }
}
