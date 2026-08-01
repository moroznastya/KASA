import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Search, Plus, Minus, Trash2, ShoppingCart, CreditCard, Banknote, Loader2, X, AlertTriangle, UserPlus, Users, User, Layers, EyeOff, Settings2, DollarSign, BadgePercent, RotateCcw, Clock, FileCheck2, Wifi, WifiOff, PlayCircle, StopCircle } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { useUnifiedSearch } from '@/hooks/useUnifiedSearch';
import { receiptService } from '@/services/receiptService';
import { debtorService, Debtor } from '@/services/debtorService';
import { settingsService } from '@/services/settingsService';
import { prroService } from '@/services/prroService';
import { usePrroStore, startPrroStatusPolling } from '@/store/prroStore';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { formatCurrency, formatUnit } from '@/utils/format';
import { Receipt, ReceiptCreate, PaymentMethod } from '@/types/receipt';
import toast from 'react-hot-toast';
import { CategoryBrowser } from '@/components/pos/CategoryBrowser';
import ProductCardModal from '@/components/pos/ProductCardModal';
import PrintReceiptDialog from '@/components/pos/PrintReceiptDialog';
import SearchReceiptModal from '@/components/pos/SearchReceiptModal';
import SelectItemsFromReceipt from '@/components/pos/SelectItemsFromReceipt';
import type { ReturnCartItem } from '@/components/pos/SelectItemsFromReceipt';
import ReturnWithoutReceipt from '@/components/pos/ReturnWithoutReceipt';
import type { ReceiptSearchResult } from '@/types/receipt';

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  image_url: string | null;
  quantity: number;
  price: number;
  tax_rate: number;
  is_weight: boolean;
  stock: number;
  unit: string;
  original_receipt_id?: string;  // ID оригінального чеку (для товарів повернення)
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

const DEBT_PRODUCT_ID = 'c230fe32-78ef-4501-a21d-71467a668fc4';

