export interface Category {
  id: number;
  name: string;
  parent_id: number | null;
  children?: Category[];
  created_at: string;
  updated_at: string;
}

export interface CategoryCreate {
  name: string;
  parent_id?: number | null;
}

export interface CategoryUpdate extends CategoryCreate {
  id: number;
}

export interface Product {
  id: number;
  name: string;
  barcode: string | null;
  article: string | null;
  price: string;
  cost_price: string | null;
  stock: number;
  category_id: number | null;
  category_name?: string;
  supplier_id: number | null;
  supplier_name?: string;
  vat_rate: VatRate;
  unit: UnitOfMeasure;
  is_weight: boolean;
  is_excise: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export type VatRate = 0 | 7 | 20;
export type UnitOfMeasure = 'pcs' | 'kg' | 'l' | 'm' | 'box' | 'pack';

export interface ProductCreate {
  name: string;
  barcode?: string | null;
  article?: string | null;
  price: number;
  cost_price?: number | null;
  stock?: number;
  category_id?: number | null;
  supplier_id?: number | null;
  vat_rate?: VatRate;
  unit?: UnitOfMeasure;
  is_weight?: boolean;
  is_excise?: boolean;
  is_active?: boolean;
}

export interface ProductUpdate extends ProductCreate {
  id: number;
}

export interface BarcodeSearchResult {
  product: Product;
  found: boolean;
}
