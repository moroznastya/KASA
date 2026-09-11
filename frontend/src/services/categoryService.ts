import api from './api';
import { Category, CategoryCreate, CategoryUpdate } from '@/types/product';

// ═════════════════════════════════════════════════════════════════════════════
// API v2 (модуль categories — 6 ендпоінтів)
//
// ✅ v2 сумісний: CategoryListResponse {items, total, page, size},
//    CategoryResponse {id, name, parent_id, description, sort_order, is_active},
//    GET /categories/tree → list[CategoryTreeResponse {id, name, parent_id, children}].
//
// ⚠️ v2 НЕ повертає created_at/updated_at — тип Category оновлено
//    (поля зроблено опціональними, в UI вони не використовуються).
//
// Патерн per-request baseURL — як у prroService (services/prroService.ts).
// ═════════════════════════════════════════════════════════════════════════════

// API_ROOT: у DEV лишається відносний шлях (dev-проксі Vite),
// у production (Tauri/desktop) — АБСОЛЮТНИЙ http://127.0.0.1:8000,
// щоб запити не йшли на tauri://localhost (SPA-fallback → HTML-рядок).
const API_ROOT = import.meta.env.DEV ? '' : 'http://127.0.0.1:8000';
const V2 = { baseURL: `${API_ROOT}/api/v2` } as const;

export const categoryService = {
  async getCategories(): Promise<Category[]> {
    const response = await api.get<{ items: Category[]; total: number }>('/categories', {
      params: { size: 1000 },
      ...V2,
    });
    return response.data.items || [];
  },

  async getCategoryTree(): Promise<Category[]> {
    const response = await api.get<Category[]>('/categories/tree', V2);
    return response.data;
  },

  async getCategory(id: string): Promise<Category> {
    const response = await api.get<Category>(`/categories/${id}`, V2);
    return response.data;
  },

  async createCategory(data: CategoryCreate): Promise<Category> {
    const response = await api.post<Category>('/categories', data, V2);
    return response.data;
  },

  async updateCategory(id: string, data: CategoryUpdate): Promise<Category> {
    const response = await api.put<Category>(`/categories/${id}`, data, V2);
    return response.data;
  },

  async deleteCategory(id: string): Promise<void> {
    await api.delete(`/categories/${id}`, V2);
  },
};
