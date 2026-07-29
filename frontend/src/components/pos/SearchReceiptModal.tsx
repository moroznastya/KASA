import React, { useState, useCallback, useEffect } from 'react';
import { Search, X, Loader2, Calendar, ChevronLeft, ChevronRight, RotateCcw, ShoppingCart } from 'lucide-react';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { receiptService } from '@/services/receiptService';
import { formatCurrency } from '@/utils/format';
import type { ReceiptSearchResult } from '@/types/receipt';

// ── Пропси ────────────────────────────────────
interface SearchReceiptModalProps {
  isOpen: boolean;
  onClose: () => void;
  onReceiptSelect: (receipt: ReceiptSearchResult) => void;
  /** Коли searchId змінюється — стан пошуку скидається (свіжий пошук).
   *  Якщо searchId той самий — стан зберігається (повернення з вибору товарів). */
  searchId?: number;
}

// ── Компонент ─────────────────────────────────
const SearchReceiptModal: React.FC<SearchReceiptModalProps> = ({
  isOpen,
  onClose,
  onReceiptSelect,
  searchId = 0,
}) => {
  const [query, setQuery] = useState('');
  const [dateFrom, setDateFrom] = useState('');
  const [dateTo, setDateTo] = useState('');
  const [results, setResults] = useState<ReceiptSearchResult[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize] = useState(20);
  const [isLoading, setIsLoading] = useState(false);
  const [hasSearched, setHasSearched] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Скидаємо стан коли searchId змінюється (новий пошук з панелі)
  useEffect(() => {
    setQuery('');
    setDateFrom('');
    setDateTo('');
    setResults([]);
    setTotal(0);
    setPage(1);
    setHasSearched(false);
    setError(null);
  }, [searchId]);

  // Пошук чеків
  const doSearch = useCallback(async (searchPage: number = 1) => {
    if (!query.trim() && !dateFrom && !dateTo) {
      setError('Введіть номер чеку, назву товару або виберіть дату');
      return;
    }

    setIsLoading(true);
    setError(null);
    setHasSearched(true);

    try {
      const result = await receiptService.searchReceipts({
        q: query.trim() || undefined,
        date_from: dateFrom || undefined,
        date_to: dateTo || undefined,
        receipt_type: 'sale',
        page: searchPage,
        size: pageSize,
      });
      setResults(result.items);
      setTotal(result.total);
      setPage(result.page);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Помилка пошуку чеків');
      setResults([]);
    } finally {
      setIsLoading(false);
    }
  }, [query, dateFrom, dateTo, pageSize]);

  // Debounce при введенні тексту
  useEffect(() => {
    if (!query.trim()) return;
    const timer = setTimeout(() => {
      doSearch(1);
    }, 300);
    return () => clearTimeout(timer);
  }, [query, doSearch]);

  // Обробник натискання Enter в полі пошуку
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      doSearch(1);
    }
  };

  // Загальна кількість сторінок
  const totalPages = Math.ceil(total / pageSize);

  // Форматування дати
  const formatDate = (dateStr: string) => {
    const d = new Date(dateStr);
    return d.toLocaleDateString('uk-UA', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="🔍 Пошук оригінального чеку" size="2xl">
      <div className="space-y-4">
        {/* ── Поле пошуку ──────────────────────── */}
        <div className="flex gap-3 items-end">
          <div className="flex-1">
            <Input
              label="Номер або назва товару"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Введіть номер чеку або назву товару..."
              icon={<Search className="w-4 h-4" />}
              id="search-receipt-query"
              name="search-receipt-query"
            />
          </div>
          <Button
            onClick={() => doSearch(1)}
            disabled={isLoading}
            icon={isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
          >
            Пошук
          </Button>
        </div>

        {/* ── Фільтр за датою ──────────────────── */}
        <div className="flex gap-3 items-end">
          <div className="flex-1">
            <Input
              label="Дата з"
              type="date"
              value={dateFrom}
              onChange={(e) => setDateFrom(e.target.value)}
              icon={<Calendar className="w-4 h-4" />}
              id="search-receipt-date-from"
              name="search-receipt-date-from"
            />
          </div>
          <div className="flex-1">
            <Input
              label="Дата по"
              type="date"
              value={dateTo}
              onChange={(e) => setDateTo(e.target.value)}
              icon={<Calendar className="w-4 h-4" />}
              id="search-receipt-date-to"
              name="search-receipt-date-to"
            />
          </div>
        </div>

        {/* ── Помилка ──────────────────────────── */}
        {error && (
          <div className="p-3 rounded-lg bg-danger-50 dark:bg-danger-900/20 border border-danger-200 dark:border-danger-800 text-sm text-danger-700 dark:text-danger-300">
            {error}
          </div>
        )}

        {/* ── Індикатор завантаження ──────────── */}
        {isLoading && (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="w-6 h-6 animate-spin text-primary-600" />
            <span className="ml-2 text-sm text-gray-500">Пошук чеків...</span>
          </div>
        )}

        {/* ── Результати ───────────────────────── */}
        {!isLoading && hasSearched && results.length === 0 && (
          <div className="text-center py-8 text-gray-400">
            <ShoppingCart className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p className="text-sm">Чеків не знайдено</p>
            <p className="text-xs mt-1">Спробуйте змінити параметри пошуку</p>
          </div>
        )}

        {!isLoading && results.length > 0 && (
          <>
            {/* Список чеків */}
            <div className="space-y-2 max-h-80 overflow-y-auto">
              {results.map((receipt) => (
                <button
                  key={receipt.id}
                  onClick={() => onReceiptSelect(receipt)}
                  className="w-full text-left p-4 rounded-lg border border-gray-200 dark:border-slate-700 hover:border-primary-300 dark:hover:border-primary-600 hover:bg-primary-50/50 dark:hover:bg-primary-900/10 transition-all cursor-pointer group"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0 flex-1">
                      {/* Номер чеку та тип */}
                      <div className="flex items-center gap-2 mb-1">
                        <span className="font-semibold text-gray-900 dark:text-gray-100 group-hover:text-primary-600 dark:group-hover:text-primary-400">
                          Чек №{receipt.receipt_number}
                        </span>
                        {receipt.receipt_type === 'return' ? (
                          <Badge variant="danger" size="sm">
                            <RotateCcw className="w-3 h-3 mr-0.5" />
                            Повернення
                          </Badge>
                        ) : (
                          <Badge variant="success" size="sm">Продаж</Badge>
                        )}
                      </div>

                      {/* Дата та касир */}
                      <div className="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400">
                        <span>{formatDate(receipt.created_at)}</span>
                        {receipt.cashier_name && (
                          <>
                            <span className="text-gray-300 dark:text-slate-600">|</span>
                            <span>Касир: {receipt.cashier_name}</span>
                          </>
                        )}
                      </div>
                    </div>

                    {/* Сума та кількість */}
                    <div className="text-right shrink-0">
                      <div className="font-bold text-gray-900 dark:text-gray-100">
                        {formatCurrency(receipt.total_amount)}
                      </div>
                      <div className="text-xs text-gray-500 dark:text-gray-400">
                        {receipt.items_count} поз.
                      </div>
                    </div>
                  </div>
                </button>
              ))}
            </div>

            {/* Пагінація */}
            {totalPages > 1 && (
              <div className="flex items-center justify-between pt-3 border-t border-gray-200 dark:border-slate-700">
                <span className="text-xs text-gray-500 dark:text-gray-400">
                  {total} чеків, сторінка {page} з {totalPages}
                </span>
                <div className="flex gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => doSearch(page - 1)}
                    disabled={page <= 1 || isLoading}
                    icon={<ChevronLeft className="w-4 h-4" />}
                  >
                    Попередня
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => doSearch(page + 1)}
                    disabled={page >= totalPages || isLoading}
                    icon={<ChevronRight className="w-4 h-4" />}
                    >
                    Наступна
                  </Button>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </Modal>
  );
};

export default SearchReceiptModal;
