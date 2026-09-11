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
  /** Юрособа/ФОП (ПРРО-вкладка; Етапи 4-6 — заглушка). */
  legal_name?: string | null;
  /** Код ЄДРПОУ/ІПН (ПРРО-вкладка). */
  edrpou?: string | null;
  is_active: boolean;
  created_at: string;
  updated_at?: string;
  /** Роль користувача НА ЦІЙ ТОЧЦІ (з user_stores): owner | store_manager | admin | cashier | manager */
  role: string;
  /** Чи є точка точкою за замовчуванням для користувача */
  is_default: boolean;
  /** Лічильники (адмін-ендпоінти /admin/stores). */
  devices_count?: number;
  workers_count?: number;
}

export interface StoreCreateInput {
  name: string;
  address?: string;
  phone?: string;
  legal_name?: string;
  edrpou?: string;
}

/** Оновлення точки (PUT /admin/stores/:id). */
export interface StoreUpdateInput {
  name: string;
  address?: string | null;
  phone?: string | null;
  legal_name?: string | null;
  edrpou?: string | null;
  is_active?: boolean;
}

/** Працівник точки (GET /admin/stores/:id/workers). */
export interface StoreWorker {
  id: string;
  name: string;
  login: string;
  /** Глобальна роль (users.role). */
  role: string;
  is_active: boolean;
  /** Роль на цій точці (user_stores.role). */
  store_role: string;
  is_default: boolean;
  created_at: string;
  updated_at: string;
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
