import api from './api';
import { User, PermissionsListResponse } from '@/types/auth';
import { PaginatedResponse } from '@/types/api';

export interface UserCreate {
  name: string;
  login?: string;
  password: string;
  pin_code?: string;
  role: 'admin' | 'cashier';
  is_active?: boolean;
  permissions?: string[];
}

export interface UserUpdate {
  name?: string;
  login?: string;
  password?: string;
  pin_code?: string;
  role?: 'admin' | 'cashier';
  is_active?: boolean;
  permissions?: string[];
}

export const userService = {
  async list(): Promise<User[]> {
    const response = await api.get<{ items: User[] }>('/users');
    return response.data.items;
  },

  async getUsers(params?: { page?: number; size?: number }): Promise<PaginatedResponse<User>> {
    const response = await api.get<PaginatedResponse<User>>('/users', { params });
    return response.data;
  },

  async getById(id: string): Promise<User> {
    const response = await api.get<User>(`/users/${id}`);
    return response.data;
  },

  async create(data: UserCreate): Promise<User> {
    const response = await api.post<User>('/users', data);
    return response.data;
  },

  async update(id: string, data: UserUpdate): Promise<User> {
    const response = await api.put<User>(`/users/${id}`, data);
    return response.data;
  },

  async delete(id: string): Promise<void> {
    await api.delete(`/users/${id}`);
  },

  /**
   * Оновлює права доступу користувача.
   */
  async updatePermissions(id: string, permissions: string[]): Promise<User> {
    const response = await api.put<User>(`/users/${id}/permissions`, { permissions });
    return response.data;
  },

  /**
   * Отримує список всіх доступних прав доступу.
   */
  async getPermissionsList(): Promise<PermissionsListResponse> {
    const response = await api.get<PermissionsListResponse>('/users/permissions/list');
    return response.data;
  },
};
