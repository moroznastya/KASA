import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Search, Plus, Minus, Trash2, ShoppingCart, CreditCard, Banknote, Loader2, X, AlertTriangle } from 'lucide-react';
import { useUnifiedSearch } from '@/hooks/useUnifiedSearch';
import { receiptService } from '@/services/receiptService';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { formatCurrency, formatUnit } from '@/utils/format';
import { ReceiptCreate, PaymentMethod } from '@/types/receipt';
import toast from 'react-hot-toast';

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
  tax_rate: number;
  is_weight: boolean;
  stock: number; // поточний залишок товару
  unit: string; // одиниця виміру
}

const PosPage: React.FC = () => {
  const [cart, setCart] = useState<CartItem[]>([]);
  const [showPayment, setShowPayment] = useState(false);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod>('cash');
  const [cashAmount, setCashAmount] = useState('');
  const [cardAmount, setCardAmount] = useState('');
  const [isDebt, setIsDebt] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [showQuantityModal, setShowQuantityModal] = useState(false);
  const [selectedProduct, setSelectedProduct] = useState<any>(null);
  const [quantityInput, setQuantityInput] = useState('1');
  const [quantityError, setQuantityError] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const handleBarcodeFound = useCallback((product: any) => {
    const stock = parseFloat(product.stock) || 0;
    if (stock <= 0) {
      toast.error(`Товар "${product.title}" відсутній на складі`);
      return;
    }
    addToCart(product);
    toast.success(`Додано: ${product.title}`);
  }, []);

  const {
    query,
    results,
    isSearching,
    error,
    setQuery: handleSearchChange,
    reset: resetSearch,
  } = useUnifiedSearch({
    onBarcodeFound: handleBarcodeFound,
  });

  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  const addToCart = useCallback((product: any, quantity: number = 1) => {
    const stock = parseFloat(product.stock) || 0;

    // Перевірка: чи є товар в наявності
    if (stock <= 0) {
      toast.error(`Товар "${product.title}" відсутній на складі`);
      return;
    }

    setCart((prev) => {
      const existing = prev.find((item) => item.product_id === product.id);
      const currentQtyInCart = existing ? existing.quantity : 0;
      const totalRequested = currentQtyInCart + quantity;

      // Перевірка: чи не перевищує загальна кількість залишок
      if (totalRequested > stock) {
        toast.error(
          `Недостатньо товару "${product.title}" на складі. Доступно: ${stock} ${formatUnit(product.unit)}`
        );
        return prev;
      }

      if (existing) {
        return prev.map((item) =>
          item.product_id === product.id
            ? { ...item, quantity: totalRequested }
            : item
        );
      }
      return [
        ...prev,
        {
          product_id: product.id,
          product_title: product.title,
          product_barcode: product.barcode,
          quantity,
          price: parseFloat(product.price),
          tax_rate: parseFloat(product.tax_rate) || 20,
          is_weight: product.is_weight || false,
          stock,
          unit: product.unit || 'шт',
        },
      ];
    });
    resetSearch();
  }, [resetSearch]);

  const handleProductSelect = (product: any) => {
    const stock = parseFloat(product.stock) || 0;

    // Якщо товару нема в наявності — блокуємо
    if (stock <= 0) {
      toast.error(`Товар "${product.title}" відсутній на складі`);
      return;
    }

    if (product.is_weight) {
      setSelectedProduct(product);
      setQuantityInput('1');
      setQuantityError(null);
      setShowQuantityModal(true);
    } else {
      addToCart(product);
    }
  };

  const handleQuantityConfirm = () => {
    if (selectedProduct) {
      const qty = parseFloat(quantityInput);
      if (qty > 0) {
        const stock = parseFloat(selectedProduct.stock) || 0;

        // Перевірка чи є в наявності
        if (stock <= 0) {
          setQuantityError('Товар відсутній на складі');
          return;
        }

        // Перевірка чи не перевищує залишок
        if (qty > stock) {
          setQuantityError(`Доступно лише ${stock} ${formatUnit(selectedProduct.unit)}`);
          return;
        }

        addToCart(selectedProduct, qty);
        setShowQuantityModal(false);
        setSelectedProduct(null);
        setQuantityError(null);
      }
    }
  };

  const updateQuantity = (productId: string, delta: number) => {
    setCart((prev) => {
      const item = prev.find((i) => i.product_id === productId);
      if (!item) return prev;

      const newQty = item.quantity + delta;

      // Перевірка: не більше ніж залишок
      if (newQty > item.stock) {
        toast.error(
          `Недостатньо товару "${item.product_title}" на складі. Доступно: ${item.stock} ${formatUnit(item.unit)}`
        );
        return prev;
      }

      return prev
        .map((i) =>
          i.product_id === productId
            ? { ...i, quantity: Math.max(0.001, newQty) }
            : i
        )
        .filter((i) => i.quantity > 0);
    });
  };

  const setItemQuantity = (productId: string, quantity: number) => {
    if (quantity <= 0) {
      setCart((prev) => prev.filter((item) => item.product_id !== productId));
      return;
    }

    setCart((prev) => {
      const item = prev.find((i) => i.product_id === productId);
      if (!item) return prev;

      // Перевірка: не більше ніж залишок
      if (quantity > item.stock) {
        toast.error(
          `Недостатньо товару "${item.product_title}" на складі. Доступно: ${item.stock} ${formatUnit(item.unit)}`
        );
        return prev;
      }

      return prev.map((i) =>
        i.product_id === productId ? { ...i, quantity } : i
      );
    });
  };

  const removeFromCart = (productId: string) => {
    setCart((prev) => prev.filter((item) => item.product_id !== productId));
  };

  const clearCart = () => {
    setCart([]);
  };

  const subtotal = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
  const vatAmount = cart.reduce(
    (sum, item) => sum + (item.quantity * item.price * item.tax_rate) / (100 + item.tax_rate),
    0
  );

  const handlePayment = async () => {
    if (cart.length === 0) {
      toast.error('Кошик порожній');
      return;
    }

    // Фінальна перевірка залишків перед оплатою
    for (const item of cart) {
      if (item.quantity > item.stock) {
        toast.error(
          `Недостатньо товару "${item.product_title}" на складі. Доступно: ${item.stock} ${formatUnit(item.unit)}`
        );
        return;
      }
    }

    setIsProcessing(true);
    try {
      const receiptData: ReceiptCreate = {
        receipt_type: 'sale',
        total_amount: subtotal.toFixed(2),
        items: cart.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
      };

      await receiptService.createReceipt(receiptData);
      toast.success('Чек створено успішно');
      setCart([]);
      setShowPayment(false);
      setCashAmount('');
      setCardAmount('');
      setIsDebt(false);
      setPaymentMethod('cash');
    } catch (error: any) {
      const errMsg = error?.response?.data?.detail;
      if (typeof errMsg === 'string') {
        toast.error(errMsg);
      } else if (Array.isArray(errMsg)) {
        toast.error(errMsg.map((e: any) => e.msg || JSON.stringify(e)).join(', '));
      } else if (errMsg && typeof errMsg === 'object') {
        toast.error(JSON.stringify(errMsg));
      } else {
        toast.error('Помилка створення чеку');
      }
    } finally {
      setIsProcessing(false);
    }
  };

  const changeAmount =
    paymentMethod === 'cash' && parseFloat(cashAmount) >= subtotal
      ? parseFloat(cashAmount) - subtotal
      : 0;

  return (
    <div className="flex h-[calc(100vh-8rem)] gap-4">
      {/* Left panel - Product search */}
      <div className="w-80 flex flex-col gap-4">
        {/* Unified search field */}
        <div className="card p-4">
          <div className="relative">
            {/* Лупа — завжди зліва */}
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 pointer-events-none" />
            <input
              ref={searchInputRef}
              type="text"
              value={query}
              onChange={(e) => handleSearchChange(e.target.value)}
              placeholder="Пошук товару за назвою або штрих-кодом..."
              className="input-field pl-10 pr-10"
              id="unified-search"
              name="unified-search"
              autoComplete="off"
            />
            {isSearching && (
              <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-primary-500 animate-spin" />
            )}
            {query && !isSearching && (
              <button
                onClick={() => { resetSearch(); searchInputRef.current?.focus(); }}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
              >
                <X className="w-4 h-4" />
              </button>
            )}
          </div>
          {/* Error message (barcode not found) */}
          {error && query.length >= 8 && (
            <div className="mt-2 text-xs text-danger-500 bg-danger-50 dark:bg-danger-900/20 px-3 py-2 rounded-lg">
              {error}
            </div>
          )}
        </div>

        {/* Search results */}
        <div className="flex-1 overflow-y-auto space-y-2">
          {results.map((result) => {
            const stock = parseFloat(result.product.stock) || 0;
            const isOutOfStock = stock <= 0;

            return (
              <button
                key={result.product.id}
                onClick={() => handleProductSelect(result.product)}
                disabled={isOutOfStock}
                className={`
                  w-full card p-3 text-left transition-all group
                  ${isOutOfStock
                    ? 'opacity-50 cursor-not-allowed border-gray-200 dark:border-slate-700'
                    : 'hover:border-primary-300 dark:hover:border-primary-600'
                  }
                `}
              >
                <p className={`
                  font-medium text-sm
                  ${isOutOfStock
                    ? 'text-gray-400 dark:text-gray-500'
                    : 'text-gray-900 dark:text-gray-100 group-hover:text-primary-600'
                  }
                `}>
                  {result.product.title}
                </p>
                <div className="flex items-center justify-between mt-1">
                  <span className="text-xs text-gray-400">
                    {result.product.barcode || 'Без ШК'} | {result.product.stock} {formatUnit(result.product.unit)}
                    {isOutOfStock && (
                      <span className="ml-1 text-danger-500 font-medium">(немає)</span>
                    )}
                  </span>
                  <span className="font-bold text-primary-600">
                    {formatCurrency(result.product.price)}
                  </span>
                </div>
              </button>
            );
          })}
          {query.length >= 2 && results.length === 0 && !isSearching && !error && (
            <div className="text-center py-8 text-gray-400 text-sm">
              Товари не знайдено
            </div>
          )}
        </div>
      </div>

      {/* Center - Cart */}
      <div className="flex-1 card flex flex-col">
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-200 dark:border-slate-700">
          <div className="flex items-center gap-2">
            <ShoppingCart className="w-5 h-5 text-primary-600" />
            <h3 className="font-semibold text-gray-900 dark:text-gray-100">
              Кошик
            </h3>
            <span className="text-sm text-gray-400">({cart.length} поз.)</span>
          </div>
          {cart.length > 0 && (
            <button
              onClick={clearCart}
              className="text-sm text-danger-600 hover:text-danger-700"
            >
              Очистити
            </button>
          )}
        </div>

        <div className="flex-1 overflow-y-auto">
          {cart.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full text-gray-400">
              <ShoppingCart className="w-16 h-16 mb-3 opacity-30" />
              <p className="text-sm">Кошик порожній</p>
              <p className="text-xs mt-1">Знайдіть товар або відскануйте штрих-код</p>
            </div>
          ) : (
            <div className="divide-y divide-gray-100 dark:divide-slate-700">
              {cart.map((item) => {
                const isOverStock = item.quantity > item.stock;

                return (
                  <div
                    key={item.product_id}
                    className={`
                      p-4 hover:bg-gray-50 dark:hover:bg-slate-700/30
                      ${isOverStock ? 'bg-danger-50 dark:bg-danger-900/10' : ''}
                    `}
                  >
                    <div className="flex items-start justify-between">
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                          {item.product_title}
                        </p>
                        <p className="text-xs text-gray-400">
                          {formatCurrency(item.price)} / шт
                        </p>
                      </div>
                      <button
                        onClick={() => removeFromCart(item.product_id)}
                        className="p-1 text-gray-300 hover:text-danger-500 transition-colors"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>
                    <div className="flex items-center justify-between mt-2">
                      <div className="flex items-center gap-2">
                        <button
                          onClick={() => updateQuantity(item.product_id, -1)}
                          className="w-7 h-7 rounded-lg border border-gray-200 dark:border-slate-600 flex items-center justify-center text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700"
                        >
                          <Minus className="w-3 h-3" />
                        </button>
                        <input
                          type="number"
                          value={item.quantity}
                          onChange={(e) =>
                            setItemQuantity(item.product_id, parseFloat(e.target.value) || 0)
                          }
                          className="w-16 text-center input-field text-sm py-1 px-3"
                          min="0"
                          max={item.stock}
                          step={item.is_weight ? '0.1' : '1'}
                          id={`cart-qty-${item.product_id}`}
                          name={`cart-qty-${item.product_id}`}
                        />
                        <button
                          onClick={() => updateQuantity(item.product_id, 1)}
                          disabled={item.quantity >= item.stock}
                          className={`
                            w-7 h-7 rounded-lg border flex items-center justify-center
                            ${item.quantity >= item.stock
                              ? 'border-gray-100 dark:border-slate-700 text-gray-300 cursor-not-allowed'
                              : 'border-gray-200 dark:border-slate-600 text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700'
                            }
                          `}
                        >
                          <Plus className="w-3 h-3" />
                        </button>
                      </div>
                      <div className="flex items-center gap-2">
                        {isOverStock && (
                          <AlertTriangle className="w-4 h-4 text-danger-500" aria-label="Перевищує залишок" />
                        )}
                        <span className="font-semibold text-gray-900 dark:text-gray-100">
                          {formatCurrency(item.quantity * item.price)}
                        </span>
                      </div>
                    </div>
                    {/* Показуємо залишок під позицією */}
                    <div className="flex justify-between mt-1">
                      <span className="text-xs text-gray-400">
                        Залишок: {item.stock} {formatUnit(item.unit)}
                      </span>
                      {isOverStock && (
                        <span className="text-xs text-danger-500 font-medium">
                          Перевищує залишок на {(item.quantity - item.stock).toFixed(2)}
                        </span>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Cart summary */}
        {cart.length > 0 && (
          <div className="border-t border-gray-200 dark:border-slate-700 p-4 space-y-2">
            <div className="flex justify-between text-sm text-gray-500">
              <span>ПДВ</span>
              <span>{formatCurrency(vatAmount)}</span>
            </div>
            <div className="flex justify-between text-lg font-bold text-gray-900 dark:text-gray-100">
              <span>До сплати</span>
              <span className="text-primary-600">{formatCurrency(subtotal)}</span>
            </div>
            <Button
              className="w-full mt-2"
              size="lg"
              onClick={() => setShowPayment(true)}
            >
              ОПЛАТА
            </Button>
          </div>
        )}
      </div>

      {/* Quantity modal for weight products */}
      <Modal
        isOpen={showQuantityModal}
        onClose={() => {
          setShowQuantityModal(false);
          setSelectedProduct(null);
          setQuantityError(null);
        }}
        title="Введіть кількість"
        size="sm"
      >
        <div className="space-y-4">
          {selectedProduct && (
            <div>
              <p className="text-sm text-gray-600 dark:text-gray-400">
                {selectedProduct.title}
              </p>
              <p className="text-xs text-gray-400 mt-1">
                Доступно: {selectedProduct.stock} {formatUnit(selectedProduct.unit)}
              </p>
            </div>
          )}
          <Input
            type="number"
            step="0.1"
            min="0.001"
            max={selectedProduct ? parseFloat(selectedProduct.stock) || 0 : undefined}
            value={quantityInput}
            onChange={(e) => {
              setQuantityInput(e.target.value);
              setQuantityError(null);
            }}
            placeholder="Кількість"
            autoFocus
            id="weight-quantity"
            name="weight-quantity"
          />
          {quantityError && (
            <p className="text-xs text-danger-500 bg-danger-50 dark:bg-danger-900/20 px-3 py-2 rounded-lg">
              {quantityError}
            </p>
          )}
          <div className="flex justify-end gap-3">
            <Button
              variant="secondary"
              onClick={() => {
                setShowQuantityModal(false);
                setSelectedProduct(null);
                setQuantityError(null);
              }}
            >
              Скасувати
            </Button>
            <Button onClick={handleQuantityConfirm}>Додати</Button>
          </div>
        </div>
      </Modal>

      {/* Payment modal */}
      <Modal
        isOpen={showPayment}
        onClose={() => setShowPayment(false)}
        title="Оплата"
        size="lg"
      >
        <div className="space-y-6">
          <div className="text-center">
            <p className="text-3xl font-bold text-gray-900 dark:text-gray-100">
              {formatCurrency(subtotal)}
            </p>
            <p className="text-sm text-gray-500">Сума до сплати</p>
          </div>

          <Select
            label="Метод оплати"
            options={[
              { value: 'cash', label: 'Готівка' },
              { value: 'card', label: 'Картка' },
              { value: 'mixed', label: 'Змішаний' },
            ]}
            value={paymentMethod}
            onChange={(e) => setPaymentMethod(e.target.value as PaymentMethod)}
          />

          {paymentMethod === 'cash' && (
            <Input
              label="Сума готівки"
              type="number"
              step="0.01"
              min="0"
              value={cashAmount}
              onChange={(e) => setCashAmount(e.target.value)}
              placeholder="Введіть суму"
              icon={<Banknote className="w-4 h-4" />}
              id="cash-amount"
              name="cash-amount"
            />
          )}

          {paymentMethod === 'mixed' && (
            <div className="grid grid-cols-2 gap-4">
              <Input
                label="Готівка"
                type="number"
                step="0.01"
                min="0"
                value={cashAmount}
                onChange={(e) => setCashAmount(e.target.value)}
                icon={<Banknote className="w-4 h-4" />}
                id="mixed-cash"
                name="mixed-cash"
              />
              <Input
                label="Картка"
                type="number"
                step="0.01"
                min="0"
                value={cardAmount}
                onChange={(e) => setCardAmount(e.target.value)}
                icon={<CreditCard className="w-4 h-4" />}
                id="mixed-card"
                name="mixed-card"
              />
            </div>
          )}

          {paymentMethod === 'cash' && parseFloat(cashAmount) >= subtotal && (
            <div className="flex justify-between items-center p-3 bg-success-50 dark:bg-success-900/20 rounded-lg">
              <span className="text-sm font-medium text-success-700 dark:text-success-400">
                Решта
              </span>
              <span className="text-lg font-bold text-success-700 dark:text-success-400">
                {formatCurrency(changeAmount)}
              </span>
            </div>
          )}

          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={isDebt}
              onChange={(e) => setIsDebt(e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
              id="is-debt"
              name="is-debt"
            />
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Оплата в борг
            </span>
          </label>

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => setShowPayment(false)}>
              Скасувати
            </Button>
            <Button
              onClick={handlePayment}
              isLoading={isProcessing}
              size="lg"
            >
              {isDebt ? 'Оформити борг' : 'Сплатити'}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};

export default PosPage;
