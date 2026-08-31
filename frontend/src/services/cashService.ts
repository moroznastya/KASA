import api from './api';
import {
  CashOperation,
  CashOperationCreateInput,
  CashOperationsResponse,
} from '@/types/cash';

/** Сервіс касових операцій: внесення та інкасація готівки. */
export const cashService = {
  /** Створити операцію (внесення deposit / інкасація collection). */
  async createOperation(data: CashOperationCreateInput): Promise<CashOperation> {
    const response = await api.post<CashOperation>('/cash-operations', data);
    return response.data;
  },

  /** Журнал операцій + поточні баланси кас (готівка/безготівка). */
  async getOperations(): Promise<CashOperationsResponse> {
    const response = await api.get<CashOperationsResponse>('/cash-operations');
    return response.data;
  },
};
