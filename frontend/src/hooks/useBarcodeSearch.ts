import { useState, useEffect, useCallback, useRef } from 'react';
import { productService } from '@/services/productService';
import { Product } from '@/types/product';

interface UseBarcodeSearchOptions {
  onProductFound?: (product: Product) => void;
  debounceMs?: number;
}

export function useBarcodeSearch(options: UseBarcodeSearchOptions = {}) {
  const { onProductFound, debounceMs = 300 } = options;
  const [barcode, setBarcode] = useState('');
  const [product, setProduct] = useState<Product | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const searchByBarcode = useCallback(
    async (code: string) => {
      if (code.length < 8) {
        setProduct(null);
        setError(null);
        return;
      }

      setIsSearching(true);
      setError(null);

      try {
        const result = await productService.searchByBarcode(code);
        if (result.found) {
          setProduct(result.product);
          setError(null);
          onProductFound?.(result.product);
        } else {
          setProduct(null);
          setError('Товар з таким штрих-кодом не знайдено');
        }
      } catch (err: any) {
        setProduct(null);
        setError(err?.response?.data?.detail || 'Помилка пошуку');
      } finally {
        setIsSearching(false);
      }
    },
    [onProductFound]
  );

  useEffect(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }

    if (barcode) {
      timerRef.current = setTimeout(() => {
        searchByBarcode(barcode);
      }, debounceMs);
    } else {
      setProduct(null);
      setError(null);
    }

    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, [barcode, debounceMs, searchByBarcode]);

  const handleBarcodeInput = useCallback((value: string) => {
    // Only allow digits
    const cleaned = value.replace(/\D/g, '');
    setBarcode(cleaned);
  }, []);

  const reset = useCallback(() => {
    setBarcode('');
    setProduct(null);
    setError(null);
  }, []);

  return {
    barcode,
    product,
    isSearching,
    error,
    setBarcode: handleBarcodeInput,
    reset,
  };
}
