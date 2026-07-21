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

  async createPayment(data: PaymentCreate): Promise<SupplierLedgerEntry> {
    // Використовуємо POST /ledger для створення запису про оплату
    const response = await api.post<SupplierLedgerEntry>('/ledger', {
      supplier_id: data.supplier_id,
      operation_type: 'payment',
      amount: -Math.abs(data.amount), // оплата зменшує борг, тому від'ємна сума
      operation_date: new Date().toISOString(),
      notes: data.notes ? `Оплата: ${data.notes}` : 'Оплата постачальнику',
    });
    return response.data;
  },

  async getPayments(supplierId: string, params?: SearchParams): Promise<PaginatedResponse<SupplierLedgerEntry>> {
    // Історія операцій — це той самий ledger
    return ledgerService.getSupplierLedger(supplierId, params);
  },
};
