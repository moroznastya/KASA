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

  /** Міжточкова наявність: залишки по всіх точках користувача (read-only). */
  async availability(): Promise<AvailabilityItem[]> {
    const response = await api.get<AvailabilityItem[]>('/inventory/availability');
    return response.data;
  },
};
