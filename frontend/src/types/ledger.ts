export interface SupplierLedgerEntry {
  id: string;
  supplier_id: string;
  supplier_name: string;
  document_type: string;
  document_id: string;
  document_number: string;
  debit: string;
  credit: string;
  balance: string;
  description: string | null;
  created_at: string;
}

export interface BalanceResponse {
  supplier_id: string;
  supplier_name: string;
  balance: string;
}

export interface PaymentCreate {
  supplier_id: string;
  amount: number;
  payment_method: PaymentMethod;
  notes?: string;
}

export type PaymentMethod = 'cash' | 'card' | 'bank_transfer';

export interface Payment {
  id: string;
  supplier_id: string;
  supplier_name: string;
  amount: string;
  payment_method: PaymentMethod;
  notes: string | null;
  created_by: string;
  created_by_name?: string;
  created_at: string;
}
