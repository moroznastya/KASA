import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { productService } from '@/services/productService';
import { ProductCreate, ProductUpdate } from '@/types/product';
import { SearchParams } from '@/types/api';
import toast from 'react-hot-toast';

export function useProducts(params?: SearchParams) {
  return useQuery({
    queryKey: ['products', params],
    queryFn: () => productService.getProducts(params),
  });
}

export function useProduct(id: number) {
  return useQuery({
    queryKey: ['product', id],
    queryFn: () => productService.getProduct(id),
    enabled: !!id,
  });
}

export function useCreateProduct() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: ProductCreate) => productService.createProduct(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['products'] });
      toast.success('Товар успішно створено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка при створенні товару');
    },
  });
}

export function useUpdateProduct() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: number; data: ProductUpdate }) =>
      productService.updateProduct(id, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['products'] });
      queryClient.invalidateQueries({ queryKey: ['product'] });
      toast.success('Товар успішно оновлено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка при оновленні товару');
    },
  });
}

export function useDeleteProduct() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) => productService.deleteProduct(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['products'] });
      toast.success('Товар успішно видалено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка при видаленні товару');
    },
  });
}

export function useSearchProducts(query: string) {
  return useQuery({
    queryKey: ['products-search', query],
    queryFn: () => productService.searchProducts(query),
    enabled: query.length >= 2,
  });
}

export function useBarcodeSearch(barcode: string) {
  return useQuery({
    queryKey: ['product-barcode', barcode],
    queryFn: () => productService.searchByBarcode(barcode),
    enabled: barcode.length >= 8,
  });
}
