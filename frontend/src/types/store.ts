/**
 * Типи для мультиточковості (Етап 4).
 * Відповідають Rust DTO: torgashka-domain/src/stores.rs (StoreDto,
 * StoreCreateInput, UserStoreAssignInput, AvailabilityItemDto).
 */

export interface Store {
  id: string;               // UUID
  name: string;
  address?: string | null;
  phone?: string | null;
  is_active: boolean;
  created_at: string;
  /** Роль користувача НА ЦІЙ ТОЧЦІ (з user_stores): owner | admin | cashier | manager */
  role: string;
  /** Чи є точка точкою за замовчуванням для користувача */
  is_default: boolean;
}

export interface StoreCreateInput {
  name: string;
  address?: string;
  phone?: string;
}

export interface UserStoreAssignInput {
  user_id: string;
  store_id: string;
  role?: string;
  is_default?: boolean;
}

export interface StoreAvailability {
  store_id: string;
  store_name: string;
  /** Decimal у вигляді рядка (Rust Decimal serde) */
  quantity: string;
  price: string;
}

export interface AvailabilityItem {
  product_id: string;
  title: string;
  barcode?: string | null;
  unit?: string | null;
  stores: StoreAvailability[];
}
