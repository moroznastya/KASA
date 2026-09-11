import api from './api';
import { Store, StoreCreateInput, StoreUpdateInput, StoreWorker } from '@/types/store';
import { User } from '@/types/auth';

/**
 * Адмін-панель власника мережі (Етап 1, ТЗ 5.1–5.3).
 *
 * Серверні endpoints (Rust, router_v1.rs — /api/v1/admin/* поза
 * store_middleware; лише auth_middleware + require_admin(owner|store_manager|admin)):
 *   GET    /api/v1/admin/stores                      → AdminStoreDto[]
 *   POST   /api/v1/admin/stores                      → створити точку (+ юрособа/ЄДРПОУ)
 *   GET    /api/v1/admin/stores/:store_id            → AdminStoreDto
 *   PUT    /api/v1/admin/stores/:store_id            → редагування (у т.ч. is_active)
 *   DELETE /api/v1/admin/stores/:store_id            → АРХІВАЦІЯ (is_active=false;
 *                                                       каси точки архівуються разом)
 *   GET    /api/v1/admin/stores/:store_id/workers    → працівники точки
 *   POST   /api/v1/admin/stores/:store_id/workers    → створити працівника + прив'язка
 *   POST   /api/v1/admin/users/:user_id/deactivate   → деактивація (is_active=false)
 *   POST   /api/v1/admin/users/:user_id/activate     → повторна активація
 *   POST   /api/v1/admin/users/:user_id/reset-password
 *   POST   /api/v1/admin/users/:user_id/reset-pin
 */

export interface ArchiveStoreResult {
  store: Store;
  archived_devices: number;
  warning?: string | null;
}

export const adminService = {
  /** Усі точки мережі (адмін-огляд). */
  async listStores(): Promise<Store[]> {
    const response = await api.get<Store[]>('/admin/stores');
    return response.data;
  },

  /** Деталі точки з лічильниками (каси/працівники). */
  async getStore(storeId: string): Promise<Store> {
    const response = await api.get<Store>(`/admin/stores/${storeId}`);
    return response.data;
  },

  /** Створити точку (автоприв'язка творця як власник). */
  async createStore(data: StoreCreateInput): Promise<Store> {
    const response = await api.post<Store>('/admin/stores', data);
    return response.data;
  },

  /** Редагувати точку (назва/адреса/телефон/юрособа/ЄДРПОУ/статус). */
  async updateStore(storeId: string, data: StoreUpdateInput): Promise<Store> {
    const response = await api.put<Store>(`/admin/stores/${storeId}`, data);
    return response.data;
  },

  /** Архівувати точку (is_active=false; каси архівуються разом). */
  async archiveStore(storeId: string): Promise<ArchiveStoreResult> {
    const response = await api.delete<ArchiveStoreResult>(`/admin/stores/${storeId}`);
    return response.data;
  },

  /** Працівники точки (фільтр по точці — user_stores). */
  async listWorkers(storeId: string): Promise<StoreWorker[]> {
    const response = await api.get<StoreWorker[]>(`/admin/stores/${storeId}/workers`);
    return response.data;
  },

  /** Створити працівника і прив'язати до точки. */
  async createWorker(
    storeId: string,
    data: {
      name: string;
      login?: string;
      password: string;
      pin_code?: string;
      role?: 'store_manager' | 'admin' | 'cashier';
      store_role?: string;
    }
  ): Promise<StoreWorker> {
    const response = await api.post<StoreWorker>(`/admin/stores/${storeId}/workers`, data);
    return response.data;
  },

  /** Деактивація працівника (is_active=false, рядок у БД лишається). */
  async deactivateUser(userId: string): Promise<User> {
    const response = await api.post<User>(`/admin/users/${userId}/deactivate`);
    return response.data;
  },

  /** Повторна активація працівника. */
  async activateUser(userId: string): Promise<User> {
    const response = await api.post<User>(`/admin/users/${userId}/activate`);
    return response.data;
  },

  /** Скинути пароль працівника. */
  async resetPassword(userId: string, password: string): Promise<User> {
    const response = await api.post<User>(`/admin/users/${userId}/reset-password`, { password });
    return response.data;
  },

  /** Скинути PIN-код працівника. */
  async resetPin(userId: string, pinCode: string): Promise<User> {
    const response = await api.post<User>(`/admin/users/${userId}/reset-pin`, { pin_code: pinCode });
    return response.data;
  },
};
