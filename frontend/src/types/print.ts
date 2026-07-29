// ── Типи для друку цінників та етикеток ────────────

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
