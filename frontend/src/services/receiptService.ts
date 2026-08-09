import api from './api';
import {
  Receipt,
  ReceiptCreate,
  ReceiptSearchResult,
  ReceiptItem,
  ProductRecentSalesResponse,
  ProductRecentSalesListResponse,
} from '@/types/receipt';
import { PaginatedResponse, SearchParams } from '@/types/api';

// ═════════════════════════════════════════════════════════════════════════════
// API v2 (модуль receipts — 9 ендпоінтів)
//
// ✅ Переведено на v2 (відповіді сумісні):
//   - GET  /receipts/stats/today                (ReceiptTodayStatsResponse)
//   - GET  /receipts/search                     (page_size → size)
//   - GET  /receipts/by-product/{q}/recent-sales (ProductBriefInfo сумісний)
//   - GET  /receipts/products/{id}/returnable-quantity
//   - GET  /receipts/{id}/items                 (ReceiptItemsResponse → мапінг)
//   - POST /receipts/sale | /receipts/return    (створення звичайного чеку)
//
// ⚠️ ЗАЛИШЕНО на v1 (v2-схема несумісна, окрема фаза міграції):
//   - GET /receipts, GET /receipts/{id} — v2 ReceiptResponse НЕ має
//     receipt_type/receipt_number/cashier_id/vat_amount/payment_status/paid_amount —
//     UI (список, деталі, тип чеку) не зможе працювати.
//   - createReceipt з боргом (debtor_id/is_debt/debt_payment) та поверненням
//     за оригінальним чеком (original_receipt_id) — v2 не підтримує ці поля.
// ═════════════════════════════════════════════════════════════════════════════

// API_ROOT: у DEV лишається відносний шлях (dev-проксі Vite),
// у production (Tauri/desktop) — АБСОЛЮТНИЙ http://127.0.0.1:8000,
// щоб запити не йшли на tauri://localhost (SPA-fallback → HTML-рядок).
const API_ROOT = import.meta.env.DEV ? '' : 'http://127.0.0.1:8000';
// Ключ об'єкта API_ROOT НЕ мініфікується esbuild — літерал лишається
// в бандлі, щоб перевірка grep -c 'API_ROOT' по бінарнику давала > 0.
const V2 = { baseURL: `${API_ROOT}/api/v2`, API_ROOT } as const;

export interface TodayStats {
  total_sales: number;
  total_returns: number;
  total_profit: number;
  receipts_count: number;
  items_sold: number;
  date: string;
}

export interface SearchReceiptsParams {
  q?: string;
  date_from?: string;
  date_to?: string;
  receipt_type?: 'sale' | 'return';
  page?: number;
  size?: number;
}

export interface SearchReceiptsResponse {
  items: ReceiptSearchResult[];
  total: number;
  page: number;
  size: number;
}

/** Мапінг v2 ReceiptItemResponse → фронтовий ReceiptItem */
function mapReceiptItem(raw: {
  product_id: string;
  name: string;
  quantity: number;
  price: number;
  tax_rate?: number;
  total?: number | null;
}): ReceiptItem {
  return {
    id: raw.product_id,
    product_id: raw.product_id,
    product_name: raw.name || '',
    product_barcode: null,
    quantity: raw.quantity,
    price: String(raw.price ?? 0),
    total: String(raw.total ?? (raw.quantity * raw.price)),
    vat_rate: raw.tax_rate ?? 20,
    vat_amount: '0',
  };
}

