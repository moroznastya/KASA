export interface Receipt {
  id: number;
  receipt_number: string;
  items: ReceiptItem[];
  total_amount: string;
  vat_amount: string;
  payment_method: PaymentMethod;
  payment_status: PaymentStatus;
  cash_amount: string;
  card_amount: string;
  change_amount: string;
  created_by: number;
  created_by_name?: string;
  created_at: string;
}

export interface ReceiptItem {
  id: number;
  product_id: number;
  product_name: string;
  product_barcode: string | null;
  quantity: number;
  price: string;
  total: string;
  vat_rate: number;
  vat_amount: string;
}

export type PaymentMethod = 'cash' | 'card' | 'mixed';
export type PaymentStatus = 'paid' | 'debt' | 'partially_paid';

export interface ReceiptCreate {
  items: ReceiptItemCreate[];
  payment_method: PaymentMethod;
  cash_amount?: number;
  card_amount?: number;
  is_debt?: boolean;
}

export interface ReceiptItemCreate {
  product_id: number;
  quantity: number;
  price: number;
}
