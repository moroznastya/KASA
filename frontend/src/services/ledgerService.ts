import api from './api';
import { SupplierLedgerEntry, BalanceResponse, PaymentCreate, Payment, InvoiceInfo, InvoicePaymentInfo } from '@/types/ledger';
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
    const payload: any = {
      supplier_id: data.supplier_id,
      operation_type: 'payment',
      amount: -Math.abs(data.amount), // оплата зменшує борг, тому від'ємна сума
      operation_date: new Date().toISOString(),
      notes: data.notes ? `Оплата: ${data.notes}` : 'Оплата постачальнику',
    };
    
    // Якщо обрано накладну - передаємо її ID та номер
    if (data.document_id) {
      payload.document_id = data.document_id;
      payload.document_number = data.document_number;
      payload.notes = `Оплата по накладній №${data.document_number}`;
    }
    
    const response = await api.post<SupplierLedgerEntry>('/ledger', payload);
    return response.data;
  },

  async getSupplierInvoices(supplierId: string): Promise<InvoiceInfo[]> {
    const response = await api.get<InvoiceInfo[]>('/invoices/', {
      params: { supplier_id: supplierId, status: 'confirmed' }
    });
    return response.data;
  },

  async getInvoicePaymentInfo(invoiceId: string): Promise<InvoicePaymentInfo> {
    const response = await api.get<InvoicePaymentInfo>(`/invoices/${invoiceId}/payment-info`);
    return response.data;
  },

  async getPayments(supplierId: string, params?: SearchParams): Promise<PaginatedResponse<SupplierLedgerEntry>> {
    // Історія операцій — це той самий ledger
    return ledgerService.getSupplierLedger(supplierId, params);
  },
};
