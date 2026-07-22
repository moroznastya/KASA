import api from './api';
import { Receipt } from '@/types/receipt';

export interface Debtor {
  id: string;
  name: string;
  phone: string | null;
  notes: string | null;
  total_debt: number;
  created_at: string;
  updated_at: string;
}

export interface DebtorCreate {
  name: string;
  phone?: string;
  notes?: string;
}

export interface DebtorPayRequest {
  amount: number;
  payment_method?: string;
}

export const debtorService = {
  async list(): Promise<Debtor[]> {
    const response = await api.get<Debtor[]>('/debtors');
    return response.data;
  },

  async search(query: string, limit: number = 10): Promise<Debtor[]> {
    const response = await api.get<Debtor[]>('/debtors/search', {
      params: { query, limit },
    });
    return response.data;
  },

  async create(data: DebtorCreate): Promise<Debtor> {
    const response = await api.post<Debtor>('/debtors', data);
    return response.data;
  },

  async getById(id: string): Promise<Debtor> {
    const response = await api.get<Debtor>(`/debtors/${id}`);
    return response.data;
  },

  async update(id: string, data: Partial<DebtorCreate>): Promise<Debtor> {
    const response = await api.put<Debtor>(`/debtors/${id}`, data);
    return response.data;
  },

  async payDebt(id: string, data: DebtorPayRequest): Promise<Debtor> {
    const response = await api.post<Debtor>(`/debtors/${id}/pay`, data);
    return response.data;
  },

  async getDebtorReceipts(debtorId: string): Promise<Receipt[]> {
    const response = await api.get<Receipt[]>(`/debtors/${debtorId}/receipts`);
    return response.data;
  },
};
