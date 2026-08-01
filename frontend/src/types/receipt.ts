/** Тип статусу фіскалізації чеку */
export type FiscalStatus = 'none' | 'pending' | 'sent' | 'failed';

export interface Receipt {
  id: string;
  receipt_number: string;
  receipt_type: 'sale' | 'return';
  items: ReceiptItem[];
  total_amount: string;
  paid_amount?: string;
  vat_amount: string;
  payment_method: 'cash' | 'card' | 'mixed' | null;
  payment_status: PaymentStatus;
  cash_amount: string;
  card_amount: string;
  change_amount: string;
  cashier_id: string;
  cashier_name?: string;
  created_by: string;
  created_by_name?: string;
  created_at: string;
  total_profit?: number;
  original_receipt_number?: string;   // номер оригінального чеку (для повернення)
  return_reason?: string;              // причина повернення

  // ── Фіскалізація (ПРРО) ─────────────────────────────────────────────
  is_fiscal?: boolean;
  fiscal_status?: FiscalStatus | string;
  fiscal_number?: string | null;
  fiscal_serial?: string | null;
  fiscal_sent_at?: string | null;
  fiscal_error?: string | null;
  /** URL перевірки фіскального чеку (для QR-коду на друку) */
  fiscal_check_url?: string | null;
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
  purchase_price?: number;
  profit?: number;
}

export type PaymentMethod = 'cash' | 'card' | 'mixed';
export type DebtPaymentMethod = 'cash' | 'card' | 'transfer' | 'mixed';
export type PaymentStatus = 'paid' | 'debt' | 'partially_paid';

export interface DebtPaymentInfo {
  debtor_id: string;
  amount: string;
}

export interface ReceiptCreate {
  receipt_number?: string;
  receipt_type: 'sale' | 'return';
  cashier_id?: string;
  total_amount: string;
  paid_amount?: string;
  debtor_id?: string;
  items: ReceiptItemCreate[];
  payment_method?: PaymentMethod;
  cash_amount?: number;
  card_amount?: number;
  is_debt?: boolean;
  debt_payment_method?: string;
  debt_payment?: DebtPaymentInfo;
  original_receipt_id?: string;  // ID оригінального чеку (для повернення)
  return_reason?: string;         // причина повернення
}

export interface ReceiptItemCreate {
  product_id: string;
  quantity: number;
  price: number;
}

// ── Для пошуку чеків ──────────────────────────
export interface ReceiptSearchResult {
  id: string;
  receipt_number: string;
  receipt_type: 'sale' | 'return';
  total_amount: number;
  created_at: string;
  cashier_name: string;
  items_count: number;
}

// ── Для повернення без чеку ────────────────────
export interface RecentSaleInfo {
  receipt_id: string;
  receipt_number: string;
  created_at: string;
  quantity: number;
  price: number;
}

export interface ProductRecentSalesResponse {
  product: {
    id: string;
    title: string;
    barcode: string | null;
    price: number;
    unit: string;
  };
  total_sold: number;
  total_returned: number;
  returnable: number;
  recent_sales: RecentSaleInfo[];
}

export interface ProductRecentSalesListResponse {
  items: ProductRecentSalesResponse[];
  total: number;
}

/** Зручні функції для статусу фіскалізації */
export const FISCAL_STATUS_LABELS: Record<string, string> = {
  none: 'Не фіскалізовано',
  pending: 'Очікує',
  sent: 'Фіскалізовано',
  failed: 'Помилка',
};

export function getFiscalStatusLabel(status?: string | null): string {
  if (!status) return FISCAL_STATUS_LABELS.none;
  return FISCAL_STATUS_LABELS[status] || status;
}
