//! Сервіси application-шару (етап 2 — CRUD довідників + інвентаризація).
//!
//! [`WriteService`] — тонкий фасад над портом [`torgashka_domain::WriteDirectories`].
//! Валідація вхідних даних виконується на рівні API (torgashka-api); тут —
//! лише делегування та конвертація помилок.

use torgashka_domain::{
    CategoryCreateInput, CategoryDto, CategoryUpdateInput, InventoryCountsDto,
    InventoryCreateInput, InventoryDto, InventoryUpdateInput, Page, ProductCreateInput, ProductDto,
    ProductUpdateInput, SupplierCreateInput, SupplierDto, SupplierUpdateInput, WriteDirectories,
};
use uuid::Uuid;

use super::readdirs::ServiceError;

/// Фасад write-операцій. Параметризується реалізацією [`WriteDirectories`].
pub struct WriteService<R> {
    repo: R,
}

impl<R: WriteDirectories> WriteService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    // ─── Products ───────────────────────────────────────────────────────────
    pub async fn create_product(
        &self,
        input: &ProductCreateInput,
    ) -> Result<ProductDto, ServiceError> {
        Ok(self.repo.create_product(input).await?)
    }

    pub async fn update_product(
        &self,
        id: Uuid,
        input: &ProductUpdateInput,
    ) -> Result<ProductDto, ServiceError> {
        Ok(self.repo.update_product(id, input).await?)
    }

    pub async fn delete_product(&self, id: Uuid) -> Result<(), ServiceError> {
        Ok(self.repo.delete_product(id).await?)
    }

    // ─── Categories ─────────────────────────────────────────────────────────
    pub async fn create_category(
        &self,
        input: &CategoryCreateInput,
    ) -> Result<CategoryDto, ServiceError> {
        Ok(self.repo.create_category(input).await?)
    }

    pub async fn update_category(
        &self,
        id: Uuid,
        input: &CategoryUpdateInput,
    ) -> Result<CategoryDto, ServiceError> {
        Ok(self.repo.update_category(id, input).await?)
    }

    pub async fn delete_category(&self, id: Uuid) -> Result<(), ServiceError> {
        Ok(self.repo.delete_category(id).await?)
    }

    // ─── Suppliers ──────────────────────────────────────────────────────────
    pub async fn create_supplier(
        &self,
        input: &SupplierCreateInput,
    ) -> Result<SupplierDto, ServiceError> {
        Ok(self.repo.create_supplier(input).await?)
    }

    pub async fn update_supplier(
        &self,
        id: Uuid,
        input: &SupplierUpdateInput,
    ) -> Result<SupplierDto, ServiceError> {
        Ok(self.repo.update_supplier(id, input).await?)
    }

    pub async fn delete_supplier(&self, id: Uuid) -> Result<(), ServiceError> {
        Ok(self.repo.delete_supplier(id).await?)
    }

    // ─── Inventory ──────────────────────────────────────────────────────────
    pub async fn list_inventories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<InventoryDto>, ServiceError> {
        Ok(self.repo.list_inventories(page, size).await?)
    }

    pub async fn inventory_counts(&self) -> Result<InventoryCountsDto, ServiceError> {
        Ok(self.repo.inventory_counts().await?)
    }

    pub async fn get_inventory(&self, id: Uuid) -> Result<InventoryDto, ServiceError> {
        Ok(self.repo.get_inventory(id).await?)
    }

    pub async fn create_inventory(
        &self,
        input: &InventoryCreateInput,
    ) -> Result<InventoryDto, ServiceError> {
        Ok(self.repo.create_inventory(input).await?)
    }

    pub async fn update_inventory(
        &self,
        id: Uuid,
        input: &InventoryUpdateInput,
    ) -> Result<InventoryDto, ServiceError> {
        Ok(self.repo.update_inventory(id, input).await?)
    }

    pub async fn delete_inventory(&self, id: Uuid) -> Result<(), ServiceError> {
        Ok(self.repo.delete_inventory(id).await?)
    }

    pub async fn confirm_inventory(&self, id: Uuid) -> Result<InventoryDto, ServiceError> {
        Ok(self.repo.confirm_inventory(id).await?)
    }

    pub async fn cancel_inventory(&self, id: Uuid) -> Result<InventoryDto, ServiceError> {
        Ok(self.repo.cancel_inventory(id).await?)
    }
}
