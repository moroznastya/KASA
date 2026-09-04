/**
 * Звітність мережі (Етап 4 адмін-панелі, ТЗ 5.5/5.6).
 *
 * Rust endpoints (router_v1.rs — /api/v1/admin/reports/*, під require_admin
 * owner|store_manager|admin; поза store_middleware):
 *   GET /admin/reports/network-sales?from=&to=&limit=
 *   GET /admin/reports/cash-operations?from=&to=
 *   GET /admin/reports/supplier-ledger?from=&to=
 *
 * Гроші — рядки (scale БД, як решта Rust-фасаду): фронтенд форматує через
 * formatCurrency. Кількості — числа.
 */

export interface NetworkSalesStore {
  store_id: string;
  store_name: string;
  is_active: boolean;
  /** Продажі (sale) за період. */
  sales: string;
  /** Повернення (return) за період — додатне число. */
  returns: string;
  /** Нетто = sales - returns. */
  net_sales: string;
  sales_checks: number;
  returns_checks: number;
}

export interface NetworkSalesTotals {
  sales: string;
  returns: string;
  net_sales: string;
  sales_checks: number;
  returns_checks: number;
}

export interface NetworkTopProduct {
  product_id: string;
  product_name: string;
  total: string;
}

export interface NetworkSalesReport {
  from: string | null;
  to: string | null;
  stores: NetworkSalesStore[];
  totals: NetworkSalesTotals;
  top_products: NetworkTopProduct[];
}

export interface CashOperationStore {
  store_id: string;
  store_name: string;
  is_active: boolean;
  deposit: string;
  collection: string;
  operations: number;
}

export interface CashOperationsTotals {
  deposit: string;
  collection: string;
  operations: number;
}

export interface CashOperationsReport {
  from: string | null;
  to: string | null;
  stores: CashOperationStore[];
  totals: CashOperationsTotals;
}

export interface SupplierLedgerRow {
  supplier_id: string;
  supplier_name: string;
  period_operations: number;
  /** amount>0 за період (надходження). */
  period_inflow: string;
  /** |amount| для amount<0 за період (оплати). */
  period_outflow: string;
  period_net: string;
  /** Поточний зведений баланс (balance_after останнього запису журналу). */
  current_balance: string;
  last_operation_date: string | null;
}

export interface SupplierLedgerTotals {
  inflow: string;
  outflow: string;
  net: string;
  balance: string;
}

export interface SupplierLedgerReport {
  from: string | null;
  to: string | null;
  suppliers: SupplierLedgerRow[];
  totals: SupplierLedgerTotals;
}
