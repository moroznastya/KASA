export type PrintTemplateType =
  | 'receipt_58mm'
  | 'receipt_80mm'
  | 'return_receipt_58mm'
  | 'fiscal'
  | 'custom'
  | 'price_tag'
  | 'label';

export interface PrintTemplate {
  id: string;
  name: string;
  type: PrintTemplateType;
  content: string;
  variables: TemplateVariable[];
  is_default: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface TemplateVariable {
  key: string;
  label: string;
  default: string;
}

export interface PrintTemplateFormData {
  name: string;
  type: PrintTemplateType;
  content: string;
  is_active: boolean;
}

export interface PrintTemplateRenderData {
  shop_name?: string;
  shop_address?: string;
  shop_phone?: string;
  shop_edrpou?: string;
  receipt_number?: string;
  date?: string;
  time?: string;
  cashier_name?: string;
  original_receipt_number?: string;
  return_reason?: string;
  items?: Array<{
    name: string;
    quantity: number;
    price: number;
    total: number;
  }>;
  total?: number;
  payment_type?: string;
  payment_amount?: number;
  change?: number;
  barcode?: string;
  [key: string]: unknown;
}

// ── Поля для цінників та етикеток ────────────────
export const PRICE_TAG_FIELD_OPTIONS = [
  { value: 'name', label: 'Назва товару' },
  { value: 'price', label: 'Ціна' },
  { value: 'barcode', label: 'Штрих-код' },
  { value: 'article', label: 'Артикул' },
  { value: 'category', label: 'Категорія' },
  { value: 'created_date', label: 'Дата створення' },
] as const;

export const PRICE_TAG_LABEL_MAP: Record<string, string> = {
  name: 'Назва товару',
  price: 'Ціна',
  barcode: 'Штрих-код',
  article: 'Артикул',
  category: 'Категорія',
  created_date: 'Дата створення',
};
