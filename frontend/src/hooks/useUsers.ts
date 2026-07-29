import { useQuery } from '@tanstack/react-query';
import { userService } from '@/services/userService';

export function useUsers(params?: { page?: number; size?: number }) {
  return useQuery({
    queryKey: ['users', params],
    queryFn: () => userService.getUsers(params),
    // API повертає масив, тому трансформуємо в пагіновану відповідь
    select: (data) => {
      if (Array.isArray(data)) {
        return { items: data, total: data.length };
      }
      return data as { items: any[]; total: number };
    },
  });
}
