import api from './api';
import { Supplier, SupplierCreate, SupplierUpdate } from '@/types/supplier';
import { PaginatedResponse, SearchParams } from '@/types/api';

export const supplierService = {
  async getSuppliers(params?: SearchParams): Promise<PaginatedResponse<Supplier>> {
    const response = await api.get<PaginatedResponse<Supplier>>('/suppliers', { params });
    return response.data;
  },

  async getAllSuppliers(): Promise<Supplier[]> {
    const response = await api.get<Supplier[]>('/suppliers/all');
    return response.data;
  },

  async getSupplier(id: string): Promise<Supplier> {
    const response = await api.get<Supplier>(`/suppliers/${id}`);
    return response.data;
  },

  async createSupplier(data: SupplierCreate): Promise<Supplier> {
    const response = await api.post<Supplier>('/suppliers', data);
    return response.data;
  },

  async updateSupplier(id: string, data: SupplierUpdate): Promise<Supplier> {
    const response = await api.put<Supplier>(`/suppliers/${id}`, data);
    return response.data;
  },

  async deleteSupplier(id: string): Promise<void> {
    await api.delete(`/suppliers/${id}`);
  },
};
