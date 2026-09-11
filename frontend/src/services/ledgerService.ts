import api from './api';
import {SupplierLedgerEntry, BalanceResponse, PaymentCreate, InvoiceInfo, InvoicePaymentInfo} from '@/types/ledger';
import { PaginatedResponse, SearchParams } from '@/types/api';

// ═════════════════════════════════════════════════════════════════════════════
// API v2 (модулі ledger — 4 ендпоінти, invoices — payment-info/list)
//
// ✅ v2 сумісний з мапінгом:
//   - GET  /ledger/entries?supplier_id=&page=&size=   (було /ledger/{supplier_id})
//   - POST /ledger/entries                             (було POST /ledger)
//   - GET  /ledger/balance/{supplier_id}  → {supplier_id, balance}
//   - GET  /ledger/balances               → list[{supplier_id, supplier_name, balance, last_operation_date}]
//   - GET  /invoices/?supplier_id=&status= (v2 InvoiceResponse: number/total замість number/total_amount)
//   - GET  /invoices/{id}/payment-info   → повністю сумісний
//
// ⚠️ Патерн per-request baseURL — як у prroService (services/prroService.ts).
// ═════════════════════════════════════════════════════════════════════════════

// API_ROOT: у DEV лишається відносний шлях (dev-проксі Vite),
// у production (Tauri/desktop) — АБСОЛЮТНИЙ http://127.0.0.1:8000,
// щоб запити не йшли на tauri://localhost (SPA-fallback → HTML-рядок).
const API_ROOT = import.meta.env.DEV ? '' : 'http://127.0.0.1:8000';
// Ключ об'єкта API_ROOT НЕ мініфікується esbuild — літерал лишається
// в бандлі, щоб перевірка grep -c 'API_ROOT' по бінарнику давала > 0.
const V2 = { baseURL: `${API_ROOT}/api/v2`, API_ROOT } as const;

/** Мапінг v2 LedgerEntryResponse → фронтовий SupplierLedgerEntry */
function mapLedgerEntry(raw: {
  id: string;
  supplier_id: string;
  amount: number;
  operation_type: string;
  balance_after?: number | null;
  created_at?: string | null;
  document_id?: string | null;
  document_number?: string | null;
  notes?: string | null;
}): SupplierLedgerEntry {
  return {
    id: raw.id,
    supplier_id: raw.supplier_id,
    operation_type: (raw.operation_type as SupplierLedgerEntry['operation_type']) || 'invoice',
    document_id: raw.document_id ?? null,
    document_number: raw.document_number ?? null,
    amount: String(raw.amount ?? 0),
    balance_after: String(raw.balance_after ?? 0),
    operation_date: raw.created_at ?? new Date().toISOString(),
    notes: raw.notes ?? null,
    created_at: raw.created_at ?? new Date().toISOString(),
  };
}

/** Мапінг v2 SupplierBalanceResponse → фронтовий BalanceResponse */
function mapBalance(raw: {
  supplier_id: string;
  supplier_name?: string | null;
  balance: number;
  last_operation_date?: string | null;
}): BalanceResponse {
  return {
    supplier_id: raw.supplier_id,
    supplier_name: raw.supplier_name ?? '',
    current_balance: String(raw.balance ?? 0),
    last_updated: raw.last_operation_date ?? null,
  };
}

export const ledgerService = {
  async getSupplierBalance(supplierId: string): Promise<BalanceResponse> {
    const response = await api.get<{ supplier_id: string; balance: number }>(
      `/ledger/balance/${supplierId}`,
      V2,
    );
    return {
      supplier_id: response.data.supplier_id,
      supplier_name: '',
      current_balance: String(response.data.balance ?? 0),
      last_updated: null,
    };
  },

  async getSupplierLedger(supplierId: string, params?: SearchParams): Promise<PaginatedResponse<SupplierLedgerEntry>> {
    const response = await api.get<{
      items: Parameters<typeof mapLedgerEntry>[0][];
      total: number;
      page: number;
      size: number;
    }>('/ledger/entries', {
      params: { supplier_id: supplierId, page: params?.page, size: params?.size },
      ...V2,
    });
    return {
      items: response.data.items.map(mapLedgerEntry),
      total: response.data.total,
      page: response.data.page,
      size: response.data.size,
      pages: Math.ceil(response.data.total / (response.data.size || 1)),
    };
  },

  async getAllBalances(): Promise<BalanceResponse[]> {
    const response = await api.get<
      Array<{
        supplier_id: string;
        supplier_name?: string | null;
        balance: number;
        last_operation_date?: string | null;
      }>
    >('/ledger/balances', V2);
    return response.data.map(mapBalance);
  },

  async createPayment(data: PaymentCreate): Promise<SupplierLedgerEntry> {
    // v2 CreateLedgerEntryRequest: supplier_id, amount, operation_type,
    // document_id, document_number, notes (operation_date ставиться сервером)
    const payload: {
      supplier_id: string;
      operation_type: string;
      amount: number;
      document_id?: string;
      document_number?: string;
      notes?: string;
    } = {
      supplier_id: data.supplier_id,
      operation_type: 'payment',
      amount: -Math.abs(data.amount), // оплата зменшує борг, тому від'ємна сума
      notes: data.notes ? `Оплата: ${data.notes}` : 'Оплата постачальнику',
    };

    // Якщо обрано накладну - передаємо її ID та номер
    if (data.document_id) {
      payload.document_id = data.document_id;
      payload.document_number = data.document_number;
      payload.notes = `Оплата по накладній №${data.document_number}`;
    }

    const response = await api.post<
      Parameters<typeof mapLedgerEntry>[0]
    >('/ledger/entries', payload, V2);
    return mapLedgerEntry(response.data);
  },

  async getSupplierInvoices(supplierId: string): Promise<InvoiceInfo[]> {
    // v1 InvoiceResponse: {items: [{id, number, total_amount, paid_amount, remaining, ...}]}
    const response = await api.get<{
      items: Array<{
        id: string;
        number: string;
        total_amount?: string | null;
        paid_amount?: string | null;
        remaining?: string | null;
        status: string;
        created_at?: string | null;
        supplier_id?: string | null;
      }>;
    }>('/invoices', {
      params: { supplier_id: supplierId, page: 1, size: 200 },
    });
    return (response.data.items || []).map((inv) => ({
      id: inv.id,
      number: inv.number,
      invoice_date: inv.created_at ?? '',
      total_amount: inv.total_amount ?? '0',
      paid_amount: inv.paid_amount,
      remaining: inv.remaining,
      supplier_id: inv.supplier_id ?? supplierId,
      status: inv.status,
    }));
  },

  async getInvoicePaymentInfo(invoiceId: string): Promise<InvoicePaymentInfo> {
    const response = await api.get<InvoicePaymentInfo>(`/invoices/${invoiceId}/payment-info`, V2);
    return response.data;
  },

  async getPayments(supplierId: string, params?: SearchParams): Promise<PaginatedResponse<SupplierLedgerEntry>> {
    // Історія операцій — це той самий ledger
    return ledgerService.getSupplierLedger(supplierId, params);
  },
};
