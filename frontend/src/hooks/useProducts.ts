import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { productService } from '@/services/productService';
import { ProductCreate, ProductUpdate, Product } from '@/types/product';
import { SearchParams } from '@/types/api';
import toast from 'react-hot-toast';

/**
 * Сортує товари за релевантністю до пошукового запиту.
 * 
 * Пріоритет:
 * 1. Назва починається з запиту (без урахування регістру)
 * 2. Назва містить запит як окреме слово (після пробілу)
 * 3. Назва просто містить запит
 * 4. Штрих-код або артикул містять запит
 * 
 * Всередині кожної групи — за алфавітом.
 */
function sortByRelevance(products: Product[], query: string): Product[] {
  const q = query.toLowerCase().trim();
  if (!q) return products;

  return [...products].sort((a, b) => {
    const titleA = a.title.toLowerCase();
    const titleB = b.title.toLowerCase();
    const barcodeA = (a.barcode || '').toLowerCase();
    const barcodeB = (b.barcode || '').toLowerCase();

    // Функція визначення групи релевантності (0 = найкраща)
    const getGroup = (title: string, barcode: string): number => {
      if (title.startsWith(q)) return 0;           // Починається з запиту
      if (title.includes(` ${q}`)) return 1;        // Слово починається з запиту
      if (title.includes(q)) return 2;              // Містить запит
      if (barcode.includes(q)) return 3;            // Штрих-код містить запит
      return 4;                                      // Інше
    };

    const groupA = getGroup(titleA, barcodeA);
    const groupB = getGroup(titleB, barcodeB);

    if (groupA !== groupB) return groupA - groupB;

    // В межах однієї групи — за алфавітом
    return titleA.localeCompare(titleB, 'uk');
  });
}

export function useProducts(params?: SearchParams) {
  return useQuery({
    queryKey: ['products', params],
    queryFn: () => productService.getProducts(params),
  });
}

export function useProduct(id: string) {
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
    mutationFn: ({ id, data }: { id: string; data: ProductUpdate }) =>
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
    mutationFn: (id: string) => productService.deleteProduct(id),
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
    select: (data) => {
      // Сортуємо за релевантністю на клієнтській стороні
      const items = data.items || [];
      return {
        ...data,
        items: sortByRelevance(items, query),
      };
    },
  });
}

export function useBarcodeSearch(barcode: string) {
  return useQuery({
    queryKey: ['product-barcode', barcode],
    queryFn: () => productService.searchByBarcode(barcode),
    enabled: barcode.length >= 8,
  });
}
