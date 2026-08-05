import api from './api';
import { Product, ProductCreate, ProductUpdate, ProductImage, Barcode } from '@/types/product';
import { PaginatedResponse, SearchParams } from '@/types/api';

/**
 * Фільтрує порожні значення ('' / null / undefined) з параметрів запиту.
 * Запобігає відправці category_id='' — FastAPI валідує UUID-поля
 * і відхиляє порожній рядок з помилкою 422 Unprocessable Entity.
 * Якщо категорія НЕ вибрана — параметр просто не потрапляє у запит.
 */
function cleanParams(params?: SearchParams): Record<string, unknown> | undefined {
  if (!params) return undefined;
  const clean: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(params)) {
    if (value !== '' && value !== null && value !== undefined) {
      clean[key] = value;
    }
  }
  return clean;
}

export const productService = {
  async getProducts(params?: SearchParams): Promise<PaginatedResponse<Product>> {
    const response = await api.get<PaginatedResponse<Product>>('/products', {
      params: cleanParams(params),
    });
    return response.data;
  },

  async getProduct(id: string): Promise<Product> {
    const response = await api.get<Product>(`/products/${id}`);
    return response.data;
  },

  async createProduct(data: ProductCreate): Promise<Product> {
    const response = await api.post<Product>('/products', data);
    return response.data;
  },

  async updateProduct(id: string, data: ProductUpdate): Promise<Product> {
    const response = await api.put<Product>(`/products/${id}`, data);
    return response.data;
  },

  async deleteProduct(id: string): Promise<void> {
    await api.delete(`/products/${id}`);
  },

  async searchByBarcode(barcode: string): Promise<Product> {
    const response = await api.get<Product>(`/products/barcode/${barcode}`);
    return response.data;
  },

  async searchProducts(query: string): Promise<PaginatedResponse<Product>> {
    // Порожній пошук не викликає 422 (guards у викликачах: query.length >= 2),
    // тому логіку не змінюємо — щоб не зламати пошук
    const response = await api.get<PaginatedResponse<Product>>('/products', {
      params: { query },
    });
    return response.data;
  },

  async getProductsByCategory(categoryId: string): Promise<PaginatedResponse<Product>> {
    // Якщо категорія не вибрана ('' / null / undefined) — не робимо запит взагалі
    if (!categoryId) {
      return { items: [], total: 0, page: 1, size: 100, pages: 0 };
    }
    const response = await api.get<PaginatedResponse<Product>>('/products', {
      params: { category_id: categoryId, size: 100 },
    });
    return response.data;
  },

  async uploadImage(productId: string, file: File, isMain: boolean = false): Promise<ProductImage> {
    const formData = new FormData();
    formData.append('file', file);
    formData.append('is_main', String(isMain));
    const response = await api.post<ProductImage>(`/products/${productId}/images`, formData, {
      headers: { 'Content-Type': 'multipart/form-data' },
    });
    return response.data;
  },

  async deleteImage(productId: string, imageId: string): Promise<void> {
    await api.delete(`/products/${productId}/images/${imageId}`);
  },

  async addBarcode(productId: string, barcode: string, isPrimary: boolean = false): Promise<Barcode> {
    const response = await api.post<Barcode>(`/products/${productId}/barcodes`, {
      barcode,
      is_primary: isPrimary,
    });
    return response.data;
  },

  async deleteBarcode(productId: string, barcodeId: string): Promise<void> {
    await api.delete(`/products/${productId}/barcodes/${barcodeId}`);
  },
};
