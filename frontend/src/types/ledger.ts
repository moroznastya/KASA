export interface SupplierLedgerEntry {
  id: string;
  supplier_id: string;
  operation_type: 'invoice' | 'payment' | 'return' | 'correction';
  document_id: string | null;
  document_number: string | null;
  amount: string;
  balance_after: string;
  operation_date: string;
  notes: string | null;
  created_at: string;
}

export interface BalanceResponse {
  supplier_id: string;
  supplier_name: string;
  current_balance: string;
  last_updated: string | null;
}

export interface InvoiceInfo {
  id: string;
  number: string;
  invoice_date: string;
  total_amount: string;
  paid_amount?: string | null;
  remaining?: string | null;
  supplier_id: string;
  status: string;
}

export interface InvoicePaymentInfo {
  invoice_id: string;
  invoice_number: string;
  invoice_date: string;
  total_amount: string;
  paid_amount: string;
  remaining: string;
}

export interface PaymentCreate {
  supplier_id: string;
  amount: number;
  payment_method: PaymentMethod;
  notes?: string;
  document_id?: string;
  document_number?: string;
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
