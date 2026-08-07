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

pub mod auth;
pub mod dto;
pub mod ledger;
pub mod pos;
pub mod repos;
pub mod settings;
pub mod write;

/// Типізовані помилки доменного шару (thiserror).
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    /// Ще не реалізовано на поточному етапі міграції.
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

pub use auth::{
    default_permissions, generate_login_from_name, permission_label, AuthError, AuthService,
    HourlyRateInput, LoginPinRequest, LoginRequest, LoginResult, PermissionsUpdateInput,
    PublicUserDto, SettingDto, SettingUpdateInput, SettingsBatchInput, SettingsModulesDto,
    UserCreateInput, UserDto, UserListDto, UserRole, UserUpdateInput, VerifyDto, ALL_PERMISSIONS,
    CASHIER_PERMISSIONS, PERMISSION_GROUPS,
};
pub use dto::{
    BarcodeDto, CategoryDto, InventoryDto, InventoryItemDto, InventorySummaryDto, Page,
    ProductBriefDto, ProductDto, ProductImageDto, SupplierDto,
};
pub use ledger::{
    LedgerBalanceV1Dto, LedgerBalanceV2Dto, LedgerEntriesQuery, LedgerEntryInput, LedgerEntryV1Dto,
    LedgerEntryV2Dto, LedgerError, LedgerHistoryV1Dto, LedgerListV2Dto, LedgerService,
    SupplierBalanceV2Dto,
};
pub use pos::{
    iso_naive, iso_utc_z, parse_scaled2, parse_scaled3, receipt_total, DocItemInput, MySessionsDto,
    PosError, PosService, ProductBriefInfoDto, ProductRecentSalesDto, PrroShiftDto,
    ReceiptCreateInput, ReceiptDto, ReceiptItemDetailDto, ReceiptItemDto, ReceiptItemInput,
    ReceiptListDto, ReceiptListQuery, ReceiptSearchDto, ReceiptSearchItemDto, ReceiptSearchQuery,
    ReceiptStatsDto, RecentSaleDto, ReturnableQtyDto, ShiftListDto, TransferCreateInput,
    TransferDto, TransferItemDto, TransferListDto, TransferUpdateInput, UserHoursSummaryDto,
    UserSessionsDto, WorkReportDto, WorkSessionDto, WriteOffCreateInput, WriteOffDto,
    WriteOffItemDto, WriteOffListDto, WriteOffUpdateInput,
};
pub use repos::{DirectoryError, ProductFilters, ReadDirectories};
pub use settings::{
    determine_module, determine_value_type, humanize_key, validate_and_normalize_setting_value,
};
pub use write::{
    calc_markup, CategoryCreateInput, CategoryUpdateInput, InventoryCountsDto,
    InventoryCreateInput, InventoryItemInput, InventoryUpdateInput, ProductCreateInput,
    ProductUpdateInput, SupplierCreateInput, SupplierUpdateInput, WriteDirectories, WriteError,
};
