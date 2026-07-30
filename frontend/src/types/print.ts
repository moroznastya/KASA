// ── Типи для друку цінників та етикеток ────────────
// ═════════════════════════════════════════════════════════════════════════════

/** Тип штрих-коду / QR */
export type BarcodeType = 'code128' | 'qr';

/** Тип друку */
export type PrintType = 'price_tag' | 'label';

/** Конфігурація типу друку */
export interface PrintTypeConfig {
  id: PrintType;
  label: string;
  description: string;
  defaultSettings: {
    templateId: string;
    widthMm: number;
    heightMm: number;
    gapMm: number;
    marginMm: number;
  };
  templateType: string;
  apiEndpoint: 'renderPriceTags' | 'renderLabels';
}

/** Конфігурація всіх типів друку */
export const PRINT_TYPES: Record<PrintType, PrintTypeConfig> = {
  price_tag: {
    id: 'price_tag',
    label: 'Цінник',
    description: 'Друк цінників на A4',
    defaultSettings: {
      templateId: '',
      widthMm: 40,
      heightMm: 25,
      gapMm: 3,
      marginMm: 10,
    },
    templateType: 'price_tag',
    apiEndpoint: 'renderPriceTags',
  },
  label: {
    id: 'label',
    label: 'Етикетка',
    description: 'Друк етикеток на термопринтер',
    defaultSettings: {
      templateId: '',
      widthMm: 58,
      heightMm: 40,
      gapMm: 2,
      marginMm: 0,
    },
    templateType: 'label',
    apiEndpoint: 'renderLabels',
  },
};

// ═════════════════════════════════════════════════════════════════════════════
// Основи типи (використовуються API)
// ═════════════════════════════════════════════════════════════════════════════

/** Товар для рендеру цінників/етикеток */
export interface PrintProductItem {
  id: string;
  title: string;
  price: string;
  barcode: string;
  article?: string;
  category?: string;
  copies: number;
}

/** Запит на рендер цінників (A4) */
export interface PriceTagRenderRequest {
  template_id: string;
  products: PrintProductItem[];
  width_mm: number;
  height_mm: number;
  gap_mm: number;
  margin_mm: number;
  barcode_type?: BarcodeType;
  barcode_height_mm?: number;
}

/** Відповідь рендеру цінників */
export interface PriceTagRenderResponse {
  html: string;
  total_pages: number;
  total_labels: number;
}

/** Запит на рендер етикеток (термопринтер) */
export interface LabelRenderRequest {
  template_id: string;
  products: PrintProductItem[];
  width_mm: number;
  height_mm: number;
  gap_mm: number;
  barcode_type?: BarcodeType;
  barcode_height_mm?: number;
}

/** Відповідь рендеру етикеток */
export interface LabelRenderResponse {
  html: string;
  total_labels: number;
}

/** Вибраний товар з кількістю копій */
export interface SelectedProduct {
  id: string;
  title: string;
  price: string;
  barcode: string;
  sku: string | null;
  category_id: string | null;
  copies: number;
}

// ═════════════════════════════════════════════════════════════════════════════
// Друк цінників/етикеток з накладної
// ═════════════════════════════════════════════════════════════════════════════

/** Запит на друк цінників/етикеток з товарів накладної */
export interface InvoicePrintRequest {
  print_type: 'price_tag' | 'label';
  only_changed: boolean;
  template_id: string;
  width_mm: number;
  height_mm: number;
  gap_mm: number;
  margin_mm: number;
  barcode_type: 'code128' | 'qr';
  barcode_height_mm: number;
}

/** Відповідь на запит друку з накладної */
export interface InvoicePrintResponse {
  html: string;
  total_labels: number;
  total_pages?: number;
  changed_count?: number;
  total_count: number;
}
