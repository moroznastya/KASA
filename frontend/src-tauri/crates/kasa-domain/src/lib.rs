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
pub mod debtors;
pub mod documents;
pub mod dto;
pub mod invoices;
pub mod ledger;
pub mod pos;
pub mod print;
pub mod products_v2;
pub mod purchase_orders;
pub mod repos;
pub mod return_invoices;
pub mod settings;
pub mod suppliers;
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
pub use debtors::{
    DebtorCreateInput, DebtorDto, DebtorError, DebtorListDto, DebtorPayInput, DebtorPaymentDto,
    DebtorReceiptDto, DebtorReceiptItemDto, DebtorSearchQuery, DebtorService, DebtorUpdateInput,
};
pub use documents::{
    BatchConfirmErrorDto, BatchConfirmInput, BatchConfirmResultDto, DocListDto, DocListQuery,
    DocPrintDto, DocumentDto, DocumentsError, DocumentsService, ExportData, ExportQuery,
};
pub use dto::{
    BarcodeDto, CategoryDto, InventoryDto, InventoryItemDto, InventorySummaryDto, Page,
    ProductBriefDto, ProductDto, ProductImageDto, SupplierDto,
};
pub use invoices::{InvoicesError, InvoicesV1Service, InvoicesV2Service};
pub use ledger::{
    LedgerBalanceV1Dto, LedgerBalanceV2Dto, LedgerEntriesQuery, LedgerEntryInput, LedgerEntryV1Dto,
    LedgerEntryV2Dto, LedgerError, LedgerHistoryV1Dto, LedgerListV2Dto, LedgerService,
    SupplierBalanceV2Dto,
};
pub use pos::{
    iso_naive, iso_utc_z, parse_scaled2, parse_scaled3, receipt_total, DebtPaymentInput,
    DocItemInput, MySessionsDto, PosError, PosService, ProductBriefInfoDto, ProductRecentSalesDto,
    PrroShiftDto, ReceiptCreateInput, ReceiptDto, ReceiptItemDetailDto, ReceiptItemDto,
    ReceiptItemInput, ReceiptListDto, ReceiptListQuery, ReceiptSearchDto, ReceiptSearchItemDto,
    ReceiptSearchQuery, ReceiptStatsDto, ReceiptV1CreateInput, ReceiptV1Dto, ReceiptV1ItemDto,
    ReceiptV1ItemInput, ReceiptV1ListDto, ReceiptV1ListQuery, ReceiptV1RecentSalesListDto,
    ReceiptV1SearchDto, ReceiptV1SearchItemDto, RecentSaleDto, ReturnableQtyDto, ShiftListDto,
    TransferCreateInput, TransferDto, TransferItemDto, TransferListDto, TransferUpdateInput,
    UserHoursSummaryDto, UserSessionsDto, WorkReportDto, WorkSessionDto, WriteOffCreateInput,
    WriteOffDto, WriteOffItemDto, WriteOffListDto, WriteOffUpdateInput,
};
pub use print::{PrintError, PrintTemplatesService};
pub use products_v2::{
    BarcodeCreateV2Input, ProductBarcodeV2Dto, ProductCreateV2Input, ProductImageV2Dto,
    ProductListV2Dto, ProductUpdateV2Input, ProductV2Dto, ProductsV2Error, ProductsV2Service,
};
pub use purchase_orders::*;
pub use repos::{DirectoryError, ProductFilters, ReadDirectories};
pub use return_invoices::{ReturnInvoicesError, ReturnInvoicesService};
pub use settings::{
    determine_module, determine_value_type, humanize_key, validate_and_normalize_setting_value,
};
pub use suppliers::{
    SupplierProductItem, SupplierProductMovement, SupplierProductMovementsResponse,
    SupplierProductsResponse,
};
pub use write::{
    calc_markup, CategoryCreateInput, CategoryUpdateInput, InventoryCountsDto,
    InventoryCreateInput, InventoryItemInput, InventoryUpdateInput, ProductCreateInput,
    ProductUpdateInput, SupplierCreateInput, SupplierUpdateInput, WriteDirectories, WriteError,
};
