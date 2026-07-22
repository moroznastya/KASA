import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Search, Plus, Minus, Trash2, ShoppingCart, CreditCard, Banknote, Loader2, X, AlertTriangle, UserPlus, Users, Layers, EyeOff, Settings2 } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { useUnifiedSearch } from '@/hooks/useUnifiedSearch';
import { receiptService } from '@/services/receiptService';
import { debtorService, Debtor } from '@/services/debtorService';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { formatCurrency, formatUnit } from '@/utils/format';
import { ReceiptCreate, PaymentMethod } from '@/types/receipt';
import toast from 'react-hot-toast';
import { CategoryBrowser } from '@/components/pos/CategoryBrowser';

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
  tax_rate: number;
  is_weight: boolean;
  stock: number;
  unit: string;
}

interface PaymentOption {
  value: PaymentMethod;
  label: string;
  icon: React.ReactNode;
}

const paymentOptions: PaymentOption[] = [
  { value: 'cash', label: 'Готівка', icon: <Banknote className="w-5 h-5" /> },
  { value: 'card', label: 'Картка', icon: <CreditCard className="w-5 h-5" /> },
  { value: 'mixed', label: 'Змішаний', icon: <CreditCard className="w-5 h-5" /> },
];

const PosPage: React.FC = () => {
  const navigate = useNavigate();
  const [cart, setCart] = useState<CartItem[]>([]);
  const cartEndRef = useRef<HTMLDivElement>(null);
  const [showPayment, setShowPayment] = useState(false);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod>('cash');
  const [cashAmount, setCashAmount] = useState('');
  const [cardAmount, setCardAmount] = useState('');
  const [isProcessing, setIsProcessing] = useState(false);
  const [showQuantityModal, setShowQuantityModal] = useState(false);
  const [selectedProduct, setSelectedProduct] = useState<any>(null);
  const [quantityInput, setQuantityInput] = useState('1');
  const [quantityError, setQuantityError] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Debtor state for payment
  const [debtorQuery, setDebtorQuery] = useState('');
  const [debtors, setDebtors] = useState<Debtor[]>([]);
  const [selectedDebtor, setSelectedDebtor] = useState<Debtor | null>(null);
  const [showDebtorField, setShowDebtorField] = useState(false);
  const [isSearchingDebtors, setIsSearchingDebtors] = useState(false);
  const [showDebtorDropdown, setShowDebtorDropdown] = useState(false);
  const debtorSearchRef = useRef<HTMLDivElement>(null);
  const debtorInputRef = useRef<HTMLInputElement>(null);
  
  // Debtor modal for partial payment
  const [showDebtorModal, setShowDebtorModal] = useState(false);
  const [debtorModalDebtor, setDebtorModalDebtor] = useState<Debtor | null>(null);
  const [debtorModalQuery, setDebtorModalQuery] = useState('');
  const [debtorModalResults, setDebtorModalResults] = useState<Debtor[]>([]);
  const [isSearchingDebtorModal, setIsSearchingDebtorModal] = useState(false);
  const [showDebtorModalDropdown, setShowDebtorModalDropdown] = useState(false);
  const debtorModalRef = useRef<HTMLDivElement>(null);
  const debtorModalInputRef = useRef<HTMLInputElement>(null);
  
  // Debtor payment method


  // Search debtors when query changes
  useEffect(() => {
    if (!debtorQuery.trim()) {
      setDebtors([]);
      setShowDebtorDropdown(false);
      return;
    }

    const timer = setTimeout(async () => {
      setIsSearchingDebtors(true);
      try {
        const results = await debtorService.search(debtorQuery);
        setDebtors(results);
        setShowDebtorDropdown(results.length > 0);
      } catch {
        // Ignore
      } finally {
        setIsSearchingDebtors(false);
      }
    }, 300);

    return () => clearTimeout(timer);
  }, [debtorQuery]);

  // Search debtors in modal
  useEffect(() => {
    if (!debtorModalQuery.trim()) {
      setDebtorModalResults([]);
      setShowDebtorModalDropdown(false);
      return;
    }
    const timer = setTimeout(async () => {
      setIsSearchingDebtorModal(true);
      try {
        const results = await debtorService.search(debtorModalQuery);
        setDebtorModalResults(results);
        setShowDebtorModalDropdown(results.length > 0);
      } catch {
        // Ignore
      } finally {
        setIsSearchingDebtorModal(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [debtorModalQuery]);

  // Close debtor modal dropdown on click outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (debtorModalRef.current && !debtorModalRef.current.contains(e.target as Node)) {
        setShowDebtorModalDropdown(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Close debtor dropdown on click outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (debtorSearchRef.current && !debtorSearchRef.current.contains(e.target as Node)) {
        setShowDebtorDropdown(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

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

    if (stock <= 0) {
      toast.error(`Товар "${product.title}" відсутній на складі`);
      return;
    }

    setCart((prev) => {
      const existing = prev.find((item) => item.product_id === product.id);
      const currentQtyInCart = existing ? existing.quantity : 0;
      const totalRequested = currentQtyInCart + quantity;

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

        if (stock <= 0) {
          setQuantityError('Товар відсутній на складі');
          return;
        }

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

      if (newQty > item.stock) {
        toast.error(
          `Недостатньо товару "${item.product_title}" на складі. Доступно: ${item.stock} ${formatUnit(item.unit)}`
        );
        return prev;
      }

      return prev
        .map((i) =>
          i.product_id === productId
            ? { ...i, quantity: Math.max(0, newQty) }
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

    // Автопрокрутка до останньої позиції при додаванні товару
  useEffect(() => {
    cartEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [cart.length]);

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

    // Визначаємо фактично сплачену суму
    let paidAmount: number;
    if (paymentMethod === 'cash') {
      paidAmount = parseFloat(cashAmount) || 0;
    } else if (paymentMethod === 'card') {
      paidAmount = subtotal; // карткою платять повну суму
    } else {
      // mixed
      paidAmount = (parseFloat(cashAmount) || 0) + (parseFloat(cardAmount) || 0);
    }

    // Якщо сума менша за чек — показуємо модалку боржника
    const isPartialPayment = paidAmount < subtotal;
    if (isPartialPayment) {
      setShowDebtorModal(true);
      return;
    }

    setIsProcessing(true);
    try {
      const receiptData: ReceiptCreate = {
        receipt_type: 'sale',
        total_amount: subtotal.toFixed(2),
        paid_amount: paidAmount.toFixed(2),
        debtor_id: selectedDebtor?.id || undefined,
        items: cart.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
      };

      await receiptService.createReceipt(receiptData);

      if (isPartialPayment) {
        const debtAmount = subtotal - paidAmount;
        toast.success(
          `Чек створено. Сплачено ${formatCurrency(paidAmount)}. ` +
          `Борг ${formatCurrency(debtAmount)} записано на "${selectedDebtor!.name}"`
        );
      } else {
        toast.success('Чек створено успішно');
      }

      setCart([]);
      setShowPayment(false);
      setShowDebtorField(false);
      setCashAmount('');
      setCardAmount('');
      setPaymentMethod('cash');
      setSelectedDebtor(null);
      setDebtorQuery('');
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

  const handleDebtorSelect = (debtor: Debtor) => {
    setSelectedDebtor(debtor);
    setDebtorQuery(debtor.name);
    setShowDebtorDropdown(false);
  };

  const handleDebtorModalSelect = (debtor: Debtor) => {
    setDebtorModalDebtor(debtor);
    setDebtorModalQuery(debtor.name);
    setShowDebtorModalDropdown(false);
  };


  const handleConfirmDebtorModal = async () => {
    let debtor = debtorModalDebtor;
    if (!debtor && debtorModalQuery.trim()) {
      try {
        debtor = await debtorService.create({ name: debtorModalQuery.trim() });
        setDebtorModalDebtor(debtor);
      } catch {
        toast.error('Помилка створення боржника');
        return;
      }
    }

    if (!debtor) {
      toast.error('Введіть ім\'я боржника');
      return;
    }
    
    // Визначаємо фактично сплачену суму
    let paidAmount: number;
    if (paymentMethod === 'cash') {
      paidAmount = parseFloat(cashAmount) || 0;
    } else if (paymentMethod === 'card') {
      paidAmount = subtotal;
    } else {
      paidAmount = (parseFloat(cashAmount) || 0) + (parseFloat(cardAmount) || 0);
    }
    
    setIsProcessing(true);
    try {
      const receiptData: ReceiptCreate = {
        receipt_type: 'sale',
        total_amount: subtotal.toFixed(2),
        paid_amount: paidAmount.toFixed(2),
        debtor_id: debtor.id,
        items: cart.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
      };

      await receiptService.createReceipt(receiptData);

      const debtAmount = subtotal - paidAmount;
      toast.success(
        `Чек створено. Сплачено ${formatCurrency(paidAmount)}. ` +
        `Борг ${formatCurrency(debtAmount)} записано на "${debtor.name}"`
      );

      setCart([]);
      setShowPayment(false);
      setShowDebtorModal(false);
      setCashAmount('');
      setCardAmount('');
      setPaymentMethod('cash');

      setDebtorModalDebtor(null);
      setDebtorModalQuery('');
      setSelectedDebtor(null);
      setDebtorQuery('');
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

  // Визначаємо чи сума оплати менша за чек
  const getPaidAmount = () => {
    if (paymentMethod === 'cash') return parseFloat(cashAmount) || 0;
    if (paymentMethod === 'card') return subtotal;
    return (parseFloat(cashAmount) || 0) + (parseFloat(cardAmount) || 0);
  };
  const isPartialPayment = showPayment && getPaidAmount() > 0 && getPaidAmount() < subtotal;

  return (
    <>
      {/* Category browser - horizontal bar above search and cart */}
      <CategoryBrowser onProductSelect={handleProductSelect} />

      <div className="flex h-[calc(100vh-8rem)] gap-4">
        {/* Left panel - Product search / Categories */}
      <div className="w-80 flex flex-col gap-4">
        {/* Unified search field */}
        <div className="card p-4">
          <div className="relative">
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
                        <p className="text-lg font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {item.product_title}
                        </p>
                        <p className="text-sm text-gray-400">
                          {formatCurrency(item.price)} / шт
                        </p>
                      </div>
                      <button
                        onClick={() => removeFromCart(item.product_id)}
                        className="p-1.5 text-red-500 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                        title="Видалити позицію"
                      >
                        <Trash2 className="w-6 h-6" />
                      </button>
                    </div>
                    <div className="flex items-center justify-between mt-2">
                      <div className="flex items-center gap-2">
                        <button
                          onClick={() => updateQuantity(item.product_id, -1)}
                          className="w-12 h-12 rounded-lg flex items-center justify-center text-2xl font-bold
                            text-white bg-red-500 hover:bg-red-600
                            dark:bg-red-600 dark:hover:bg-red-700
                            transition-colors shadow-sm"
                          title="Зменшити"
                        >
                          &minus;
                        </button>
                        <input
                          type="number"
                          value={item.quantity}
                          onChange={(e) =>
                            setItemQuantity(item.product_id, parseFloat(e.target.value) || 0)
                          }
                          className="w-24 h-12 text-center input-field !w-24 text-base font-semibold no-spinner"
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
                            w-12 h-12 rounded-lg flex items-center justify-center text-2xl font-bold
                            transition-colors shadow-sm
                            ${item.quantity >= item.stock
                              ? 'bg-gray-300 dark:bg-gray-600 text-gray-500 cursor-not-allowed'
                              : 'text-white bg-green-500 hover:bg-green-600 dark:bg-green-600 dark:hover:bg-green-700'
                            }
                          `}
                          title="Збільшити"
                        >
                          +
                        </button>
                      </div>
                      <div className="flex items-center gap-2">
                        {isOverStock && (
                          <AlertTriangle className="w-4 h-4 text-danger-500" aria-label="Перевищує залишок" />
                        )}
                        <span className="font-bold text-xl text-gray-900 dark:text-gray-100">
                          {formatCurrency(item.quantity * item.price)}
                        </span>
                      </div>
                    </div>
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
              <div ref={cartEndRef} />
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
            <div className="flex justify-between text-3xl font-bold text-gray-900 dark:text-gray-100">
              <span>До сплати</span>
              <span className="text-primary-600">{formatCurrency(subtotal)}</span>
            </div>
            <div className="flex gap-2">
              <Button
                className="flex-1"
                size="lg"
                onClick={() => setShowPayment(true)}
              >
                ОПЛАТА
              </Button>
              <Button
                variant="secondary"
                size="lg"
                onClick={() => navigate('/debtors')}
                title="Перейти до списку боржників"
              >
                <Users className="w-5 h-5" />
              </Button>
            </div>
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
        onClose={() => {
          setShowPayment(false);
          setShowDebtorField(false);
          setSelectedDebtor(null);
          setDebtorQuery('');
        }}
        title="Оплата"
        size="4xl"
      >
        <div className="space-y-6">
          <div className="text-center">
            <p className="text-3xl font-bold text-gray-900 dark:text-gray-100">
              {formatCurrency(subtotal)}
            </p>
            <p className="text-sm text-gray-500">Сума до сплати</p>
          </div>

          {/* Payment method buttons */}
          <div>
            <p className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Спосіб оплати
            </p>
            <div className="grid grid-cols-3 gap-2">
              {paymentOptions.map((option) => (
                <button
                  key={option.value}
                  onClick={() => setPaymentMethod(option.value)}
                  className={`
                    flex flex-col items-center gap-1.5 py-3 px-2 rounded-xl border-2 transition-all
                    ${
                      paymentMethod === option.value
                        ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-400'
                        : 'border-gray-200 dark:border-slate-600 bg-white dark:bg-slate-700 text-gray-600 dark:text-gray-400 hover:border-gray-300 dark:hover:border-slate-500'
                    }
                  `}
                >
                  {option.icon}
                  <span className="text-xs font-medium">{option.label}</span>
                </button>
              ))}
            </div>
          </div>

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
                onChange={(e) => {
                  const val = e.target.value;
                  setCashAmount(val);
                  // Автоматично розраховуємо картку: сума до сплати - готівка
                  const cash = parseFloat(val) || 0;
                  const remaining = subtotal - cash;
                  if (remaining >= 0) {
                    setCardAmount(remaining.toFixed(2));
                  } else {
                    setCardAmount('0');
                  }
                }}
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
                onChange={(e) => {
                  const val = e.target.value;
                  setCardAmount(val);
                  // Автоматично розраховуємо готівку: сума до сплати - картка
                  const card = parseFloat(val) || 0;
                  const remaining = subtotal - card;
                  if (remaining >= 0) {
                    setCashAmount(remaining.toFixed(2));
                  } else {
                    setCashAmount('0');
                  }
                }}
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

          {/* Partial payment warning + debtor selection */}
          {isPartialPayment && (
            <div className="p-3 bg-amber-50 dark:bg-amber-900/20 rounded-lg border border-amber-200 dark:border-amber-700">
              <p className="text-sm font-medium text-amber-700 dark:text-amber-400">
                Недостатня сума оплати
              </p>
              <p className="text-xs text-amber-600 dark:text-amber-500 mt-1">
                Сума до сплати: {formatCurrency(subtotal)} | 
                Внесено: {formatCurrency(getPaidAmount())} | 
                <span className="font-bold"> Борг: {formatCurrency(subtotal - getPaidAmount())}</span>
              </p>
            </div>
          )}

          {/* Debtor selection — показано коли сума менша за чек */}
          {(showDebtorField || selectedDebtor) && (
            <div ref={debtorSearchRef} className="relative">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                {isPartialPayment ? 'Боржник (обов\'язково)' : 'Боржник (необов\'язково)'}
              </label>
              <div className="relative">
                <input
                  ref={debtorInputRef}
                  type="text"
                  value={debtorQuery}
                  onChange={(e) => {
                    setDebtorQuery(e.target.value);
                    setSelectedDebtor(null);
                  }}
                  placeholder="Введіть ім'я боржника..."
                  className="input-field pl-10 pr-10"
                  autoFocus={isPartialPayment}
                  id="debtor-name"
                  name="debtor-name"
                  autoComplete="off"
                />
                <UserPlus className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
                {isSearchingDebtors && (
                  <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-primary-500 animate-spin" />
                )}
              </div>

              {/* Debtor dropdown */}
              {showDebtorDropdown && debtors.length > 0 && (
                <div className="absolute z-50 w-full mt-1 bg-white dark:bg-slate-700 border border-gray-200 dark:border-slate-600 rounded-lg shadow-lg overflow-hidden">
                  {debtors.map((debtor) => (
                    <button
                      key={debtor.id}
                      onClick={() => handleDebtorSelect(debtor)}
                      className="w-full px-4 py-2.5 text-left text-sm hover:bg-gray-50 dark:hover:bg-slate-600 transition-colors flex items-center justify-between"
                    >
                      <span className="font-medium text-gray-900 dark:text-gray-100">
                        {debtor.name}
                      </span>
                      {debtor.total_debt > 0 && (
                        <span className="text-xs text-danger-500 font-medium">
                          Борг: {formatCurrency(debtor.total_debt)}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              )}



              {selectedDebtor && (
                <div className="mt-2 px-3 py-2 bg-primary-50 dark:bg-primary-900/20 rounded-lg flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium text-primary-700 dark:text-primary-400">
                      {selectedDebtor.name}
                    </p>
                    {selectedDebtor.total_debt > 0 && (
                      <p className="text-xs text-danger-500">
                        Поточний борг: {formatCurrency(selectedDebtor.total_debt)}
                      </p>
                    )}
                  </div>
                  <button
                    onClick={() => {
                      setSelectedDebtor(null);
                      setDebtorQuery('');
                      debtorInputRef.current?.focus();
                    }}
                    className="text-xs text-gray-400 hover:text-gray-600"
                  >
                    Змінити
                  </button>
                </div>
              )}
            </div>
          )}

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => {
              setShowPayment(false);
              setShowDebtorField(false);
              setSelectedDebtor(null);
              setDebtorQuery('');
            }}>
              Скасувати
            </Button>
            <Button
              onClick={handlePayment}
              isLoading={isProcessing}
              size="lg"
            >
              Сплатити
            </Button>
          </div>
        </div>
      </Modal>

      {/* Модалка боржника для неповної оплати */}
      <Modal
        isOpen={showDebtorModal}
        onClose={() => {
          setShowDebtorModal(false);
          setDebtorModalDebtor(null);
          setDebtorModalQuery('');
        }}
        title="Недостатня сума оплати"
        size="md"
      >
        <div className="space-y-4">
          <div className="p-4 bg-amber-50 dark:bg-amber-900/20 rounded-lg border border-amber-200 dark:border-amber-700">
            <p className="text-sm font-medium text-amber-700 dark:text-amber-400">
              Сума оплати менша за суму чеку.
            </p>
            <p className="text-sm text-amber-600 dark:text-amber-500 mt-1">
              Решта ({formatCurrency(subtotal - getPaidAmount())}) буде записана в борг.
            </p>
            <div className="flex justify-between text-sm mt-2 pt-2 border-t border-amber-200 dark:border-amber-700">
              <span className="text-amber-700 dark:text-amber-400">До сплати:</span>
              <span className="font-bold text-amber-700 dark:text-amber-400">{formatCurrency(subtotal)}</span>
            </div>
            <div className="flex justify-between text-sm">
              <span className="text-amber-700 dark:text-amber-400">Сплачено:</span>
              <span className="font-bold text-amber-700 dark:text-amber-400">{formatCurrency(getPaidAmount())}</span>
            </div>
            <div className="flex justify-between text-sm">
              <span className="text-danger-600 font-medium">Борг:</span>
              <span className="font-bold text-danger-600">{formatCurrency(subtotal - getPaidAmount())}</span>
            </div>
          </div>

          <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
            <p className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
              Оберіть або створіть боржника <span className="text-danger-500">*</span>
            </p>
            
            <div ref={debtorModalRef} className="relative">
              <div className="relative">
                <input
                  ref={debtorModalInputRef}
                  type="text"
                  value={debtorModalQuery}
                  onChange={(e) => {
                    setDebtorModalQuery(e.target.value);
                    setDebtorModalDebtor(null);
                  }}
                  placeholder="Введіть ім'я боржника..."
                  className="input-field pl-10 pr-10"
                  autoFocus
                  id="debtor-modal-name"
                  name="debtor-modal-name"
                  autoComplete="off"
                />
                <UserPlus className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
                {isSearchingDebtorModal && (
                  <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-primary-500 animate-spin" />
                )}
              </div>

              {/* Dropdown */}
              {showDebtorModalDropdown && debtorModalResults.length > 0 && (
                <div className="absolute z-50 w-full mt-1 bg-white dark:bg-slate-700 border border-gray-200 dark:border-slate-600 rounded-lg shadow-lg overflow-hidden max-h-48 overflow-y-auto">
                  {debtorModalResults.map((debtor) => (
                    <button
                      key={debtor.id}
                      onClick={() => handleDebtorModalSelect(debtor)}
                      className="w-full px-4 py-2.5 text-left text-sm hover:bg-gray-50 dark:hover:bg-slate-600 transition-colors flex items-center justify-between"
                    >
                      <span className="font-medium text-gray-900 dark:text-gray-100">
                        {debtor.name}
                      </span>
                      {debtor.total_debt > 0 && (
                        <span className="text-xs text-danger-500 font-medium">
                          Борг: {formatCurrency(debtor.total_debt)}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              )}



              {debtorModalDebtor && (
                <div className="mt-2 px-3 py-2 bg-primary-50 dark:bg-primary-900/20 rounded-lg flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium text-primary-700 dark:text-primary-400">
                      {debtorModalDebtor.name}
                    </p>
                    {debtorModalDebtor.total_debt > 0 && (
                      <p className="text-xs text-danger-500">
                        Поточний борг: {formatCurrency(debtorModalDebtor.total_debt)}
                      </p>
                    )}
                  </div>
                  <button
                    onClick={() => {
                      setDebtorModalDebtor(null);
                      setDebtorModalQuery('');
                      debtorModalInputRef.current?.focus();
                    }}
                    className="text-xs text-gray-400 hover:text-gray-600"
                  >
                    Змінити
                  </button>
                </div>
              )}
            </div>
          </div>

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => {
              setShowDebtorModal(false);
              setDebtorModalDebtor(null);
              setDebtorModalQuery('');
            }}>
              Скасувати
            </Button>
            <Button
              onClick={handleConfirmDebtorModal}
              isLoading={isProcessing}
              disabled={!debtorModalQuery.trim() && !debtorModalDebtor}
            >
              {debtorModalDebtor
                ? `Створити чек (борг на ${debtorModalDebtor.name})`
                : debtorModalQuery.trim()
                  ? `Створити боржника "${debtorModalQuery.trim()}"`
                  : 'Оберіть боржника'}
            </Button>
          </div>
        </div>
      </Modal>

    </div>
    </>
  );
};

export default PosPage;
