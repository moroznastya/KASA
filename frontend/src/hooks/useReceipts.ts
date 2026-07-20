import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { receiptService } from '@/services/receiptService';
import { ReceiptCreate } from '@/types/receipt';
import { SearchParams } from '@/types/api';
import toast from 'react-hot-toast';

export function useReceipts(params?: SearchParams) {
  return useQuery({
    queryKey: ['receipts', params],
    queryFn: () => receiptService.getReceipts(params),
  });
}

export function useReceipt(id: string) {
  return useQuery({
    queryKey: ['receipt', id],
    queryFn: () => receiptService.getReceipt(id),
    enabled: !!id,
  });
}

export function useCreateReceipt() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: ReceiptCreate) => receiptService.createReceipt(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['receipts'] });
      queryClient.invalidateQueries({ queryKey: ['dashboard'] });
      toast.success('Чек успішно створено');
    },
    onError: (error: any) => {
      const detail = error?.response?.data?.detail;
      if (Array.isArray(detail)) {
        toast.error(detail.map((d: any) => d.msg || d.message || String(d)).join(', '));
      } else if (typeof detail === 'object' && detail !== null) {
        toast.error(JSON.stringify(detail));
      } else {
        toast.error(detail || 'Помилка при створенні чеку');
      }
    },
  });
}

export function useTodayStats() {
  return useQuery({
    queryKey: ['receipts', 'stats', 'today'],
    queryFn: () => receiptService.getTodayStats(),
  });
}
