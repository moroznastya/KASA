import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { supplierService } from '@/services/supplierService';
import { SupplierCreate, SupplierUpdate } from '@/types/supplier';
import { SearchParams } from '@/types/api';
import toast from 'react-hot-toast';

export function useSuppliers(params?: SearchParams) {
  return useQuery({
    queryKey: ['suppliers', params],
    queryFn: () => supplierService.getSuppliers(params),
  });
}

export function useAllSuppliers() {
  return useQuery({
    queryKey: ['suppliers-all'],
    queryFn: () => supplierService.getAllSuppliers(),
  });
}

export function useSupplier(id: number) {
  return useQuery({
    queryKey: ['supplier', id],
    queryFn: () => supplierService.getSupplier(id),
    enabled: !!id,
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
    mutationFn: ({ id, data }: { id: number; data: SupplierUpdate }) =>
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
    mutationFn: (id: number) => supplierService.deleteSupplier(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['suppliers'] });
      toast.success('Постачальника видалено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка видалення постачальника');
    },
  });
}
