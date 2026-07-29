import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { supplierService } from '@/services/supplierService';
import { SupplierCreate, SupplierUpdate } from '@/types/supplier';
import { SearchParams } from '@/types/api';
import { useAuthStore } from '@/store/authStore';
import toast from 'react-hot-toast';

export function useSuppliers(params?: SearchParams) {
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated);

  return useQuery({
    queryKey: ['suppliers', params],
    queryFn: () => supplierService.getSuppliers(params),
    enabled: isAuthenticated,
  });
}

export function useAllSuppliers() {
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated);

  return useQuery({
    queryKey: ['suppliers-all'],
    queryFn: () => supplierService.getAllSuppliers(),
    enabled: isAuthenticated,
  });
}

export function useSupplier(id: string) {
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated);

  return useQuery({
    queryKey: ['supplier', id],
    queryFn: () => supplierService.getSupplier(id),
    enabled: !!id && isAuthenticated,
  });
}

export function useCreateSupplier() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: SupplierCreate) => supplierService.createSupplier(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['suppliers'] });
      toast.success('Постачальника створено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка створення постачальника');
    },
  });
}

export function useUpdateSupplier() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: SupplierUpdate }) =>
      supplierService.updateSupplier(id, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['suppliers'] });
      queryClient.invalidateQueries({ queryKey: ['supplier'] });
      toast.success('Постачальника оновлено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка оновлення постачальника');
    },
  });
}

export function useDeleteSupplier() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => supplierService.deleteSupplier(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['suppliers'] });
      toast.success('Постачальника видалено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка видалення постачальника');
    },
  });
}
