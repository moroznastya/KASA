export type DocumentType = 'invoice' | 'transfer' | 'write_off' | 'return_invoice' | 'purchase_order' | 'inventory';
export type DocumentStatus = 'draft' | 'confirmed' | 'cancelled';

/** Тип дії при підтвердженні повернення постачальнику */
export type ReturnActionType = 'deduct_from_debt' | 'add_to_cash' | 'exchange';

export interface DocumentItem {
  id: string;
  product_id: string;
  product_name: string;
  product_barcode: string | null;
  quantity: number;
  price: string;
  total: string;
}

export interface Document {
  id: string;
  document_type: DocumentType;
  document_number: string;
  status: DocumentStatus;
  supplier_id: string | null;
  supplier_name?: string;
  from_location: string | null;
  to_location: string | null;
  notes: string | null;
  items: DocumentItem[];
  total_amount: string;
  purchase_total?: number;
  created_by: string;
  created_by_name?: string;
  created_at: string;
  updated_at: string;
  confirmed_at: string | null;
}

export interface DocumentCreate {
  document_type: DocumentType;
  supplier_id?: string | null;
  from_location?: string | null;
  to_location?: string | null;
  notes?: string | null;
  items: DocumentItemCreate[];
}

export interface DocumentItemCreate {
  product_id: string;
  quantity: number;
  price?: number;
}

/** Елемент товару в прибутковій накладній */
export interface InvoiceItemInput {
  product_id: string;
  quantity: number;
  /** Ціна продажу */
  price: number;
  /** Собівартість */
  cost_price?: number;
  /** Відсоток націнки (автоматично розраховується) */
  markup_percent?: number;
  /** Загальна сума */
  total?: number;
}

export interface InvoiceCreate {
  document_type: 'invoice';
  number: string;
  supplier_id: string;
  invoice_date: string;
  payment_method?: 'credit' | 'bank_transfer' | 'cash' | 'other';
  is_fiscal: boolean;
  notes?: string | null;
  items: InvoiceItemInput[];
}

export interface TransferCreate extends DocumentCreate {
  document_type: 'transfer';
  from_location: string;
  to_location: string;
}

export interface WriteOffCreate extends DocumentCreate {
  document_type: 'write_off';
  notes?: string;
}

export interface ReturnInvoiceCreate {
  document_type: 'return_invoice';
  /** Номер документа (якщо не вказано, генерується автоматично) */
  number?: string;
  supplier_id: string;
  return_date: string;
  /** Дія при підтвердженні (за замовчуванням deduct_from_debt) */
  return_action?: ReturnActionType;
  is_fiscal: boolean;
  notes?: string | null;
  items: InvoiceItemInput[];
  /** Товари для обміну (обов'язково, якщо return_action = exchange) */
  exchange_items?: InvoiceItemInput[];
  /** Опціональна прив'язка до прибуткової накладної */
  source_invoice_id?: string | null;
}

/** Замовлення постачальнику */
export interface PurchaseOrderCreate {
  document_type: 'purchase_order';
  /** Номер документа (якщо не вказано, генерується автоматично) */
  number?: string;
  supplier_id: string;
  order_date: string;
  /** Очікувана дата поставки */
  expected_date?: string | null;
  is_fiscal: boolean;
  notes?: string | null;
  items: InvoiceItemInput[];
}

/** Фільтр для розширеного пошуку накладних */
export interface DocumentFilterPreset {
  name: string;
  filters: {
    search: string;
    document_type: string;
    status: string;
    date_from: string;
    date_to: string;
    supplier_id: string;
    amount_from: string;
    amount_to: string;
  };
  created_at: string;
}

/** Запит на масове підтвердження документів */


/** Елемент товару в накладній інвентаризації */
export interface InventoryItemInput {
  product_id: string;
  /** Фактична кількість (вводить користувач) */
  actual_quantity: number;
  /** Облікова кількість (поточний залишок, підтягується з Product.stock) */
  accounting_quantity: number;
  /** Різниця = actual - accounting (розраховується автоматично) */
  difference: number;
}

export interface InventoryCreate {
  document_type: 'inventory';
  /** Номер документа (якщо не вказано, генерується автоматично) */
  number?: string;
  location: string;
  inventory_date: string;
  notes?: string | null;
  items: InventoryItemInput[];
}

export interface BatchConfirmRequest {
  items: Array<{
    id: string;
    document_type: DocumentType;
  }>;
}
