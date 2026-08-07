//! Сервіси application-шару (етап 1 — довідники READ).
//!
//! Тонкі фасади над портами [`kasa_domain::ReadDirectories`]:
//! валідація/нормалізація вхідних даних + делегування репозиторію.
//! Залежить ТІЛЬКИ від kasa-domain.

use kasa_domain::{
    CategoryDto, DirectoryError, Page, ProductDto, ProductFilters, ReadDirectories, SupplierDto,
};

/// Помилки application-шару (обгортка доменних).
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Directory(#[from] DirectoryError),
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
}
