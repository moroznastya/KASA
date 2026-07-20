import { useState, useCallback, useMemo } from 'react';

interface UsePaginationOptions {
  initialPage?: number;
  initialSize?: number;
  total?: number;
}

export function usePagination(options: UsePaginationOptions = {}) {
  const { initialPage = 1, initialSize = 20, total = 0 } = options;
  const [page, setPage] = useState(initialPage);
  const [size, setSize] = useState(initialSize);

  const totalPages = useMemo(() => Math.max(1, Math.ceil(total / size)), [total, size]);

  const nextPage = useCallback(() => {
    setPage((prev) => Math.min(prev + 1, totalPages));
  }, [totalPages]);

  const prevPage = useCallback(() => {
    setPage((prev) => Math.max(prev - 1, 1));
  }, []);

  const goToPage = useCallback(
    (p: number) => {
      setPage(Math.max(1, Math.min(p, totalPages)));
    },
    [totalPages]
  );

  const changeSize = useCallback((newSize: number) => {
    setSize(newSize);
    setPage(1);
  }, []);

  const reset = useCallback(() => {
    setPage(1);
  }, []);

  return {
    page,
    size,
    totalPages,
    nextPage,
    prevPage,
    goToPage,
    changeSize,
    reset,
    setTotal: () => {}, // handled externally
  };
}
