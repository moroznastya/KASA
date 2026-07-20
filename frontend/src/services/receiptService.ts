import api from './api';
import { Receipt, ReceiptCreate } from '@/types/receipt';
import { PaginatedResponse, SearchParams } from '@/types/api';

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

  async getTodayStats(): Promise<{ total: string; count: number }> {
    const response = await api.get<{ total: string; count: number }>('/receipts/stats/today');
    return response.data;
  },
};
