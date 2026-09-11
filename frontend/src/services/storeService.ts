import api from './api';
import {
  Store,
  StoreCreateInput,
  UserStoreAssignInput,
  AvailabilityItem,
} from '@/types/store';

/**
 * Сервіс торговельних точок (Етап 3/4 мультиточковості).
 * GET/POST /stores, POST /user-stores, GET /inventory/availability
 * (Rust-гілка torgashka-api, router_v1.rs).
 */
export const storeService = {
  /** Список точок користувача (RLS: тільки свої через user_stores). */
  async list(): Promise<Store[]> {
    const response = await api.get<Store[]>('/stores');
    return response.data;
  },

  /** Створити точку (owner) + автоприв'язка творця як owner. */
  async create(data: StoreCreateInput): Promise<Store> {
    const response = await api.post<Store>('/stores', data);
    return response.data;
  },

  /** Призначити користувача на точку (owner). */
  async assignUser(data: UserStoreAssignInput): Promise<Store> {
    const response = await api.post<Store>('/user-stores', data);
    return response.data;
  },

  /**
   * Фізично видалити «порожню» точку (owner).
   *
   * ⚠️ Реальний ендпоінт — POST /admin/stores/:id/delete (require_owner):
   * `DELETE /admin/stores/:id` зайнятий АРХІВАЦІЄЮ (archiveStore, див.
   * коментар у Rust admin.rs + store_delete_e2e.rs). Відповідь 204;
   * 409 з detail «є дані у …», якщо точка не порожня.
   */
  async removeStore(id: string): Promise<void> {
    await api.post(`/admin/stores/${id}/delete`);
  },

  /** Міжточкова наявність: залишки по всіх точках користувача (read-only). */
  async availability(): Promise<AvailabilityItem[]> {
    const response = await api.get<AvailabilityItem[]>('/inventory/availability');
    return response.data;
  },
};
