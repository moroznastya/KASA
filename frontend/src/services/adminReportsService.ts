import api from './api';
import {
  CashOperationsReport,
  NetworkSalesReport,
  SupplierLedgerReport,
} from '@/types/adminReports';

/**
 * «Звітність мережі» (Етап 4 адмін-панелі, ТЗ 5.5/5.6).
 *
 * Серверні endpoints (Rust, router_v1.rs — /api/v1/admin/reports/* поза
 * store_middleware; auth_middleware + require_admin(owner|store_manager|admin)):
 *   GET /admin/reports/network-sales?from=&to=&limit=  → дашборд мережі
 *   GET /admin/reports/cash-operations?from=&to=       → каса мережі
 *   GET /admin/reports/supplier-ledger?from=&to=       → постачальники
 *
 * from/to — YYYY-MM-DD (сервер розширює to до кінця доби). Відсутні
 * параметри = відкритий період (весь журнал).
 */

export interface ReportRangeParams {
  from?: string;
  to?: string;
  limit?: number;
}

function queryString(p: ReportRangeParams): string {
  const q = new URLSearchParams();
  if (p.from) q.set('from', p.from);
  if (p.to) q.set('to', p.to);
  if (p.limit) q.set('limit', String(p.limit));
  const s = q.toString();
  return s ? `?${s}` : '';
}

export const adminReportsService = {
  /** Дашборд мережі: продажі по точках + топ товарів (ТЗ 5.5). */
  async networkSales(p: ReportRangeParams = {}): Promise<NetworkSalesReport> {
    const response = await api.get<NetworkSalesReport>(
      `/admin/reports/network-sales${queryString(p)}`,
    );
    return response.data;
  },

  /** Фінанси/каса мережі: deposit/collection по точках (ТЗ 5.6). */
  async cashOperations(p: ReportRangeParams = {}): Promise<CashOperationsReport> {
    const response = await api.get<CashOperationsReport>(
      `/admin/reports/cash-operations${queryString(p)}`,
    );
    return response.data;
  },

  /** Взаєморозрахунки з постачальниками: зведений баланс (ТЗ 5.6). */
  async supplierLedger(p: ReportRangeParams = {}): Promise<SupplierLedgerReport> {
    const response = await api.get<SupplierLedgerReport>(
      `/admin/reports/supplier-ledger${queryString(p)}`,
    );
    return response.data;
  },
};
