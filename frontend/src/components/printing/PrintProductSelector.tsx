import React, { useState, useEffect, useRef } from 'react';
import { Search, Plus, X, Loader2, Package } from 'lucide-react';
import { formatCurrency } from '@/utils/format';
import type { Product } from '@/types/product';
import type { SelectedProduct } from '@/types/print';
import api from '@/services/api';
import type { PaginatedResponse } from '@/types/api';

// ── Пропси ───────────────────────────────────────
interface PrintProductSelectorProps {
  selected: SelectedProduct[];
  onAdd: (product: Product) => void;
  onRemove: (id: string) => void;
  onUpdateCopies: (id: string, copies: number) => void;
}

// ── Компонент ────────────────────────────────────
const PrintProductSelector: React.FC<PrintProductSelectorProps> = ({
  selected,
  onAdd,
  onRemove,
  onUpdateCopies,
}) => {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<Product[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Дебаунс пошуку
  useEffect(() => {
    if (!query.trim() || query.trim().length < 2) {
      setResults([]);
      setError(null);
      return;
    }

    const timer = setTimeout(async () => {
      setIsSearching(true);
      setError(null);
      try {
        const res = await api.get<PaginatedResponse<Product>>('/products', {
          params: { search: query.trim(), size: 20 },
        });
        setResults(res.data.items);
      } catch (err: any) {
        const msg = err?.response?.data?.detail || 'Помилка пошуку товарів';
        setError(msg);
        setResults([]);
      } finally {
        setIsSearching(false);
      }
    }, 350);

    return () => clearTimeout(timer);
  }, [query]);

  // Фокус на поле пошуку при монтуванні
  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  // Фільтруємо результати — прибираємо вже вибрані
  const availableResults = results.filter(
    (p) => !selected.some((s) => s.id === p.id)
  );

  return (
    <div className="flex flex-col h-full">
      {/* Поле пошуку */}
      <div className="relative mb-3">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 pointer-events-none" />
        <input
          ref={searchInputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Пошук товарів за назвою або штрих-кодом..."
          className="input-field pl-10 pr-10 w-full"
          autoComplete="off"
        />
        {isSearching && (
          <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-primary-500 animate-spin" />
        )}
        {query && !isSearching && (
          <button
            onClick={() => {
              setQuery('');
              setResults([]);
              setError(null);
              searchInputRef.current?.focus();
            }}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
          >
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Помилка */}
      {error && (
        <div className="mb-3 px-3 py-2 text-xs text-danger-500 bg-danger-50 dark:bg-danger-900/20 rounded-lg">
          {error}
        </div>
      )}

      {/* Результати пошуку */}
      <div className="flex-1 overflow-y-auto min-h-0 space-y-1 mb-3">
        {query.length >= 2 && !isSearching && availableResults.length === 0 && !error && (
          <div className="text-center py-8 text-gray-400 text-sm">
            <Package className="w-12 h-12 mx-auto mb-2 opacity-30" />
            <p>Товари не знайдено</p>
            <p className="text-xs mt-1">Спробуйте змінити пошуковий запит</p>
          </div>
        )}

        {availableResults.map((product) => (
          <button
            key={product.id}
            onClick={() => onAdd(product)}
            className="w-full card p-3 text-left transition-all hover:border-primary-300 dark:hover:border-primary-600 group"
          >
            <div className="flex items-center justify-between">
              <div className="min-w-0 flex-1">
                <p className="font-medium text-sm text-gray-900 dark:text-gray-100 truncate group-hover:text-primary-600">
                  {product.title}
                </p>
                <div className="flex items-center gap-2 mt-0.5">
                  <span className="text-xs text-gray-400">
                    {product.barcode || 'Без ШК'}
                  </span>
                  {product.sku && (
                    <span className="text-xs text-gray-400">Арт: {product.sku}</span>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-3 flex-shrink-0 ml-3">
                <span className="font-bold text-sm text-primary-600">
                  {formatCurrency(parseFloat(product.price))}
                </span>
                <span className="w-7 h-7 rounded-full bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400 flex items-center justify-center group-hover:bg-primary-200 dark:group-hover:bg-primary-900/50 transition-colors">
                  <Plus className="w-4 h-4" />
                </span>
              </div>
            </div>
          </button>
        ))}
      </div>

      {/* Список вибраних товарів */}
      {selected.length > 0 && (
        <div className="border-t border-gray-200 dark:border-slate-700 pt-3">
          <div className="flex items-center justify-between mb-2">
            <h4 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              Вибрано товарів: {selected.length}
            </h4>
          </div>
          <div className="space-y-2 max-h-64 overflow-y-auto">
            {selected.map((item) => (
              <div
                key={item.id}
                className="flex items-center gap-3 px-3 py-2.5 bg-gray-50 dark:bg-slate-700/50 rounded-lg"
              >
                {/* Інформація про товар */}
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                    {item.title}
                  </p>
                  <div className="flex items-center gap-2 text-xs text-gray-400">
                    <span>{item.barcode || 'Без ШК'}</span>
                    <span>{formatCurrency(parseFloat(item.price))}</span>
                  </div>
                </div>

                {/* Кількість копій */}
                <div className="flex items-center gap-1.5">
                  <label className="text-xs text-gray-500 dark:text-gray-400 whitespace-nowrap">
                    Копій:
                  </label>
                  <input
                    type="number"
                    min={1}
                    max={999}
                    value={item.copies}
                    onChange={(e) => {
                      const val = parseInt(e.target.value, 10);
                      if (val >= 1 && val <= 999) {
                        onUpdateCopies(item.id, val);
                      }
                    }}
                    className="w-16 h-8 text-center input-field !w-16 text-sm font-medium no-spinner"
                  />
                </div>

                {/* Кнопка видалення */}
                <button
                  onClick={() => onRemove(item.id)}
                  className="p-1.5 rounded-lg text-danger-500 hover:text-danger-700 hover:bg-danger-50 dark:hover:bg-danger-900/30 transition-colors"
                  title="Видалити"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

export default PrintProductSelector;
