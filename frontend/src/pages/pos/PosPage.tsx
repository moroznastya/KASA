import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Search, Plus, Minus, Trash2, ShoppingCart, CreditCard, Banknote, Loader2, X, AlertTriangle, UserPlus, Users, FolderOpen, PanelRightClose, PanelRightOpen } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { useUnifiedSearch } from '@/hooks/useUnifiedSearch';
import { receiptService } from '@/services/receiptService';
import { debtorService, Debtor } from '@/services/debtorService';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { formatCurrency, formatUnit } from '@/utils/format';
import { CategoryPanel } from '@/components/pos/CategoryPanel';
import { ReceiptCreate, PaymentMethod } from '@/types/receipt';
import { Product } from '@/types/product';
import { categoryService } from '@/services/categoryService';
import { productService } from '@/services/productService';
import toast from 'react-hot-toast';

// ========== Інтерфейси ==========
interface CategoryNode {
  id: string;
  name: string;
  parent_id: string | null;
  children: CategoryNode[];
}

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
  const [panelMode, setPanelMode] = useState<'search' | 'categories'>('search');
  
  // Collapse/Expand для правої панелі каталогу
  const [catalogCollapsed, setCatalogCollapsed] = useState(false);
  
  // Категорії для горизонтальної панелі
  const [categories, setCategories] = useState<CategoryNode[]>([]);
  const [categoriesLoading, setCategoriesLoading] = useState(true);
  const [selectedCategoryId, setSelectedCategoryId] = useState<string | null>(null);
  const [selectedCategoryName, setSelectedCategoryName] = useState('');
  const [categoryProducts, setCategoryProducts] = useState<Product[]>([]);
  const [categoryProductsLoading, setCategoryProductsLoading] = useState(false);
  
  // Debtor modal for partial payment
  const [showDebtorModal, setShowDebtorModal] = useState(false);
  const [debtorModalDebtor, setDebtorModalDebtor] = useState<Debtor | null>(null);
  const [debtorModalQuery, setDebtorModalQuery] = useState('');
  const [debtorModalResults, setDebtorModalResults] = useState<Debtor[]>([]);
  const [isSearchingDebtorModal, setIsSearchingDebtorModal] = useState(false);
  const [showDebtorModalDropdown, setShowDebtorModalDropdown] = useState(false);
  const debtorModalRef = useRef<HTMLDivElement>(null);
  const debtorModalInputRef = useRef<HTMLInputElement>(null);

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

  // Завантажуємо категорії для горизонтальної панелі
  useEffect(() => {
    const load = async () => {
      setCategoriesLoading(true);
      try {
        const tree = await categoryService.getCategoryTree();
        setCategories(tree as unknown as CategoryNode[]);
      } catch (err) {
        console.error('Помилка завантаження категорій:', err);
      } finally {
        setCategoriesLoading(false);
      }
    };
    load();
  }, []);

  // Завантажуємо товари при виборі категорії
  useEffect(() => {
    if (!selectedCategoryId) {
      setCategoryProducts([]);
      return;
    }
    const load = async () => {
      setCategoryProductsLoading(true);
      try {
        const response = await productService.getProducts({
          category_id: selectedCategoryId,
          size: 100,
        });
        setCategoryProducts(response.items);
      } catch (err) {
        console.error('Помилка завантаження товарів:', err);
        setCategoryProducts([]);
      } finally {
        setCategoryProductsLoading(false);
      }
    };
    load();
  }, [selectedCategoryId]);

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

  // ========== Категорії ==========
  const handleCategoryClick = useCallback((cat: CategoryNode) => {
    setSelectedCategoryId(cat.id);
    setSelectedCategoryName(cat.name);
  }, []);

  const clearCategorySelection = useCallback(() => {
    setSelectedCategoryId(null);
    setSelectedCategoryName('');
    setCategoryProducts([]);
  }, []);

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

  const handleCreateAndSelectDebtorModal = async () => {
    if (!debtorModalQuery.trim()) {
      toast.error("Введіть ім'я боржника");
      return;
    }
    try {
      const newDebtor = await debtorService.create({ name: debtorModalQuery.trim() });
      setDebtorModalDebtor(newDebtor);
      setDebtorModalQuery(newDebtor.name);
      setShowDebtorModalDropdown(false);
      toast.success(`Боржника "${newDebtor.name}" створено`);
    } catch {
      toast.error('Помилка створення боржника');
    }
  };

  const handleConfirmDebtorModal = async () => {
    if (!debtorModalDebtor) {
      toast.error('Оберіть або створіть боржника');
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
        debtor_id: debtorModalDebtor.id,
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
        `Борг ${formatCurrency(debtAmount)} записано на "${debtorModalDebtor.name}"`
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

  const handleCreateAndSelectDebtor = async () => {
    if (!debtorQuery.trim()) {
      toast.error('Введіть ім\'я боржника');
      return;
    }
    try {
      const newDebtor = await debtorService.create({ name: debtorQuery.trim() });
      setSelectedDebtor(newDebtor);
      setDebtorQuery(newDebtor.name);
      setShowDebtorDropdown(false);
      toast.success(`Боржника "${newDebtor.name}" створено`);
    } catch {
      toast.error('Помилка створення боржника');
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
    <div className="flex flex-col h-[calc(100vh-8rem)] gap-0">
      {/* Кнопка collapse/expand — інтегрована в ліву панель */}
      {catalogCollapsed && (
        <div className="flex flex-col items-center pt-2">
          <button
            onClick={() => setCatalogCollapsed(false)}
            className="p-2 rounded-lg border border-gray-200 dark:border-slate-700 hover:bg-gray-100 dark:hover:bg-slate-700 transition-all text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
            title="Показати каталог"
          >
            <PanelRightOpen className="w-5 h-5" />
          </button>
        </div>
      )}

      {/* Left panel - Product search / Categories */}
      <div className={`${catalogCollapsed ? 'w-0 overflow-hidden opacity-0 p-0 m-0' : 'w-80'} flex flex-col gap-4 transition-all duration-300 ease-in-out`}>
        {/* Unified search field */}
        <div className="card p-4 relative">
          {/* Кнопка collapse всередині панелі */}
          <button
            onClick={() => setCatalogCollapsed(true)}
            className="absolute -right-3 top-4 w-6 h-6 rounded-full bg-white dark:bg-slate-700 border border-gray-200 dark:border-slate-600 flex items-center justify-center text-gray-400 hover:text-gray-600 hover:border-gray-300 transition-all shadow-sm z-10"
            title="Приховати каталог"
          >
            <PanelRightClose className="w-3.5 h-3.5" />
          </button>
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

        {/* Перемикач режимів */}
        <div className="flex bg-gray-100 dark:bg-slate-700 rounded-lg p-0.5">
          <span
            onClick={() => setPanelMode('search')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-medium rounded-md cursor-pointer transition-all ${
              panelMode === 'search'
                ? 'bg-white dark:bg-slate-600 text-primary-600 dark:text-primary-400 shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
            }`}
          >
            <Search className="w-3.5 h-3.5" />
            Пошук
          </span>
          <span
            onClick={() => setPanelMode('categories')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-medium rounded-md cursor-pointer transition-all ${
              panelMode === 'categories'
                ? 'bg-white dark:bg-slate-600 text-primary-600 dark:text-primary-400 shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
            }`}
          >
            <FolderOpen className="w-3.5 h-3.5" />
            Категорії
          </span>
        </div>

        {/* Контент: пошук або категорії */}
        {panelMode === 'search' ? (
          <>
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
          </>
        ) : (
          <div className="flex-1 overflow-y-auto">
            <CategoryPanel onProductSelect={handleProductSelect} />
          </div>
        )}
      </div>

      {/* ===== Горизонтальна панель категорій під хедером ===== */}
      <div className="flex-shrink-0 bg-white dark:bg-slate-800 border-b border-gray-200 dark:border-slate-700 px-4 py-2 overflow-x-auto">
        {categoriesLoading ? (
          <div className="flex items-center gap-2 py-1">
            <Loader2 className="w-4 h-4 text-primary-500 animate-spin" />
            <span className="text-xs text-gray-400">Завантаження...</span>
          </div>
        ) : categories.length === 0 ? (
          <span className="text-xs text-gray-400">Немає категорій</span>
        ) : (
          <div className="flex items-center gap-2">
            {categories.map((cat, idx) => {
              const isSelected = selectedCategoryId === cat.id;
              const colors = [
                'bg-blue-100 text-blue-700 border-blue-300 hover:bg-blue-200',
                'bg-emerald-100 text-emerald-700 border-emerald-300 hover:bg-emerald-200',
                'bg-amber-100 text-amber-700 border-amber-300 hover:bg-amber-200',
                'bg-rose-100 text-rose-700 border-rose-300 hover:bg-rose-200',
                'bg-violet-100 text-violet-700 border-violet-300 hover:bg-violet-200',
                'bg-cyan-100 text-cyan-700 border-cyan-300 hover:bg-cyan-200',
                'bg-orange-100 text-orange-700 border-orange-300 hover:bg-orange-200',
                'bg-teal-100 text-teal-700 border-teal-300 hover:bg-teal-200',
                'bg-pink-100 text-pink-700 border-pink-300 hover:bg-pink-200',
                'bg-indigo-100 text-indigo-700 border-indigo-300 hover:bg-indigo-200',
              ];
              const color = colors[idx % colors.length];
              
              return (
                <button
                  key={cat.id}
                  onClick={() => handleCategoryClick(cat)}
                  className={`
                    flex-shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-xl border text-xs font-semibold transition-all whitespace-nowrap
                    ${isSelected 
                      ? 'bg-primary-500 text-white border-primary-500 shadow-sm' 
                      : color
                    }
                  `}
                >
                  {cat.name}
                  {cat.children && cat.children.length > 0 && (
                    <span className={`text-[10px] ${isSelected ? 'text-white/70' : 'opacity-60'}`}>
                      {cat.children.length}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* ===== Основний контент: пошук зліва + чек по центру ===== */}
      <div className="flex flex-1 gap-4 min-h-0">
      <div className={`flex-1 transition-all duration-300 ease-in-out ${catalogCollapsed ? 'max-w-none' : ''}`}>
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
      </div>
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
        size="lg"
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

              {/* Create new debtor hint */}
              {debtorQuery.trim() && !selectedDebtor && !isSearchingDebtors && (
                <button
                  onClick={handleCreateAndSelectDebtor}
                  className="mt-1 w-full px-4 py-2 text-sm text-primary-600 hover:text-primary-700 hover:bg-primary-50 dark:hover:bg-primary-900/20 rounded-lg transition-colors flex items-center gap-2"
                >
                  <UserPlus className="w-4 h-4" />
                  <span>Створити боржника &quot;{debtorQuery.trim()}&quot;</span>
                </button>
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

              {/* Create new debtor */}
              {debtorModalQuery.trim() && !debtorModalDebtor && !isSearchingDebtorModal && (
                <button
                  onClick={handleCreateAndSelectDebtorModal}
                  className="mt-1 w-full px-4 py-2 text-sm text-primary-600 hover:text-primary-700 hover:bg-primary-50 dark:hover:bg-primary-900/20 rounded-lg transition-colors flex items-center gap-2"
                >
                  <UserPlus className="w-4 h-4" />
                  <span>Створити боржника &quot;{debtorModalQuery.trim()}&quot;</span>
                </button>
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
              disabled={!debtorModalDebtor}
            >
              {debtorModalDebtor
                ? `Створити чек (борг на ${debtorModalDebtor.name})`
                : 'Оберіть боржника'}
            </Button>
          </div>
        </div>
      </Modal>

    </div>
  );
};

export default PosPage;
