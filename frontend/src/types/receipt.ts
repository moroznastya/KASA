export interface Receipt {
  id: string;
  receipt_number: string;
  receipt_type: 'SALE' | 'RETURN';
  items: ReceiptItem[];
  total_amount: string;
  vat_amount: string;
  payment_method: PaymentMethod;
  payment_status: PaymentStatus;
  cash_amount: string;
  card_amount: string;
  change_amount: string;
  cashier_id: string;
  cashier_name?: string;
  created_by: string;
  created_by_name?: string;
  created_at: string;
}

export interface ReceiptItem {
  id: string;
  product_id: string;
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
  receipt_number: string;
  receipt_type: 'SALE' | 'RETURN';
  cashier_id: string;
  total_amount: string;
  items: ReceiptItemCreate[];
  payment_method?: PaymentMethod;
  cash_amount?: number;
  card_amount?: number;
  is_debt?: boolean;
}

export interface ReceiptItemCreate {
  product_id: string;
  quantity: number;
  price: number;
}
