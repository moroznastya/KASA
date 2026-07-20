import api from './api';
import { SupplierLedgerEntry, BalanceResponse, PaymentCreate, Payment } from '@/types/ledger';
import { PaginatedResponse, SearchParams } from '@/types/api';

export const ledgerService = {
  async getSupplierBalance(supplierId: string): Promise<BalanceResponse> {
    const response = await api.get<BalanceResponse>(`/ledger/balance/${supplierId}`);
    return response.data;
  },

  async getSupplierLedger(supplierId: string, params?: SearchParams): Promise<PaginatedResponse<SupplierLedgerEntry>> {
    const response = await api.get<PaginatedResponse<SupplierLedgerEntry>>(`/ledger/${supplierId}`, { params });
    return response.data;
  },

  async getAllBalances(): Promise<BalanceResponse[]> {
    const response = await api.get<BalanceResponse[]>('/ledger/balances');
    return response.data;
  },

  async createPayment(data: PaymentCreate): Promise<Payment> {
    const response = await api.post<Payment>('/ledger/payments', data);
    return response.data;
  },

  async getPayments(supplierId: string, params?: SearchParams): Promise<PaginatedResponse<Payment>> {
    const response = await api.get<PaginatedResponse<Payment>>(`/ledger/payments/${supplierId}`, { params });
    return response.data;
  },
};
