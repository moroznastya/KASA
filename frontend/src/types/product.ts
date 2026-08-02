export interface Category {
  id: string;
  name: string;
  parent_id: string | null;
  children?: Category[];
  /** v2 API (categories) не повертає ці поля — зроблено опціональними */
  created_at?: string;
  updated_at?: string;
}

export interface CategoryCreate {
  name: string;
  parent_id?: string | null;
}

export interface CategoryUpdate extends CategoryCreate {
  id: string;
}

export interface ProductImage {
  id: string;
  url: string;
  is_main: boolean;
  sort_order: number;
  created_at: string;
}

export interface Barcode {
  id: string;
  barcode: string;
  is_primary: boolean;
  created_at: string;
}

export interface Product {
  id: string;
  title: string;
  barcode: string | null;
  sku: string | null;
  description: string | null;
  price: string;
  cost_price: string | null;
  markup: string | null;
  stock: string;
  recommended_qty: string | null;
  uktzed: string | null;
  scan_excise: boolean;
  tax_rate: string;
  tax_group: string | null;
  is_weight: boolean;
  unit: string;
  category_id: string | null;
  supplier_id: string | null;
  images: ProductImage[];
  barcodes: Barcode[];
  created_at: string;
  updated_at: string;
}

export type VatRate = 0 | 5 | 7 | 20;
export type UnitOfMeasure = 'pcs' | 'kg' | 'l' | 'm' | 'box' | 'pack';

export interface ProductCreate {
  title: string;
  barcode?: string | null;
  sku?: string | null;
  uktzed?: string | null;
  price: number;
  cost_price?: number | null;
  markup?: number | null;
  stock?: number;
  recommended_qty?: number | null;
  category_id?: string | null;
  supplier_id?: string | null;
  tax_rate?: VatRate;
  unit?: UnitOfMeasure;
  is_weight?: boolean;
  scan_excise?: boolean;
}

export interface ProductUpdate extends ProductCreate {
  id: string;
}
