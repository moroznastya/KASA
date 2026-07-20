import api from './api';
import { Product, ProductCreate, ProductUpdate, BarcodeSearchResult } from '@/types/product';
import { PaginatedResponse, SearchParams } from '@/types/api';

export const productService = {
  async getProducts(params?: SearchParams): Promise<PaginatedResponse<Product>> {
    const response = await api.get<PaginatedResponse<Product>>('/products', { params });
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

  async searchByBarcode(barcode: string): Promise<BarcodeSearchResult> {
    const response = await api.get<BarcodeSearchResult>(`/products/barcode/${barcode}`);
    return response.data;
  },

  async searchProducts(query: string): Promise<Product[]> {
    const response = await api.get<Product[]>('/products/search', {
      params: { q: query },
    });
    return response.data;
  },
};
