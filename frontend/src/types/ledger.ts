export interface SupplierLedgerEntry {
  id: number;
  supplier_id: number;
  supplier_name: string;
  document_type: string;
  document_id: number;
  document_number: string;
  debit: string;
  credit: string;
  balance: string;
  description: string | null;
  created_at: string;
}

export interface BalanceResponse {
  supplier_id: number;
  supplier_name: string;
  balance: string;
}

export interface PaymentCreate {
  supplier_id: number;
  amount: number;
  payment_method: PaymentMethod;
  notes?: string;
}

export type PaymentMethod = 'cash' | 'card' | 'bank_transfer';

export interface Payment {
  id: number;
  supplier_id: number;
  supplier_name: string;
  amount: string;
  payment_method: PaymentMethod;
  notes: string | null;
  created_by: number;
  created_by_name?: string;
  created_at: string;
}
