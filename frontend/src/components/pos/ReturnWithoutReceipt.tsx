import React, { useState, useCallback, useEffect } from 'react';
import { Search, X, Loader2, Barcode, ShoppingCart, Minus, Plus, Receipt, ImageOff, ChevronRight, Package } from 'lucide-react';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { receiptService } from '@/services/receiptService';
import { formatCurrency } from '@/utils/format';
import { toast } from 'react-hot-toast';
import type { ProductRecentSalesResponse, ProductRecentSalesListResponse, RecentSaleInfo } from '@/types/receipt';
import type { ReturnCartItem } from './SelectItemsFromReceipt';

// ── Пропси ────────────────────────────────────
interface ReturnWithoutReceiptProps {
  isOpen: boolean;
  onClose: () => void;
  onProcessReturn: (items: ReturnCartItem[]) => void;
}

// ── Компонент ─────────────────────────────────
const ReturnWithoutReceipt: React.FC<ReturnWithoutReceiptProps> = ({
  isOpen,
  onClose,
  onProcessReturn,
}) => {
  const [barcode, setBarcode] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [searchResult, setSearchResult] = useState<ProductRecentSalesListResponse | null>(null);
  const [selectedProduct, setSelectedProduct] = useState<ProductRecentSalesResponse | null>(null);
  const [returnQuantity, setReturnQuantity] = useState(1);
  const [selectedReceiptId, setSelectedReceiptId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Скидаємо стан при відкритті/закритті
  useEffect(() => {
    if (!isOpen) {
      setBarcode('');
      setSearchResult(null);
      setSelectedProduct(null);
      setReturnQuantity(1);
      setSelectedReceiptId(null);
      setError(null);
    }
  }, [isOpen]);

  // Пошук за штрих-кодом або назвою
  const searchByBarcode = useCallback(async (code: string) => {
    if (!code.trim()) return;

    setIsLoading(true);
    setError(null);
    setSearchResult(null);
    setSelectedProduct(null);
    setReturnQuantity(1);
    setSelectedReceiptId(null);

    try {
      const data = await receiptService.getRecentSalesByProduct(code.trim());
      setSearchResult(data);

      // Якщо знайдено рівно 1 товар — одразу переходимо до деталей
      if (data.items.length === 1) {
        setSelectedProduct(data.items[0]);
        if (data.items[0].returnable <= 0) {
          toast.error('Цей товар не доступний для повернення');
        }
      }
    } catch (err) {
      // Дістаємо змістовне повідомлення від сервера
      let message = 'Товар не знайдено';
      if (err && typeof err === 'object' && 'response' in err) {
        const axiosErr = err as { response?: { data?: { detail?: string } } };
        message = axiosErr.response?.data?.detail || message;
      } else if (err instanceof Error) {
        message = err.message;
      }
      setError(message);
      setSearchResult(null);
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Debounce при введенні пошуку
  useEffect(() => {
    if (!barcode.trim() || barcode.trim().length < 3) return;
    const timer = setTimeout(() => {
      searchByBarcode(barcode);
    }, 400);
    return () => clearTimeout(timer);
  }, [barcode, searchByBarcode]);

  // Обробник натискання Enter
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      searchByBarcode(barcode);
    }
  };

  // Вибрати товар зі списку
  const handleSelectProduct = useCallback((product: ProductRecentSalesResponse) => {
    setSelectedProduct(product);
    setReturnQuantity(1);
    setSelectedReceiptId(null);
    if (product.returnable <= 0) {
      toast.error('Цей товар не доступний для повернення');
    }
  }, []);

  // Повернутися до списку
  const handleBackToList = useCallback(() => {
    setSelectedProduct(null);
    setReturnQuantity(1);
    setSelectedReceiptId(null);
  }, []);

  // Додати товар без прив'язки до чеку
  const handleAddWithoutReceipt = useCallback(() => {
    if (!selectedProduct) return;

    const qty = returnQuantity;
    if (qty <= 0) {
      toast.error('Кількість має бути більше 0');
      return;
    }
    if (qty > selectedProduct.returnable) {
      toast.error(`Максимальна кількість для повернення: ${selectedProduct.returnable}`);
      return;
    }

    const items: ReturnCartItem[] = [
      {
        product_id: selectedProduct.product.id,
        product_title: selectedProduct.product.title,
        product_barcode: selectedProduct.product.barcode,
        image_url: null,
        quantity: qty,
        price: selectedProduct.product.price,
        original_receipt_id: '', // без прив'язки до чеку
        tax_rate: 20,
        unit: selectedProduct.product.unit,
      },
    ];

    onProcessReturn(items);
  }, [selectedProduct, returnQuantity, onProcessReturn]);

  // Додати товар з прив'язкою до конкретного чеку
  const handleAddWithReceipt = useCallback(
    (sale: RecentSaleInfo) => {
      if (!selectedProduct) return;

      const qty = returnQuantity;
      if (qty <= 0) {
        toast.error('Кількість має бути більше 0');
        return;
      }
      if (qty > selectedProduct.returnable) {
        toast.error(`Максимальна кількість для повернення: ${selectedProduct.returnable}`);
        return;
      }

      const items: ReturnCartItem[] = [
        {
          product_id: selectedProduct.product.id,
          product_title: selectedProduct.product.title,
          product_barcode: selectedProduct.product.barcode,
          image_url: null,
          quantity: qty,
          price: Number(sale.price),
          original_receipt_id: sale.receipt_id,
          tax_rate: 20,
          unit: selectedProduct.product.unit,
        },
      ];

      onProcessReturn(items);
    },
    [selectedProduct, returnQuantity, onProcessReturn]
  );

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
    <Modal isOpen={isOpen} onClose={onClose} title="📦 Повернення без чеку" size="2xl">
      <div className="space-y-4">
        {/* ── Поле пошуку ──────────────────────── */}
        <div className="flex gap-3 items-end">
          <div className="flex-1">
            <Input
              label="Штрих-код або назва товару"
              value={barcode}
              onChange={(e) => setBarcode(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Відскануйте або введіть штрих-код / назву..."
              icon={<Barcode className="w-4 h-4" />}
              id="return-barcode-input"
              name="return-barcode-input"
              autoFocus
            />
          </div>
          <Button
            onClick={() => searchByBarcode(barcode)}
            disabled={isLoading || !barcode.trim()}
            icon={isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
          >
            Знайти
          </Button>
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
            <span className="ml-2 text-sm text-gray-500">Пошук товару...</span>
          </div>
        )}

        {/* ═══════════════════════════════════════════
            ЕТАП 1: СПИСОК ЗНАЙДЕНИХ ТОВАРІВ
            ═══════════════════════════════════════════ */}
        {!isLoading && searchResult && !selectedProduct && (
          <>
            {/* Кількість знайдених товарів */}
            <div className="text-sm text-gray-500 dark:text-gray-400">
              Знайдено товарів: <strong>{searchResult.total}</strong>
            </div>

            {/* Список товарів */}
            <div className="space-y-2 max-h-96 overflow-y-auto">
              {searchResult.items.map((item) => (
                <button
                  key={item.product.id}
                  onClick={() => handleSelectProduct(item)}
                  className="w-full text-left p-4 rounded-lg border border-gray-200 dark:border-slate-700 
                    bg-white dark:bg-slate-800 
                    hover:border-primary-300 dark:hover:border-primary-600 
                    hover:shadow-sm hover:bg-primary-50/30 dark:hover:bg-primary-900/10
                    transition-all cursor-pointer group"
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <Package className="w-5 h-5 text-primary-500 shrink-0" />
                        <h4 className="text-base font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {item.product.title}
                        </h4>
                      </div>
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1.5 text-sm text-gray-500 dark:text-gray-400">
                        {item.product.barcode && (
                          <span className="text-xs font-mono bg-gray-100 dark:bg-slate-700 px-1.5 py-0.5 rounded">
                            {item.product.barcode}
                          </span>
                        )}
                        <span>Ціна: {formatCurrency(item.product.price)}</span>
                        <span>Од.: {item.product.unit}</span>
                      </div>
                      <div className="flex items-center gap-4 mt-2">
                        <span className="text-xs text-gray-400">
                          Продано: <strong>{item.total_sold}</strong>
                        </span>
                        <span className="text-xs text-danger-500">
                          Повернено: <strong>{item.total_returned}</strong>
                        </span>
                        <span className={`text-xs font-semibold ${
                          item.returnable > 0 ? 'text-success-600' : 'text-gray-400'
                        }`}>
                          Доступно: <strong>{item.returnable}</strong>
                        </span>
                      </div>
                    </div>
                    <ChevronRight className="w-5 h-5 text-gray-300 dark:text-slate-600 group-hover:text-primary-500 group-hover:translate-x-0.5 transition-all shrink-0" />
                  </div>
                </button>
              ))}
            </div>

            {/* Якщо нічого не знайдено — не має дійти сюди, але для безпеки */}
            {searchResult.items.length === 0 && (
              <div className="text-center py-6 text-gray-400">
                <Package className="w-10 h-10 mx-auto mb-2 opacity-50" />
                <p className="text-sm">Нічого не знайдено</p>
              </div>
            )}
          </>
        )}

        {/* ═══════════════════════════════════════════
            ЕТАП 2: ДЕТАЛІ ВИБРАНОГО ТОВАРУ
            ═══════════════════════════════════════════ */}
        {!isLoading && selectedProduct && (
          <>
            {/* Кнопка "Назад до списку" (якщо було > 1 результату) */}
            {searchResult && searchResult.total > 1 && (
              <button
                onClick={handleBackToList}
                className="text-sm text-primary-600 dark:text-primary-400 hover:underline flex items-center gap-1"
              >
                ← Назад до списку товарів
              </button>
            )}

            {/* Картка товару */}
            <div className="p-4 rounded-lg border border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0 flex-1">
                  <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    {selectedProduct.product.title}
                  </h3>
                  {selectedProduct.product.barcode && (
                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
                      Штрих-код: {selectedProduct.product.barcode}
                    </p>
                  )}
                  <div className="flex items-center gap-3 mt-2">
                    <Badge variant="primary" size="sm">
                      Ціна: {formatCurrency(selectedProduct.product.price)}
                    </Badge>
                    <Badge variant="default" size="sm">
                      Од.: {selectedProduct.product.unit}
                    </Badge>
                  </div>
                </div>
              </div>

              {/* Статистика повернення */}
              <div className="grid grid-cols-3 gap-3 mt-4">
                <div className="p-2 rounded-lg bg-gray-50 dark:bg-slate-700/50 text-center">
                  <p className="text-xs text-gray-500 dark:text-gray-400">Продано</p>
                  <p className="text-lg font-bold text-gray-900 dark:text-gray-100">
                    {selectedProduct.total_sold}
                  </p>
                </div>
                <div className="p-2 rounded-lg bg-gray-50 dark:bg-slate-700/50 text-center">
                  <p className="text-xs text-gray-500 dark:text-gray-400">Повернено</p>
                  <p className="text-lg font-bold text-danger-600">
                    {selectedProduct.total_returned}
                  </p>
                </div>
                <div className="p-2 rounded-lg bg-gray-50 dark:bg-slate-700/50 text-center">
                  <p className="text-xs text-gray-500 dark:text-gray-400">Доступно</p>
                  <p className={`text-lg font-bold ${selectedProduct.returnable > 0 ? 'text-success-600' : 'text-gray-400'}`}>
                    {selectedProduct.returnable}
                  </p>
                </div>
              </div>
            </div>

            {/* Поле кількості */}
            <div className="flex items-center gap-3">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                Кількість для повернення:
              </span>
              <div className="flex items-center gap-1">
                <button
                  onClick={() => setReturnQuantity((p) => Math.max(1, p - 1))}
                  disabled={returnQuantity <= 1}
                  className="p-1.5 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                >
                  <Minus className="w-4 h-4" />
                </button>
                <input
                  type="number"
                  min={1}
                  max={selectedProduct.returnable}
                  value={returnQuantity}
                  onChange={(e) => {
                    const val = parseInt(e.target.value) || 1;
                    setReturnQuantity(Math.max(1, Math.min(selectedProduct.returnable, val)));
                  }}
                  className="w-16 text-center text-lg font-bold rounded-md border border-gray-300 dark:border-slate-600 text-gray-900 dark:text-gray-100 bg-white dark:bg-slate-700 py-1"
                />
                <button
                  onClick={() => setReturnQuantity((p) => Math.min(selectedProduct.returnable, p + 1))}
                  disabled={returnQuantity >= selectedProduct.returnable}
                  className="p-1.5 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                >
                  <Plus className="w-4 h-4" />
                </button>
              </div>
              <span className="text-xs text-gray-400">макс. {selectedProduct.returnable}</span>
            </div>

            {/* Кнопка "Повернути без прив'язки до чеку" */}
            <Button
              onClick={handleAddWithoutReceipt}
              disabled={selectedProduct.returnable <= 0 || returnQuantity <= 0}
              className="w-full"
              variant="danger"
              size="lg"
              icon={<ShoppingCart className="w-5 h-5" />}
            >
              Повернути {returnQuantity} шт. без прив'язки до чеку (
              {formatCurrency(returnQuantity * selectedProduct.product.price)})
            </Button>

            {/* ── Останні продажі ────────────────── */}
            {selectedProduct.recent_sales.length > 0 && (
              <div>
                <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                  Останні продажі (натисніть для прив'язки до чеку)
                </h4>
                <div className="space-y-2 max-h-48 overflow-y-auto">
                  {selectedProduct.recent_sales.map((sale) => (
                    <button
                      key={sale.receipt_id}
                      onClick={() => handleAddWithReceipt(sale)}
                      className={`w-full text-left p-3 rounded-lg border transition-all cursor-pointer ${
                        selectedReceiptId === sale.receipt_id
                          ? 'border-primary-300 dark:border-primary-600 bg-primary-50/50 dark:bg-primary-900/10'
                          : 'border-gray-200 dark:border-slate-700 hover:border-primary-200 dark:hover:border-primary-700 hover:bg-gray-50 dark:hover:bg-slate-700/50'
                      }`}
                    >
                      <div className="flex items-center justify-between gap-3">
                        <div className="flex items-center gap-2 min-w-0">
                          <Receipt className="w-4 h-4 text-gray-400 shrink-0" />
                          <div className="min-w-0">
                            <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                              Чек №{sale.receipt_number}
                            </p>
                            <p className="text-xs text-gray-500">{formatDate(sale.created_at)}</p>
                          </div>
                        </div>
                        <div className="text-right shrink-0">
                          <p className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                            {sale.quantity} шт.
                          </p>
                          <p className="text-xs text-gray-500">
                            {formatCurrency(Number(sale.price))}
                          </p>
                        </div>
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            )}

            {/* Немає останніх продажів */}
            {selectedProduct.recent_sales.length === 0 && (
              <div className="text-center py-4 text-gray-400 border-t border-gray-200 dark:border-slate-700">
                <Receipt className="w-8 h-8 mx-auto mb-1 opacity-50" />
                <p className="text-sm">Немає останніх продажів для прив'язки</p>
                <p className="text-xs mt-0.5">Використайте кнопку вище для повернення без чеку</p>
              </div>
            )}
          </>
        )}

        {/* ── Початковий стан ──────────────────── */}
        {!isLoading && !searchResult && !selectedProduct && !error && (
          <div className="text-center py-8 text-gray-400">
            <Barcode className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p className="text-sm">Введіть або відскануйте штрих-код / назву товару</p>
            <p className="text-xs mt-1">Система покаже всі товари, що відповідають запиту</p>
          </div>
        )}
      </div>
    </Modal>
  );
};

export default ReturnWithoutReceipt;