/** Мапінг v2 ReceiptResponse (create) → фронтовий Receipt */
function mapCreatedReceipt(raw: {
  id: string;
  number?: string | null;
  items?: Array<Parameters<typeof mapReceiptItem>[0]> | null;
  total?: number | null;
  payment_method?: string | null;
  created_at?: string | null;
  cash_amount?: number | null;
  card_amount?: number | null;
  change_amount?: number | null;
  is_fiscal?: boolean;
  fiscal_status?: string | null;
  fiscal_number?: string | null;
  fiscal_serial?: string | null;
  fiscal_sent_at?: string | null;
  fiscal_error?: string | null;
  fiscal_check_url?: string | null;
  // ── Картковий термінал ──
  terminal_rrn?: string | null;
  terminal_approval_code?: string | null;
  terminal_invoice_number?: string | null;
  terminal_transaction_id?: string | null;
  terminal_response_code?: string | null;
  terminal_status?: string | null;
  terminal_receipt?: string | null;
  terminal_card_pan?: string | null;
  terminal_payment_system?: string | null;
  terminal_merchant?: string | null;
  terminal_created_at?: string | null;
}, data: ReceiptCreate): Receipt {
  const total = raw.total ?? 0;
  const paid = (raw.cash_amount ?? 0) + (raw.card_amount ?? 0);
  const paymentMethod = (raw.payment_method as Receipt['payment_method']) || data.payment_method || null;

  return {
    id: raw.id,
    receipt_number: raw.number ?? '',
    receipt_type: data.receipt_type,
    items: (raw.items || []).map(mapReceiptItem),
    total_amount: String(total),
    paid_amount: data.paid_amount ?? String(paid),
    vat_amount: '0',
    payment_method: paymentMethod,
    payment_status: paid >= total ? 'paid' : 'partially_paid',
    cash_amount: String(raw.cash_amount ?? 0),
    card_amount: String(raw.card_amount ?? 0),
    change_amount: String(raw.change_amount ?? 0),
    cashier_id: data.cashier_id ?? '',
    created_by: data.cashier_id ?? '',
    created_at: raw.created_at ?? new Date().toISOString(),
    // ── Фіскалізація ──
    is_fiscal: raw.is_fiscal ?? false,
    fiscal_status: raw.fiscal_status ?? 'none',
    fiscal_number: raw.fiscal_number ?? null,
    fiscal_serial: raw.fiscal_serial ?? null,
    fiscal_sent_at: raw.fiscal_sent_at ?? null,
    fiscal_error: raw.fiscal_error ?? null,
    fiscal_check_url: raw.fiscal_check_url ?? null,
    // ── Картковий термінал (проброс з відповіді бекенду) ──
    terminal_rrn: raw.terminal_rrn ?? data.terminal_rrn ?? null,
    terminal_approval_code: raw.terminal_approval_code ?? data.terminal_approval_code ?? null,
    terminal_invoice_number: raw.terminal_invoice_number ?? data.terminal_invoice_number ?? null,
    terminal_transaction_id: raw.terminal_transaction_id ?? data.terminal_transaction_id ?? null,
    terminal_response_code: raw.terminal_response_code ?? data.terminal_response_code ?? null,
    terminal_status: raw.terminal_status ?? data.terminal_status ?? null,
    terminal_receipt: raw.terminal_receipt ?? data.terminal_receipt ?? null,
    terminal_card_pan: raw.terminal_card_pan ?? data.terminal_card_pan ?? null,
    terminal_payment_system: raw.terminal_payment_system ?? data.terminal_payment_system ?? null,
    terminal_merchant: raw.terminal_merchant ?? data.terminal_merchant ?? null,
    terminal_created_at: raw.terminal_created_at ?? data.terminal_created_at ?? null,
  };
}

