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

export const receiptService = {
  async getReceipts(params?: SearchParams): Promise<PaginatedResponse<Receipt>> {
    const response = await api.get<PaginatedResponse<Receipt>>('/receipts', { params });
    return response.data;
  },

  async getReceipt(id: string): Promise<Receipt> {
    const response = await api.get<Receipt>(`/receipts/${id}`);
    return response.data;
  },

  async createReceipt(data: ReceiptCreate): Promise<Receipt> {
    const response = await api.post<Receipt>('/receipts', data);
    return response.data;
  },

  async getTodayStats(): Promise<TodayStats> {
    const response = await api.get<TodayStats>('/receipts/stats/today');
    return response.data;
  },

  // ─── Пошук чеків ──────────────────────────────────────
  async searchReceipts(params: SearchReceiptsParams): Promise<SearchReceiptsResponse> {
    const response = await api.get<SearchReceiptsResponse>('/receipts/search', { params });
    return response.data;
  },

  // ─── Отримати товари чеку ──────────────────────────────
  async getReceiptItems(receiptId: string): Promise<ReceiptItem[]> {
    const response = await api.get<ReceiptItem[]>(`/receipts/${receiptId}/items`);
    return response.data;
  },

  // ─── Останні продажі за штрих-кодом ────────────────────
  async getRecentSalesByProduct(barcode: string, limit?: number): Promise<ProductRecentSalesListResponse> {
    const response = await api.get<ProductRecentSalesListResponse>(
      `/receipts/by-product/${encodeURIComponent(barcode)}/recent-sales`,
      { params: { limit } }
    );
    return response.data;
  },

  // ─── Доступна кількість для повернення ─────────────────
  async getReturnableQuantity(productId: string): Promise<{ product_id: string; returnable: number }> {
    const response = await api.get<{ product_id: string; returnable: number }>(
      `/receipts/products/${productId}/returnable-quantity`
    );
    return response.data;
  },
};