const PosPage: React.FC = () => {
  const navigate = useNavigate();
  const [cart, setCart] = useState<CartItem[]>(() => {
    try {
      const saved = sessionStorage.getItem('pos_cart');
      return saved ? JSON.parse(saved) : [];
    } catch {
      return [];
    }
  });
  const [editingQuantity, setEditingQuantity] = useState<Record<string, string>>({});
  const cartEndRef = useRef<HTMLDivElement>(null);
  const [showPayment, setShowPayment] = useState(false);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod>('cash');
  const [cashAmount, setCashAmount] = useState('');
  const [cardAmount, setCardAmount] = useState('');
  const [isProcessing, setIsProcessing] = useState(false);
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

  // Debt payment state
  const [showDebtPaymentSearch, setShowDebtPaymentSearch] = useState(false);
  const [debtPaymentQuery, setDebtPaymentQuery] = useState('');
  const [debtPaymentResults, setDebtPaymentResults] = useState<Debtor[]>([]);
  const [isSearchingDebtPayment, setIsSearchingDebtPayment] = useState(false);
  const [showDebtPaymentDropdown, setShowDebtPaymentDropdown] = useState(false);
  const [selectedDebtPaymentDebtor, setSelectedDebtPaymentDebtor] = useState<Debtor | null>(null);
  const [debtPaymentAmount, setDebtPaymentAmount] = useState('');
  const [showDebtPaymentModal, setShowDebtPaymentModal] = useState(false);
  const [returnMode, setReturnMode] = useState(false);
  const [showPrintDialog, setShowPrintDialog] = useState(false);
  const [lastReceipt, setLastReceipt] = useState<Receipt | null>(null);
  const [autoPrintReceipt, setAutoPrintReceipt] = useState(true);
  const [fiscalStatus, setFiscalStatus] = useState<{
    receipt_id: string;
    fiscal_status: string;
    fiscal_number: string | null;
    fiscal_error: string | null;
    fiscal_check_url: string | null;
  } | null>(null);
  
  // Стани для модалок повернення
  const [showSearchReceiptModal, setShowSearchReceiptModal] = useState(false);
  const [selectedSourceReceipt, setSelectedSourceReceipt] = useState<ReceiptSearchResult | null>(null);
  const [showSelectItemsModal, setShowSelectItemsModal] = useState(false);
  const [searchReceiptId, setSearchReceiptId] = useState(0);
  const [showReturnWithoutReceipt, setShowReturnWithoutReceipt] = useState(false); // за замовчуванням друкуємо автоматично

  // Settings
  const [showCardOnScan, setShowCardOnScan] = useState(true);

  // Product card modal
  const [showProductCard, setShowProductCard] = useState(false);
  const [productCardProduct, setProductCardProduct] = useState<any>(null);

  const [heldReceipt, setHeldReceipt] = useState<CartItem[] | null>(() => {
    try {
      const saved = localStorage.getItem('pos_held_receipt');
      return saved ? JSON.parse(saved) : null;
    } catch {
      return null;
    }
  });

  // Статус ПРРО для індикатора в шапці POS
  const prroStatus = usePrroStore((s) => s.status);
  const loadPrroStatus = usePrroStore((s) => s.loadStatus);
  const prroFiscalizing = usePrroStore((s) => s.fiscalizing);

  // Авто-оновлення статусу ПРРО (кожні 30 секунд)
  useEffect(() => {
    loadPrroStatus();
    const stopPolling = startPrroStatusPolling();
    return stopPolling;
  }, [loadPrroStatus]);

  // Save cart to sessionStorage on change
  useEffect(() => {
    try {
      sessionStorage.setItem('pos_cart', JSON.stringify(cart));
    } catch {
      // Ignore storage errors
    }
  }, [cart]);

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

  // Search debtors for debt payment
  useEffect(() => {
    if (!debtPaymentQuery.trim() || debtPaymentQuery.trim().length < 2) {
      setDebtPaymentResults([]);
      setShowDebtPaymentDropdown(false);
      return;
    }
    const timer = setTimeout(async () => {
      setIsSearchingDebtPayment(true);
      try {
        const results = await debtorService.search(debtPaymentQuery.trim());
        setDebtPaymentResults(results);
        setShowDebtPaymentDropdown(results.length > 0);
      } catch {
        setDebtPaymentResults([]);
      } finally {
        setIsSearchingDebtPayment(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [debtPaymentQuery]);

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

  // Ref для handleBarcodeFound, щоб уникнути циклічної залежності
  const handleBarcodeFoundRef = useRef<(product: any) => void>((_product: any) => {});

  const {
    query,
    results,
    isSearching,
    error,
    setQuery: handleSearchChange,
    reset: resetSearch,
  } = useUnifiedSearch({
    onBarcodeFound: (product: any) => handleBarcodeFoundRef.current(product),
  });
  const [debtorSearchResults, setDebtorSearchResults] = useState<Debtor[]>([]);
  const [isSearchingDebtorsUnified, setIsSearchingDebtorsUnified] = useState(false);


  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  // Завантажуємо налаштування
  useEffect(() => {
    settingsService.getValue('show_product_card_on_scan').then((value) => {
      setShowCardOnScan(value === 'true');
    }).catch(() => {
      setShowCardOnScan(true);
    });

    settingsService.getValue('auto_print_receipt').then((value) => {
      setAutoPrintReceipt(value === 'true');
    }).catch(() => {
      setAutoPrintReceipt(true);
    });
  }, []);

  // Search debtors in unified search
  useEffect(() => {
    if (!query.trim() || query.trim().length < 2) {
      setDebtorSearchResults([]);
      return;
    }
    
    const timer = setTimeout(async () => {
      setIsSearchingDebtorsUnified(true);
      try {
        const results = await debtorService.search(query.trim());
        setDebtorSearchResults(results);
      } catch {
        setDebtorSearchResults([]);
      } finally {
        setIsSearchingDebtorsUnified(false);
      }
    }, 400);
    
    return () => clearTimeout(timer);
  }, [query]);


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
      // Знайти головне зображення або перше зображення
      const mainImage = product.images?.find((img: any) => img.is_main) || product.images?.[0];
      return [
        ...prev,
        {
          product_id: product.id,
          product_title: product.title,
          product_barcode: product.barcode,
          image_url: mainImage?.url || null,
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

  const handleBarcodeFound = useCallback((product: any) => {
    const stock = parseFloat(product.stock) || 0;
    if (stock <= 0) {
      toast.error(`Товар "${product.title}" відсутній на складі`);
      return;
    }
    if (showCardOnScan || product.is_weight) {
      // Показуємо картку товару (для вагових — завжди, щоб ввести вагу)
      setProductCardProduct(product);
      setShowProductCard(true);
    } else {
      // Додаємо напряму в кошик
      addToCart(product);
      toast.success(`Додано: ${product.title}`);
    }
  }, [showCardOnScan, addToCart]);

  // Оновлюємо ref в useEffect, а не під час рендеру
  useEffect(() => {
    handleBarcodeFoundRef.current = handleBarcodeFound;
  }, [handleBarcodeFound]);

  const handleProductSelect = (product: any) => {
    const stock = parseFloat(product.stock) || 0;
    if (stock <= 0) {
      toast.error(`Товар "${product.title}" відсутній на складі`);
      return;
    }
    if (showCardOnScan || product.is_weight) {
      // Показуємо картку товару (для вагових — завжди, щоб ввести вагу)
      setProductCardProduct(product);
      setShowProductCard(true);
    } else {
      // Додаємо напряму
      addToCart(product);
    }
  };

  const handleHoldReceipt = useCallback(() => {
    if (cart.length === 0) {
      toast.error('Кошик порожній');
      return;
    }
    localStorage.setItem('pos_held_receipt', JSON.stringify(cart));
    setHeldReceipt(cart);
    setCart([]);
    sessionStorage.removeItem('pos_cart');
    toast.success('Чек відкладено');
  }, [cart]);

  const handleRestoreHeldReceipt = useCallback(() => {
    try {
      const saved = localStorage.getItem('pos_held_receipt');
      if (saved) {
        const items: CartItem[] = JSON.parse(saved);
        setCart(items);
        localStorage.removeItem('pos_held_receipt');
        setHeldReceipt(null);
        toast.success('Відкладений чек відновлено');
      }
    } catch {
      toast.error('Помилка відновлення чеку');
    }
  }, []);

  const handleAddFromCard = useCallback((product: any, quantity: number) => {
    addToCart(product, quantity);
    toast.success(`Додано: ${product.title} × ${quantity}`);
  }, [addToCart]);

;

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
    sessionStorage.removeItem('pos_cart');
  };

  // Автопрокрутка до останньої позиції при додаванні товару
  useEffect(() => {
    cartEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [cart.length]);

  // Автофокус та виділення поля суми готівки при відкритті модалки оплати
  useEffect(() => {
    if (showPayment) {
      setTimeout(() => {
        const el = document.getElementById('cash-amount') as HTMLInputElement;
        if (el) {
          el.focus();
          el.select();
        }
      }, 150);
    }
  }, [showPayment]);

  const subtotal = cart.reduce(
    (sum, item) => sum + (item.is_weight ? Math.ceil(item.quantity * item.price) : item.quantity * item.price),
    0
  );
  const vatAmount = cart.reduce(
    (sum, item) => sum + (item.quantity * item.price * item.tax_rate) / (100 + item.tax_rate),
    0
  );

  const handleDebtPaymentSelect = (debtor: Debtor) => {
    setSelectedDebtPaymentDebtor(debtor);
    setDebtPaymentQuery(debtor.name);
    setShowDebtPaymentDropdown(false);
    // Default to full debt amount
    setDebtPaymentAmount(debtor.total_debt.toString());
  };
  const handleUnifiedDebtorSelect = (debtor: Debtor) => {
    // Set the debtor and pre-fill amount, skip debtor selection step
    setSelectedDebtPaymentDebtor(debtor);
    setDebtPaymentQuery(debtor.name);
    setShowDebtPaymentDropdown(false);
    setDebtPaymentAmount(debtor.total_debt.toString());
    // Directly show the amount entry modal
    setShowDebtPaymentModal(true);
  };


  const handleConfirmDebtPayment = () => {
    if (!selectedDebtPaymentDebtor) return;
    
    const amount = parseFloat(debtPaymentAmount);
    if (isNaN(amount) || amount <= 0) {
      toast.error('Введіть коректну суму оплати');
      return;
    }
    if (amount > selectedDebtPaymentDebtor.total_debt) {
      toast.error('Сума оплати не може перевищувати борг');
      return;
    }

    // Add the debt payment item to the cart
    setCart((prev) => {
      // Check if debt item already exists
      const existing = prev.find(item => item.product_id === DEBT_PRODUCT_ID);
      if (existing) {
        toast.error('Платіж по боргу вже додано до кошику');
        return prev;
      }
      
      return [...prev, {
        product_id: DEBT_PRODUCT_ID,
        product_title: `Оплата боргу: ${selectedDebtPaymentDebtor.name}`,
        product_barcode: 'DEBT-PAYMENT',
        image_url: null,
        quantity: 1,
        price: amount,
        tax_rate: 0,
        is_weight: false,
        stock: Infinity, // no stock limit
        unit: 'шт',
      }];
    });

    // Store debt payment info for later submission
    sessionStorage.setItem('pos_debt_payment', JSON.stringify({
      debtor_id: selectedDebtPaymentDebtor.id,
      amount: amount,
      debtor_name: selectedDebtPaymentDebtor.name,
    }));

    // Clear the unified search field
    resetSearch();

    setShowDebtPaymentModal(false);
    setShowDebtPaymentSearch(false);
    setSelectedDebtPaymentDebtor(null);
    setDebtPaymentQuery('');
    setDebtPaymentAmount('');
    
    toast.success(`Борг ${amount.toFixed(2)} грн додано до чеку`);
  };

  // Отримати фіскальні реквізити чеку (статус, номер, QR URL) з v2 API
  const fetchFiscalInfo = useCallback(async (receiptId: string): Promise<Receipt | null> => {
    try {
      const fiscal = await prroService.getReceiptFiscalInfo(receiptId);
      setFiscalStatus({
        receipt_id: fiscal.id,
        fiscal_status: fiscal.fiscal_status,
        fiscal_number: fiscal.fiscal_number,
        fiscal_error: fiscal.fiscal_error,
        fiscal_check_url: fiscal.fiscal_check_url,
      });
      return {
        id: fiscal.id,
        receipt_number: '',
        receipt_type: 'sale',
        items: [],
        total_amount: '0',
        vat_amount: '0',
        payment_method: null,
        payment_status: 'paid',
        cash_amount: '0',
        card_amount: '0',
        change_amount: '0',
        cashier_id: '',
        created_by: '',
        created_at: '',
        is_fiscal: fiscal.is_fiscal,
        fiscal_status: fiscal.fiscal_status,
        fiscal_number: fiscal.fiscal_number,
        fiscal_serial: fiscal.fiscal_serial,
        fiscal_sent_at: fiscal.fiscal_sent_at,
        fiscal_error: fiscal.fiscal_error,
        fiscal_check_url: fiscal.fiscal_check_url,
      } as Receipt;
    } catch {
      return null;
    }
  }, []);

  const handlePayment = async () => {
    if (cart.length === 0) {
      toast.error('Кошик порожній');
      return;
    }

    // Фінальна перевірка залишків перед оплатою
    for (const item of cart) {
      if (item.product_id === DEBT_PRODUCT_ID) continue; // Skip debt item stock check
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
      paidAmount = subtotal;
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
      // Extract debt payment info from sessionStorage
      const debtPaymentInfo = sessionStorage.getItem('pos_debt_payment');
      let debtPayment: { debtor_id: string; amount: number } | undefined;
      const hasDebtItem = cart.some(item => item.product_id === DEBT_PRODUCT_ID);
      if (hasDebtItem && debtPaymentInfo) {
        try {
          debtPayment = JSON.parse(debtPaymentInfo);
        } catch {}
        sessionStorage.removeItem('pos_debt_payment');
      }

      const receiptData: ReceiptCreate = {
        receipt_type: returnMode ? 'return' as const : 'sale' as const,
        total_amount: subtotal.toFixed(2),
        paid_amount: paidAmount.toFixed(2),
        debtor_id: selectedDebtor?.id || debtPayment?.debtor_id || undefined,
        items: cart.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
        debt_payment: debtPayment ? {
          debtor_id: debtPayment.debtor_id,
          amount: debtPayment.amount.toFixed(2),
        } : undefined,
      };

      const response = await receiptService.createReceipt(receiptData);

      // Отримуємо фіскальні реквізити з v2 (статус фіскалізації, QR)
      void fetchFiscalInfo(response.id).then((fiscal) => {
        setLastReceipt(fiscal || response);
      });

      // Очищуємо стан перед показом діалогу друку
      setCart([]);
      sessionStorage.removeItem('pos_cart');
      sessionStorage.removeItem('pos_debt_payment');
      localStorage.removeItem('pos_held_receipt');
      setHeldReceipt(null);
      setReturnMode(false);
      setShowPayment(false);
      setShowDebtorField(false);
      setCashAmount('');
      setCardAmount('');
      setPaymentMethod('cash');
      setSelectedDebtor(null);
      setDebtorQuery('');

      // Показуємо діалог друку (або друкуємо автоматично)
      setLastReceipt(response);
      setShowPrintDialog(true);
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
      // Extract debt payment info from sessionStorage
      const debtPaymentInfo = sessionStorage.getItem('pos_debt_payment');
      let debtPayment: { debtor_id: string; amount: number } | undefined;
      const hasDebtItem = cart.some(item => item.product_id === DEBT_PRODUCT_ID);
      if (hasDebtItem && debtPaymentInfo) {
        try {
          debtPayment = JSON.parse(debtPaymentInfo);
        } catch {}
        sessionStorage.removeItem('pos_debt_payment');
      }

      const receiptData: ReceiptCreate = {
        receipt_type: returnMode ? 'return' as const : 'sale' as const,
        total_amount: subtotal.toFixed(2),
        paid_amount: paidAmount.toFixed(2),
        debtor_id: debtor.id,
        items: cart.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
        debt_payment: debtPayment ? {
          debtor_id: debtPayment.debtor_id,
          amount: debtPayment.amount.toFixed(2),
        } : undefined,
      };

      const response = await receiptService.createReceipt(receiptData);

      // Отримуємо фіскальні реквізити з v2 (статус фіскалізації, QR)
      void fetchFiscalInfo(response.id).then((fiscal) => {
        setLastReceipt(fiscal || response);
      });

      // Очищуємо стан перед показом діалогу друку
      setCart([]);
      sessionStorage.removeItem('pos_cart');
      sessionStorage.removeItem('pos_debt_payment');
      localStorage.removeItem('pos_held_receipt');
      setHeldReceipt(null);
      setReturnMode(false);
      setShowPayment(false);
      setShowDebtorModal(false);
      setCashAmount('');
      setCardAmount('');
      setPaymentMethod('cash');

      setDebtorModalDebtor(null);
      setDebtorModalQuery('');
      setSelectedDebtor(null);
      setDebtorQuery('');

      // Показуємо діалог друку (або друкуємо автоматично)
      setLastReceipt(response);
      setShowPrintDialog(true);
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

  // Пряме проведення повернення (без додавання до кошика)
  const handleProcessReturn = useCallback(async (items: ReturnCartItem[]) => {
    if (items.length === 0) {
      toast.error('Виберіть хоча б один товар для повернення');
      return;
    }

    setIsProcessing(true);
    try {
      const totalAmount = items.reduce((sum, item) => sum + item.quantity * item.price, 0);
      // В оригінальному чеку передаємо ID першого товару з original_receipt_id
      const originalReceiptId = items.find(i => i.original_receipt_id)?.original_receipt_id;

      const receiptData: ReceiptCreate = {
        receipt_type: 'return' as const,
        total_amount: totalAmount.toFixed(2),
        paid_amount: totalAmount.toFixed(2),  // каса видає кошти
        original_receipt_id: originalReceiptId || undefined,
        items: items.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
      };

      const response = await receiptService.createReceipt(receiptData);

      toast.success('Повернення оформлено');

      // Отримуємо фіскальні реквізити з v2 (статус фіскалізації, QR)
      void fetchFiscalInfo(response.id).then((fiscal) => {
        setLastReceipt(fiscal || response);
      });

      // Показуємо діалог друку
      setLastReceipt(response);
      setShowPrintDialog(true);

      // Закриваємо модалки
      setShowSelectItemsModal(false);
      setShowReturnWithoutReceipt(false);
      setSelectedSourceReceipt(null);
    } catch (error: any) {
      const errMsg = error?.response?.data?.detail;
      if (typeof errMsg === 'string') {
        toast.error(errMsg);
      } else if (Array.isArray(errMsg)) {
        toast.error(errMsg.map((e: any) => e.msg || JSON.stringify(e)).join(', '));
      } else if (errMsg && typeof errMsg === 'object') {
        toast.error(JSON.stringify(errMsg));
      } else {
        toast.error('Помилка створення чеку повернення');
      }
    } finally {
      setIsProcessing(false);
    }
  }, []);

  // Додавання товарів повернення з модалок до кошика
  const handleAddReturnItemsToCart = useCallback((items: ReturnCartItem[]) => {
    setCart(prev => {
      const existing = [...prev];
      for (const newItem of items) {
        // Перевіряємо чи товар вже є в кошику (за product_id та original_receipt_id)
        const exists = existing.findIndex(
          i => i.product_id === newItem.product_id
            && (i as any).original_receipt_id === newItem.original_receipt_id
        );
        if (exists >= 0) {
          existing[exists] = { ...existing[exists], quantity: existing[exists].quantity + newItem.quantity };
        } else {
          existing.push({
            product_id: newItem.product_id,
            product_title: newItem.product_title,
            product_barcode: newItem.product_barcode,
            image_url: newItem.image_url,
            quantity: newItem.quantity,
            price: newItem.price,
            tax_rate: newItem.tax_rate || 20,
            is_weight: false,
            stock: 999999, // при поверненні не обмежуємо залишком
            unit: newItem.unit || 'шт',
            original_receipt_id: newItem.original_receipt_id,
          });
        }
      }
      return existing;
    });
    toast.success(`${items.reduce((s, i) => s + i.quantity, 0)} товарів додано до повернення`);
    setShowSelectItemsModal(false);
    setShowReturnWithoutReceipt(false);
  }, []);


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

  // Enter для кнопки "Сплатити" в модалці оплати
  const handlePaymentKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      e.stopPropagation();

      // Не виконувати, якщо йде обробка
      if (isProcessing) return;
      // Не виконувати, якщо кошик порожній
      if (cart.length === 0) return;

      handlePayment();
    }
  }, [isProcessing, cart.length, handlePayment]);

  // Ручна фіскалізація чеку
  const handleManualFiscalize = useCallback(async () => {
    if (!fiscalStatus) return;
    const result = await usePrroStore.getState().fiscalize(fiscalStatus.receipt_id);
    if (result) {
      setFiscalStatus({
        receipt_id: result.receipt_id,
        fiscal_status: result.fiscal_status,
        fiscal_number: result.fiscal_number,
        fiscal_error: result.error,
        fiscal_check_url: result.fiscal_check_url,
      });
    }
  }, [fiscalStatus]);

  return (
    <>
      {/* Category browser - horizontal bar above search and cart */}
      <CategoryBrowser onProductSelect={handleProductSelect} />

      {/* ─── Індикатор статусу ПРРО в шапці POS ─────────────────────── */}
      <div className="flex items-center justify-between px-4 py-2 bg-white dark:bg-slate-800 border border-gray-200 dark:border-slate-700 rounded-xl -mt-2 mb-1">
        <div className="flex items-center gap-3">
          <span className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
            ПРРО
          </span>
          {prroStatus ? (
            <div className="flex items-center gap-2">
              {prroStatus.online ? (
                <Badge variant="success">
                  <Wifi className="w-3 h-3 mr-1" /> Онлайн
                </Badge>
              ) : (
                <Badge variant="danger">
                  <WifiOff className="w-3 h-3 mr-1" /> Офлайн
                </Badge>
              )}
              {prroStatus.open_shift ? (
                <Badge variant="primary">
                  <PlayCircle className="w-3 h-3 mr-1" /> Зміна відкрита
                </Badge>
              ) : (
                <Badge variant="warning">
                  <StopCircle className="w-3 h-3 mr-1" /> Зміна закрита
                </Badge>
              )}
              {prroStatus.fn && (
                <span className="text-xs text-gray-400 hidden md:inline">ФН: {prroStatus.fn}</span>
              )}
            </div>
          ) : (
            <span className="text-xs text-gray-400">Статус недоступний</span>
          )}
        </div>
        <button
          onClick={() => navigate('/prro')}
          className="text-xs font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 hover:underline"
        >
          Відкрити вікно ПРРО →
        </button>
      </div>

      {/* ─── Банер статусу фіскалізації останнього чеку ─────────────── */}
      {fiscalStatus && (
        <div
          className={`
            flex items-center gap-3 px-4 py-2.5 rounded-xl border
            ${fiscalStatus.fiscal_status === 'sent'
              ? 'bg-success-50 dark:bg-success-900/20 border-success-200 dark:border-success-700'
              : fiscalStatus.fiscal_status === 'failed'
                ? 'bg-danger-50 dark:bg-danger-900/20 border-danger-200 dark:border-danger-700'
                : 'bg-warning-50 dark:bg-warning-900/20 border-warning-200 dark:border-warning-700'
            }
          `}
        >
          <FileCheck2
            className={`
              w-5 h-5 flex-shrink-0
              ${fiscalStatus.fiscal_status === 'sent'
                ? 'text-success-600'
                : fiscalStatus.fiscal_status === 'failed'
                  ? 'text-danger-600'
                  : 'text-warning-600'
              }
            `}
          />
          <div className="flex-1 min-w-0">
            <p className={`text-sm font-medium ${fiscalStatus.fiscal_status === 'sent' ? 'text-success-700 dark:text-success-400' : fiscalStatus.fiscal_status === 'failed' ? 'text-danger-700 dark:text-danger-400' : 'text-warning-700 dark:text-warning-400'}`}>
              {fiscalStatus.fiscal_status === 'sent' && (
                <>Чек фіскалізовано №{fiscalStatus.fiscal_number || ''}</>
              )}
              {fiscalStatus.fiscal_status === 'pending' && (
                <>Чек очікує фіскалізації (офлайн-черга)</>
              )}
              {fiscalStatus.fiscal_status === 'failed' && (
                <>Помилка фіскалізації: {fiscalStatus.fiscal_error || 'невідома'}</>
              )}
              {fiscalStatus.fiscal_status === 'none' && (
                <>Чек не фіскалізовано</>
              )}
            </p>
          </div>
          {fiscalStatus.fiscal_status !== 'sent' && (
            <Button
              variant="secondary"
              size="sm"
              onClick={handleManualFiscalize}
              isLoading={prroFiscalizing}
            >
              <FileCheck2 className="w-4 h-4" />
              Фіскалізувати
            </Button>
          )}
          <button
            onClick={() => setFiscalStatus(null)}
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            title="Закрити"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      )}

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
          {/* Debtor results */}
          {debtorSearchResults.length > 0 && (
            <div className="mt-4">
              <div className="flex items-center gap-2 px-1 mb-2">
                <Users className="w-4 h-4 text-gray-400" />
                <span className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Боржники
                </span>
              </div>
              <div className="space-y-1">
                {debtorSearchResults.map((debtor) => (
                  <button
                    key={debtor.id}
                    onClick={() => handleUnifiedDebtorSelect(debtor)}
                    className="w-full card p-3 text-left transition-all hover:border-danger-300 dark:hover:border-danger-600 group"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <User className="w-4 h-4 text-danger-400" />
                        <span className="font-medium text-sm text-gray-900 dark:text-gray-100 group-hover:text-danger-600">
                          {debtor.name}
                        </span>
                      </div>
                      <span className="text-sm font-semibold text-danger-600">
                        {formatCurrency(debtor.total_debt)}
                      </span>
                    </div>
                    {debtor.phone && (
                      <p className="text-xs text-gray-400 mt-0.5 ml-6">{debtor.phone}</p>
                    )}
                  </button>
                ))}
              </div>
            </div>
          )}

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
        {returnMode && (
          <div className="bg-danger-50 dark:bg-danger-900/20 border-b border-danger-200 dark:border-danger-700 px-5 py-3">
            <div className="flex items-center gap-2">
              <RotateCcw className="w-5 h-5 text-danger-600" />
              <span className="text-sm font-medium text-danger-700 dark:text-danger-400">
                РЕЖИМ ПОВЕРНЕННЯ
              </span>
              <button
                onClick={() => setReturnMode(false)}
                className="ml-auto text-danger-500 hover:text-danger-700"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            {/* Кнопки вибору способу повернення */}
            <div className="flex gap-2 mt-2">
              <Button
                variant="danger"
                size="sm"
                onClick={() => {
                  setSearchReceiptId(prev => prev + 1);
                  setShowSearchReceiptModal(true);
                }}
              >
                🔍 Знайти чек
              </Button>
              <Button
                variant="danger"
                size="sm"
                onClick={() => setShowReturnWithoutReceipt(true)}
              >
                📦 Без чеку
              </Button>
            </div>
          </div>
        )}

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
                const isDebtItem = item.product_id === DEBT_PRODUCT_ID;

                return (
                  <div
                    key={item.product_id}
                    className={`
                      p-4
                      ${isDebtItem
                        ? 'bg-success-50 dark:bg-success-900/10'
                        : isOverStock
                          ? 'bg-danger-50 dark:bg-danger-900/10'
                          : 'hover:bg-gray-50 dark:hover:bg-slate-700/30'
                      }
                    `}
                  >
                    <div className="flex items-start gap-3">
                      {item.image_url && (
                        <div className="flex-shrink-0 w-14 h-14 rounded-lg overflow-hidden bg-gray-100 dark:bg-slate-700 border border-gray-200 dark:border-slate-600">
                          <img
                            src={item.image_url}
                            alt={item.product_title}
                            className="w-full h-full object-cover"
                            loading="lazy"
                            onError={(e) => {
                              (e.target as HTMLImageElement).style.display = 'none';
                            }}
                          />
                        </div>
                      )}
                      <div className="flex-1 min-w-0">
                        <p className="text-lg font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {item.product_title}
                          {isDebtItem && (
                            <span className="text-xs text-success-600 font-medium ml-1">(оплата боргу)</span>
                          )}
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
                          value={editingQuantity[item.product_id] !== undefined ? editingQuantity[item.product_id] : item.quantity}
                          onFocus={(e) => {
                            setEditingQuantity((prev) => ({
                              ...prev,
                              [item.product_id]: String(item.quantity),
                            }));
                            e.target.select();
                          }}
                          onChange={(e) => {
                            const val = e.target.value;
                            // Дозволяємо порожнє поле або число
                            if (val === '' || /^\d*\.?\d*$/.test(val)) {
                              setEditingQuantity((prev) => ({
                                ...prev,
                                [item.product_id]: val,
                              }));
                            }
                          }}
                          onBlur={(e) => {
                            const val = editingQuantity[item.product_id];
                            if (val === undefined || val === '') {
                              // Якщо поле порожнє — залишаємо поточну кількість
                              setEditingQuantity((prev) => {
                                const next = { ...prev };
                                delete next[item.product_id];
                                return next;
                              });
                              return;
                            }
                            const parsed = parseFloat(val);
                            if (!isNaN(parsed) && parsed > 0) {
                              setItemQuantity(item.product_id, parsed);
                            }
                            setEditingQuantity((prev) => {
                              const next = { ...prev };
                              delete next[item.product_id];
                              return next;
                            });
                          }}
                          className="w-24 h-12 text-center input-field !w-24 text-base font-semibold no-spinner"
                          min="0"
                          max={item.stock}
                          step={item.is_weight ? '0.001' : '1'}
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
                          {formatCurrency(item.is_weight ? Math.ceil(item.quantity * item.price) : item.quantity * item.price)}
                        </span>
                      </div>
                    </div>
                    <div className="flex justify-between mt-1">
                      <span className="text-xs text-gray-400">
                        {isDebtItem
                          ? 'Оплата боргу через касу'
                          : `Залишок: ${item.stock} ${formatUnit(item.unit)}`
                        }
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

        {/* Cart summary — тільки сума і кнопка оплати (при наявності товарів) */}
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
            </div>
          </div>
        )}

        {/* Нижня панель керування — завжди видна (Повернення, Борг, Відкласти) */}
        <div className="border-t border-gray-200 dark:border-slate-700 p-4">
          <div className="flex gap-2">
            <Button
              variant={returnMode ? 'danger' : 'secondary'}
              size="lg"
              onClick={() => setReturnMode(!returnMode)}
              title={returnMode ? 'Вийти з режиму повернення' : 'Режим повернення'}
            >
              <RotateCcw className="w-5 h-5" />
              {returnMode ? 'ПОВЕРНЕННЯ (ON)' : 'ПОВЕРНЕННЯ'}
            </Button>
            <Button
              variant="secondary"
              size="lg"
              onClick={() => setShowDebtPaymentSearch(true)}
              title="Оплата боргу"
            >
              <BadgePercent className="w-5 h-5" />
              БОРГ
            </Button>
            {!returnMode && (
              <Button
                variant="secondary"
                size="lg"
                onClick={handleHoldReceipt}
                disabled={cart.length === 0}
                title="Відкласти поточний чек"
              >
                <Clock className="w-5 h-5" />
                ВІДКЛАСТИ
              </Button>
            )}
            {heldReceipt && !returnMode && (
              <Button
                variant="warning"
                size="lg"
                onClick={handleRestoreHeldReceipt}
                title="Відновити відкладений чек"
              >
                <Clock className="w-5 h-5" />
                ВІДНОВИТИ
              </Button>
            )}
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
      </div>

        {/* Відновлення відкладеного чеку при порожньому кошику */}
        {cart.length === 0 && heldReceipt && (
          <div className="border-t border-gray-200 dark:border-slate-700 p-4">
            <div className="flex flex-col items-center gap-3">
              <p className="text-sm text-gray-500 dark:text-gray-400">
                Є відкладений чек
              </p>
              <Button
                variant="warning"
                size="lg"
                onClick={handleRestoreHeldReceipt}
                className="w-full"
              >
                <Clock className="w-5 h-5" />
                ВІДНОВИТИ ВІДКЛАДЕНИЙ ЧЕК
              </Button>
            </div>
          </div>
        )}



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
        <div className="space-y-6" onKeyDown={handlePaymentKeyDown}>
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
            <>
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

              {/* ⚡ Швидкі кнопки сум */}
              <div className="flex flex-wrap gap-2 mt-2">
                {[50, 100, 200, 500, 1000].map((amount) => {
                  const isActive = parseFloat(cashAmount || '0') >= amount;
                  return (
                    <button
                      key={amount}
                      onClick={() => setCashAmount(amount.toString())}
                      className={`
                        px-4 py-2.5 rounded-lg text-sm font-bold transition-all border-2
                        ${isActive
                          ? 'bg-primary-50 border-primary-400 text-primary-700 dark:bg-primary-900/30 dark:border-primary-600 dark:text-primary-400'
                          : 'bg-white border-gray-200 text-gray-700 hover:border-primary-300 hover:text-primary-600 dark:bg-slate-700 dark:border-slate-600 dark:text-gray-300 dark:hover:border-primary-500'
                        }
                      `}
                    >
                      {amount}₴
                    </button>
                  );
                })}
              </div>
            </>
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

      {/* Modal: Search debtor for debt payment */}
      <Modal
        isOpen={showDebtPaymentSearch}
        onClose={() => {
          setShowDebtPaymentSearch(false);
          setSelectedDebtPaymentDebtor(null);
          setDebtPaymentQuery('');
        }}
        title="Оплата боргу"
        size="md"
      >
        <div className="space-y-4">
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Пошук боржника для оплати боргу через касу.
          </p>
          
          <div className="relative">
            <input
              type="text"
              value={debtPaymentQuery}
              onChange={(e) => {
                setDebtPaymentQuery(e.target.value);
                setSelectedDebtPaymentDebtor(null);
              }}
              placeholder="Введіть ім'я боржника..."
              className="input-field pl-10 pr-10"
              autoFocus
              id="debt-payment-search"
              name="debt-payment-search"
              autoComplete="off"
            />
            <Users className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
            {isSearchingDebtPayment && (
              <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-primary-500 animate-spin" />
            )}
          </div>

          {/* Dropdown */}
          {showDebtPaymentDropdown && debtPaymentResults.length > 0 && (
            <div className="max-h-48 overflow-y-auto bg-white dark:bg-slate-700 border border-gray-200 dark:border-slate-600 rounded-lg shadow-lg">
              {debtPaymentResults.map((debtor) => (
                <button
                  key={debtor.id}
                  onClick={() => handleDebtPaymentSelect(debtor)}
                  className="w-full px-4 py-3 text-left hover:bg-gray-50 dark:hover:bg-slate-600 transition-colors flex items-center justify-between"
                >
                  <span className="font-medium text-gray-900 dark:text-gray-100">{debtor.name}</span>
                  <span className="text-sm font-semibold text-danger-600">
                    Борг: {formatCurrency(debtor.total_debt)}
                  </span>
                </button>
              ))}
            </div>
          )}

          {selectedDebtPaymentDebtor && (
            <div className="px-3 py-3 bg-primary-50 dark:bg-primary-900/20 rounded-lg">
              <p className="text-sm font-medium text-primary-700 dark:text-primary-400">
                {selectedDebtPaymentDebtor.name}
              </p>
              <p className="text-2xl font-bold text-danger-600 mt-1">
                Борг: {formatCurrency(selectedDebtPaymentDebtor.total_debt)}
              </p>
            </div>
          )}

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => {
              setShowDebtPaymentSearch(false);
              setSelectedDebtPaymentDebtor(null);
              setDebtPaymentQuery('');
            }}>
              Скасувати
            </Button>
            <Button
              onClick={() => {
                if (selectedDebtPaymentDebtor) {
                  setShowDebtPaymentSearch(false);
                  setShowDebtPaymentModal(true);
                } else if (debtPaymentQuery.trim()) {
                  // Create new debtor and proceed
                  debtorService.create({ name: debtPaymentQuery.trim() }).then((newDebtor) => {
                    setSelectedDebtPaymentDebtor(newDebtor);
                    setShowDebtPaymentSearch(false);
                    setShowDebtPaymentModal(true);
                  }).catch(() => toast.error('Помилка створення боржника'));
                } else {
                  toast.error('Введіть ім\'я боржника або оберіть існуючого');
                }
              }}
              disabled={!debtPaymentQuery.trim() && !selectedDebtPaymentDebtor}
            >
              {selectedDebtPaymentDebtor ? `Обрати: ${selectedDebtPaymentDebtor.name}` : 'Далі'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Modal: Enter debt payment amount */}
      <Modal
        isOpen={showDebtPaymentModal}
        onClose={() => {
          setShowDebtPaymentModal(false);
          setShowDebtPaymentSearch(true);
        }}
        title="Введіть суму оплати боргу"
        size="sm"
      >
        <div className="space-y-4">
          {selectedDebtPaymentDebtor && (
            <div>
              <p className="text-sm font-medium text-gray-700 dark:text-gray-300">
                {selectedDebtPaymentDebtor.name}
              </p>
              <p className="text-xs text-danger-500 mt-1">
                Поточний борг: {formatCurrency(selectedDebtPaymentDebtor.total_debt)}
              </p>
            </div>
          )}

          <Input
            label="Сума оплати"
            type="number"
            step="0.01"
            min="0.01"
            max={selectedDebtPaymentDebtor?.total_debt || 0}
            value={debtPaymentAmount}
            onChange={(e) => setDebtPaymentAmount(e.target.value)}
            placeholder="Введіть суму"
            icon={<DollarSign className="w-4 h-4" />}
            id="debt-payment-amount"
            name="debt-payment-amount"
            autoFocus
          />

          <p className="text-xs text-gray-400">
            За замовчуванням — повна сума боргу. Ви можете ввести меншу суму для часткової оплати.
          </p>

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => {
              setShowDebtPaymentModal(false);
              setShowDebtPaymentSearch(true);
            }}>
              Назад
            </Button>
            <Button onClick={handleConfirmDebtPayment}>
              Сплатити {debtPaymentAmount ? formatCurrency(parseFloat(debtPaymentAmount)) : ''}
            </Button>
          </div>
        </div>
      </Modal>

    </div>

      {/* ── Модалки повернення ────────────────────── */}

      {/* Пошук оригінального чеку */}
      <SearchReceiptModal
        isOpen={showSearchReceiptModal}
        onClose={() => setShowSearchReceiptModal(false)}
        searchId={searchReceiptId}
        onReceiptSelect={(receipt) => {
          setSelectedSourceReceipt(receipt);
          setShowSearchReceiptModal(false);
          setShowSelectItemsModal(true);
        }}
      />

      {/* Вибір товарів з обраного чеку */}
      {selectedSourceReceipt && (
        <SelectItemsFromReceipt
          isOpen={showSelectItemsModal}
          onClose={() => {
            setShowSelectItemsModal(false);
            // Повертаємо на крок назад — до пошуку чеку
            setShowSearchReceiptModal(true);
          }}
          receipt={selectedSourceReceipt}
          onProcessReturn={handleProcessReturn}
        />
      )}

      {/* Повернення без чеку (за штрих-кодом) */}
      <ReturnWithoutReceipt
        isOpen={showReturnWithoutReceipt}
        onClose={() => setShowReturnWithoutReceipt(false)}
        onProcessReturn={handleProcessReturn}
      />

      {/* Product Card Modal */}
      <ProductCardModal
        isOpen={showProductCard}
        onClose={() => {
          setShowProductCard(false);
          setProductCardProduct(null);
        }}
        product={productCardProduct}
        onAdd={handleAddFromCard}
      />

      {/* ── Діалог друку чеку ───────────────────── */}

      {showPrintDialog && lastReceipt && (
        <PrintReceiptDialog
          isOpen={showPrintDialog}
          onClose={() => {
            setShowPrintDialog(false);
            setLastReceipt(null);
          }}
          receipt={lastReceipt}
          // Для повернень — завжди питати, чи друкувати (autoPrint = false)
          autoPrint={lastReceipt.receipt_type === 'return' ? false : autoPrintReceipt}
          onPrinted={() => {
            setShowPrintDialog(false);
            setLastReceipt(null);
          }}
        />
      )}
    </>
  );
};

export default PosPage;