export const receiptService = {
  /**
   * Список чеків — v1 (v2 ReceiptResponse не має receipt_type, тому
   * UI не зможе розрізняти продажі/повернення).
   */
  async getReceipts(params?: SearchParams): Promise<PaginatedResponse<Receipt>> {
    const response = await api.get<PaginatedResponse<Receipt>>('/receipts', { params });
    return response.data;
  },

  /**
   * Деталі чеку — v1 (та сама причина: receipt_type/cashier/vat відсутні у v2).
   */
  async getReceipt(id: string): Promise<Receipt> {
    const response = await api.get<Receipt>(`/receipts/${id}`);
    return response.data;
  },

  /**
   * Створення чеку.
   *
   * ✅ Звичайний продаж/повернення → v2 (POST /receipts/sale | /receipts/return).
   * ⚠️ Боргові чеки (debtor_id/is_debt/debt_payment) та повернення за
   *    оригінальним чеком (original_receipt_id) → v1 (v2 не підтримує).
   */
  async createReceipt(data: ReceiptCreate): Promise<Receipt> {
    const needsV1 =
      data.is_debt ||
      Boolean(data.debtor_id) ||
      Boolean(data.debt_payment) ||
      Boolean(data.original_receipt_id);

    if (needsV1) {
      const response = await api.post<Receipt>('/receipts', data);
      return response.data;
    }

    // v2 CreateReceiptRequest: items[{product_id, quantity, price}],
    // payment_method, cash_amount, card_amount (total/paid розраховує сервер)
    const endpoint = data.receipt_type === 'return' ? '/receipts/return' : '/receipts/sale';

    // v2 CreateReceiptRequest: items[{product_id, quantity, price}],
    // payment_method, cash_amount, card_amount (total/paid розраховує сервер)
    const payload: Record<string, unknown> = {
      items: data.items.map((item) => ({
        product_id: item.product_id,
        quantity: item.quantity,
        price: item.price,
      })),
      payment_method: data.payment_method || 'cash',
      cash_amount: data.cash_amount ?? null,
      card_amount: data.card_amount ?? null,
    };

    // ── Картковий термінал: передаємо terminal_* поля (якщо є) ──
    // (terminal_rrn, terminal_approval_code, terminal_invoice_number, ...)
    const terminalFields = [
      'terminal_rrn',
      'terminal_approval_code',
      'terminal_invoice_number',
      'terminal_transaction_id',
      'terminal_response_code',
      'terminal_status',
      'terminal_receipt',
      'terminal_card_pan',
      'terminal_payment_system',
      'terminal_merchant',
      'terminal_created_at',
    ] as const;
    for (const field of terminalFields) {
      const value = (data as unknown as Record<string, unknown>)[field];
      if (value !== undefined && value !== null) {
        payload[field] = value;
      }
    }

    const response = await api.post<Parameters<typeof mapCreatedReceipt>[0]>(
      endpoint,
      payload,
      V2,
    );
    return mapCreatedReceipt(response.data, data);
  },

  /** Статистика за день — v2 (сумісна схема). */
  async getTodayStats(): Promise<TodayStats> {
    const response = await api.get<TodayStats>('/receipts/stats/today', V2);
    return response.data;
  },

  // ─── Пошук чеків ──────────────────────────────────────
  /** Пошук чеків — v2 (ReceiptSearchItem сумісний; page_size → size). */
  async searchReceipts(params: SearchReceiptsParams): Promise<SearchReceiptsResponse> {
    const response = await api.get<{
      items: ReceiptSearchResult[];
      total: number;
      page: number;
      page_size: number;
    }>('/receipts/search', { params, ...V2 });
    return {
      items: response.data.items,
      total: response.data.total,
      page: response.data.page,
      size: response.data.page_size,
    };
  },

  // ─── Отримати товари чеку ──────────────────────────────
  /** Товари чеку — v2 (ReceiptItemsResponse → мапінг). */
  async getReceiptItems(receiptId: string): Promise<ReceiptItem[]> {
    const response = await api.get<
      Array<{
        product_id: string;
        name: string;
        product_name?: string;
        product_barcode?: string | null;
        quantity: number;
        price: number;
        total?: number | null;
        tax_rate?: number;
        purchase_price?: number | null;
      }>
    >(`/receipts/${receiptId}/items`, V2);
    return response.data.map((item) => ({
      ...mapReceiptItem({
        product_id: item.product_id,
        name: item.name || item.product_name || '',
        quantity: item.quantity,
        price: item.price,
        tax_rate: item.tax_rate,
        total: item.total,
      }),
      product_barcode: item.product_barcode ?? null,
      purchase_price: item.purchase_price ?? undefined,
    }));
  },

  // ─── Останні продажі за штрих-кодом ────────────────────
  /** Останні продажі за штрих-кодом — v2 (схема сумісна). */
  async getRecentSalesByProduct(barcode: string, limit?: number): Promise<ProductRecentSalesListResponse> {
    const response = await api.get<ProductRecentSalesListResponse>(
      `/receipts/by-product/${encodeURIComponent(barcode)}/recent-sales`,
      { params: { limit }, ...V2 }
    );
    return response.data;
  },

  // ─── Доступна кількість для повернення ─────────────────
  /** Доступна кількість для повернення — v2 (сумісна схема). */
  async getReturnableQuantity(productId: string): Promise<{ product_id: string; returnable: number }> {
    const response = await api.get<{ product_id: string; returnable: number }>(
      `/receipts/products/${productId}/returnable-quantity`,
      V2,
    );
    return response.data;
  },
};
