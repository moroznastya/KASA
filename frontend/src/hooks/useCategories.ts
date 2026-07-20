import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { categoryService } from '@/services/categoryService';
import { CategoryCreate, CategoryUpdate } from '@/types/product';
import toast from 'react-hot-toast';

export function useCategories() {
  return useQuery({
    queryKey: ['categories'],
    queryFn: () => categoryService.getCategories(),
  });
}

export function useCategoryTree() {
  return useQuery({
    queryKey: ['categories-tree'],
    queryFn: () => categoryService.getCategoryTree(),
  });
}

export function useCreateCategory() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: CategoryCreate) => categoryService.createCategory(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['categories'] });
      queryClient.invalidateQueries({ queryKey: ['categories-tree'] });
      toast.success('Категорію створено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка створення категорії');
    },
  });
}

export function useUpdateCategory() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: CategoryUpdate }) =>
      categoryService.updateCategory(id, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['categories'] });
      queryClient.invalidateQueries({ queryKey: ['categories-tree'] });
      toast.success('Категорію оновлено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка оновлення категорії');
    },
  });
}

export function useDeleteCategory() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => categoryService.deleteCategory(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['categories'] });
      queryClient.invalidateQueries({ queryKey: ['categories-tree'] });
      toast.success('Категорію видалено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка видалення категорії');
    },
  });
}
