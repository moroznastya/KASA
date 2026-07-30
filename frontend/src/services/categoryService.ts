import api from './api';
import { Category, CategoryCreate, CategoryUpdate } from '@/types/product';

export const categoryService = {
  async getCategories(): Promise<Category[]> {
    const response = await api.get<{items: Category[]}>('/categories', {
      params: { size: 1000 }
    });
    return response.data.items || [];
  },

  async getCategoryTree(): Promise<Category[]> {
    const response = await api.get<Category[]>('/categories/tree');
    return response.data;
  },

  async getCategory(id: string): Promise<Category> {
    const response = await api.get<Category>(`/categories/${id}`);
    return response.data;
  },

  async createCategory(data: CategoryCreate): Promise<Category> {
    const response = await api.post<Category>('/categories', data);
    return response.data;
  },

  async updateCategory(id: string, data: CategoryUpdate): Promise<Category> {
    const response = await api.put<Category>(`/categories/${id}`, data);
    return response.data;
  },

  async deleteCategory(id: string): Promise<void> {
    await api.delete(`/categories/${id}`);
  },
};
