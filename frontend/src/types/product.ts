export interface Category {
  id: string;
  name: string;
  parent_id: string | null;
  children?: Category[];
  created_at: string;
  updated_at: string;
}

export interface CategoryCreate {
  name: string;
  parent_id?: string | null;
}

export interface CategoryUpdate extends CategoryCreate {
  id: string;
}

export interface Product {
  id: string;
  name: string;
  barcode: string | null;
  article: string | null;
  price: string;
  cost_price: string | null;
  stock: number;
  category_id: string | null;
  category_name?: string;
  supplier_id: string | null;
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
  category_id?: string | null;
  supplier_id?: string | null;
  vat_rate?: VatRate;
  unit?: UnitOfMeasure;
  is_weight?: boolean;
  is_excise?: boolean;
  is_active?: boolean;
}

export interface ProductUpdate extends ProductCreate {
  id: string;
}

export interface BarcodeSearchResult {
  product: Product;
  found: boolean;
}
