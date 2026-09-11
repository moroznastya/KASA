import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { writeOffReasonsService, WriteOffReasonItem } from '@/services/writeOffReasonsService';

export const writeOffReasonsQueryKey = ['write-off-reasons'] as const;

/** Завантажує список причин списання з довідника */
export function useWriteOffReasons() {
  return useQuery({
    queryKey: writeOffReasonsQueryKey,
    queryFn: () => writeOffReasonsService.getWriteOffReasons(),
    staleTime: 30_000,
  });
}

/** Створює нову причину списання та оновлює кеш списку */
export function useCreateWriteOffReason() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => writeOffReasonsService.createWriteOffReason(name),
    onSuccess: (newReason: WriteOffReasonItem) => {
      queryClient.setQueryData<WriteOffReasonItem[]>(writeOffReasonsQueryKey, (old = []) => {
        // Додаємо нову причину в кінець (уникаємо дублікатів)
        if (old.some((r) => r.name.toLowerCase() === newReason.name.toLowerCase())) {
          return old;
        }
        return [...old, newReason];
      });
      queryClient.invalidateQueries({ queryKey: writeOffReasonsQueryKey });
    },
  });
}
