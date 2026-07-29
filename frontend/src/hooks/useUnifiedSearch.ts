import { useState, useEffect, useCallback, useRef } from 'react';
import { productService } from '@/services/productService';
import { Product } from '@/types/product';

interface UseUnifiedSearchOptions {
  onBarcodeFound?: (product: Product) => void;
  debounceMs?: number;
}

interface SearchResult {
  type: 'name' | 'barcode';
  product: Product;
}

export function useUnifiedSearch(options: UseUnifiedSearchOptions = {}) {
  const { onBarcodeFound, debounceMs = 300 } = options;
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ✅ Фікс: зберігаємо onBarcodeFound в реф, щоб уникнути циклічної залежності
  const onBarcodeFoundRef = useRef(onBarcodeFound);
  useEffect(() => {
    onBarcodeFoundRef.current = onBarcodeFound;
  }, [onBarcodeFound]);

  const isBarcode = query.length >= 8 && /^\d+$/.test(query);
  const isNameSearch = query.length >= 2 && !isBarcode;

  // ✅ Фікс: прибираємо onBarcodeFound з залежностей — використовуємо реф
  const performSearch = useCallback(async (searchQuery: string) => {
    const isBarcodeSearch = searchQuery.length >= 8 && /^\d+$/.test(searchQuery);

    if (!isBarcodeSearch && searchQuery.length < 2) {
      setResults([]);
      setError(null);
      setIsSearching(false);
      return;
    }

    setIsSearching(true);
    setError(null);

    try {
      if (isBarcodeSearch) {
        // Search by barcode
        const product = await productService.searchByBarcode(searchQuery);
        setResults([{ type: 'barcode', product }]);
        setError(null);
        // ✅ Використовуємо реф замість прямого виклику
        onBarcodeFoundRef.current?.(product);
      } else {
        // Search by name
        const response = await productService.searchProducts(searchQuery);
        const items = response.items.map((p) => ({ type: 'name' as const, product: p }));
        setResults(items);
        setError(null);
      }
    } catch (err: any) {
      if (isBarcodeSearch) {
        if (err?.response?.status === 404) {
          setError('Товар з таким штрих-кодом не знайдено');
          setResults([]);
        } else {
          setError(err?.response?.data?.detail || 'Помилка пошуку');
          setResults([]);
        }
      } else {
        setResults([]);
        setError(null);
      }
    } finally {
      setIsSearching(false);
    }
  }, []); // ✅ Порожній масив залежностей — performSearch ніколи не змінюється

  useEffect(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }

    if (query.length >= 2) {
      timerRef.current = setTimeout(() => {
        performSearch(query);
      }, debounceMs);
    } else {
      setResults([]);
      setError(null);
    }

    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, [query, debounceMs, performSearch]);

  const handleInputChange = useCallback((value: string) => {
    setQuery(value);
  }, []);

  const reset = useCallback(() => {
    setQuery('');
    setResults([]);
    setError(null);
  }, []);

  return {
    query,
    results,
    isSearching,
    error,
    isBarcode,
    isNameSearch,
    setQuery: handleInputChange,
    reset,
  };
}
