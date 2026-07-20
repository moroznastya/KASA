import api from './api';
import { Receipt, ReceiptCreate } from '@/types/receipt';
import { PaginatedResponse, SearchParams } from '@/types/api';

export interface TodayStats {
  total_sales: number;
  total_returns: number;
  total_profit: number;
  receipts_count: number;
  items_sold: number;
  date: string;
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
};
