import React, { useState, useCallback, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle, Banknote, RefreshCw, BookOpen, Package, FileText } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { DecimalInput } from '@/components/ui/DecimalInput';
import { Select, SelectOption } from '@/components/ui/Select';
import { formatCurrency } from '@/utils/format';
import { ReturnActionType } from '@/types/document';
import { ledgerService } from '@/services/ledgerService';
import api from '@/services/api';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';

/** Заокруглення ціни продажу до гривні (без копійок) */
const roundPrice = (value: number): number => Math.round(value);

/** Розрахувати markup % з retail_price та cost_price */
const calcMarkupPercent = (retailPrice: number, costPrice: number): number => {
  if (costPrice <= 0) return 0;
  return Math.round(((retailPrice - costPrice) / costPrice) * 100);
};

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  /** Ціна продажу = собівартість × (1 + націнка/100) */
  price: number;
  /** Собівартість = ціна з ПДВ з накладної або карточки товару */
  cost_price: number;
  /** Націнка (%) — розраховується з price та cost_price */
  markup_percent: number;
}

/** Мапа типів дій на їхні мітки та іконки */
const RETURN_ACTION_OPTIONS: { value: ReturnActionType; label: string; description: string; icon: React.ReactNode }[] = [
  {
    value: 'deduct_from_debt',
    label: 'Списати з боргу',
    description: 'Зменшити борг перед постачальником',
    icon: <BookOpen className="w-4 h-4" />,
  },
  {
    value: 'add_to_cash',
    label: 'Зачислити в касу',
    description: 'Отримати гроші в касу',
    icon: <Banknote className="w-4 h-4" />,
  },
  {
    value: 'exchange',
    label: 'Обмін на інший товар',
    description: 'Повернути товар та отримати інший',
    icon: <RefreshCw className="w-4 h-4" />,
  },
];

const ReturnInvoiceFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { id: editId } = useParams<{ id: string }>();
  const isEdit = !!editId;
  const { goBack } = useBackNavigation();
  const { data: suppliersData } = useAllSuppliers();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();

  // Основні поля
  const [number, setNumber] = useState('');
  const [returnDate, setReturnDate] = useState(new Date().toISOString().split('T')[0]);
  const [isFiscal, setIsFiscal] = useState(false);
  const [supplierId, setSupplierId] = useState<string | null>(null);
  const [returnAction, setReturnAction] = useState<ReturnActionType>('deduct_from_debt');
  const [notes, setNotes] = useState('');

  // Прив'язка до прибуткової накладної
  const [sourceInvoiceId, setSourceInvoiceId] = useState<string | null>(null);

  // Товари на повернення
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  // Товари для обміну (якщо return_action = exchange)
  const [exchangeCart, setExchangeCart] = useState<CartItem[]>([]);
  const [exchangeSearchQuery, setExchangeSearchQuery] = useState('');
  const [exchangeSearchResults, setExchangeSearchResults] = useState<any[]>([]);
  const [showExchangeSearch, setShowExchangeSearch] = useState(false);

  // Завантаження даних для редагування
  const { data: editData } = useQuery({
    queryKey: ['return-invoice', editId],
    queryFn: async () => {
      if (!editId) return null;
      const response = await api.get(`/return-invoices/${editId}`);
      return response.data;
    },
    enabled: isEdit,
  });

  // Заповнення форми при редагуванні
  useEffect(() => {
    if (!editData) return;
    setNumber(editData.number || '');
    setReturnDate(editData.return_date ? editData.return_date.split('T')[0] : '');
    setIsFiscal(editData.is_fiscal || false);
    setSupplierId(editData.supplier_id || null);
    setReturnAction(editData.return_action || 'deduct_from_debt');
    setNotes(editData.notes || '');
    setSourceInvoiceId(editData.source_invoice_id || null);

    if (editData.items && editData.items.length > 0) {
      const cartItems: CartItem[] = editData.items.map((item: any) => {
        // Актуальна собівартість з карточки товару
        const currentCostPrice = parseFloat(item.product?.cost_price) || 0;
        const savedCostPrice = Number(item.cost_price || item.price || 0);
        const costPrice = currentCostPrice > 0 ? currentCostPrice : savedCostPrice;

        // Актуальна ціна продажу з карточки товару
        const currentRetailPrice = parseFloat(item.product?.price) || 0;
        const savedPrice = Number(item.price || 0);
        const retailPrice = currentRetailPrice > 0 ? currentRetailPrice : savedPrice;

        // Актуальна націнка
        const savedMarkup = parseFloat(item.markup_percent) || 0;
        const markupPercent = retailPrice > 0 && costPrice > 0
          ? calcMarkupPercent(retailPrice, costPrice)
          : savedMarkup;

        // Ціна продажу
        const price = retailPrice > 0
          ? roundPrice(retailPrice)
          : costPrice > 0
            ? roundPrice(costPrice * (1 + markupPercent / 100))
            : roundPrice(savedPrice);

        return {
          product_id: item.product_id,
          product_title: item.product?.title || item.product_name || '',
          product_barcode: item.product?.barcode || null,
          quantity: Number(item.quantity),
          price,
          cost_price: costPrice,
          markup_percent: markupPercent,
        };
      });
      setCart(cartItems);
    }
  }, [editData]);

  const { data: searchData } = useSearchProducts(searchQuery);
  const { data: exchangeSearchData } = useSearchProducts(exchangeSearchQuery);

  // Отримуємо накладні постачальника для опціональної прив'язки
  const { data: supplierInvoices } = useQuery({
    queryKey: ['supplier-invoices', supplierId],
    queryFn: () => ledgerService.getSupplierInvoices(supplierId!),
    enabled: !!supplierId,
  });

  // Опції для вибору прибуткової накладної
  const sourceInvoiceOptions: SelectOption[] = [
    { value: '', label: '— Без прив\'язки до накладної —' },
    ...(supplierInvoices?.map((inv: any) => ({
      value: inv.id,
      label: `${inv.number} — ${formatCurrency(inv.total_amount)}`,
    })) || []),
  ];

  // ─── Пошук товарів для повернення ──────────────────────────────
  const handleSearch = useCallback(
    (query: string) => {
      setSearchQuery(query);
      if (query.length >= 2 && searchData?.items) {
        setSearchResults(searchData.items);
        setShowSearch(true);
      } else {
        setSearchResults([]);
        setShowSearch(false);
      }
    },
    [searchData]
  );

  const addToCart = (product: any) => {
    // Собівартість = ціна з ПДВ (з бази або 0)
    const costPrice = parseFloat(product.cost_price) || 0;
    // Ціна продажу = retail_price з карточки товару
    const retailPrice = parseFloat(product.price) || 0;
    // Націнка (%) — розраховуємо з retail_price та cost_price
    const markupPercent = retailPrice > 0 && costPrice > 0
      ? calcMarkupPercent(retailPrice, costPrice)
      : parseFloat(product.markup) || 0;
    // Ціна продажу
    const price = retailPrice > 0
      ? roundPrice(retailPrice)
      : costPrice > 0
        ? roundPrice(costPrice * (1 + markupPercent / 100))
        : 0;

    const existing = cart.find((item) => item.product_id === product.id);
    if (existing) {
      setCart((prev) =>
        prev.map((item) =>
          item.product_id === product.id
            ? { ...item, quantity: item.quantity + 1 }
            : item
        )
      );
    } else {
      setCart((prev) => [
        ...prev,
        {
          product_id: product.id,
          product_title: product.title,
          product_barcode: product.barcode || null,
          quantity: 1,
          price,
          cost_price: costPrice,
          markup_percent: markupPercent,
        },
      ]);
    }
    setSearchQuery('');
    setShowSearch(false);
  };

  // ─── Пошук товарів для обміну ──────────────────────────────────
  const handleExchangeSearch = useCallback(
    (query: string) => {
      setExchangeSearchQuery(query);
      if (query.length >= 2 && exchangeSearchData?.items) {
        setExchangeSearchResults(exchangeSearchData.items);
        setShowExchangeSearch(true);
      } else {
        setExchangeSearchResults([]);
        setShowExchangeSearch(false);
      }
    },
    [exchangeSearchData]
  );

  const addToExchangeCart = (product: any) => {
    const costPrice = parseFloat(product.cost_price) || 0;
    const retailPrice = parseFloat(product.price) || 0;
    const markupPercent = retailPrice > 0 && costPrice > 0
      ? calcMarkupPercent(retailPrice, costPrice)
      : parseFloat(product.markup) || 0;
    const price = retailPrice > 0
      ? roundPrice(retailPrice)
      : costPrice > 0
        ? roundPrice(costPrice * (1 + markupPercent / 100))
        : 0;

    const existing = exchangeCart.find((item) => item.product_id === product.id);
    if (existing) {
      setExchangeCart((prev) =>
        prev.map((item) =>
          item.product_id === product.id
            ? { ...item, quantity: item.quantity + 1 }
            : item
        )
      );
    } else {
      setExchangeCart((prev) => [
        ...prev,
        {
          product_id: product.id,
          product_title: product.title,
          product_barcode: product.barcode || null,
          quantity: 1,
          price,
          cost_price: costPrice,
          markup_percent: markupPercent,
        },
      ]);
    }
    setExchangeSearchQuery('');
    setShowExchangeSearch(false);
  };

  // ─── Оновлення кількості ───────────────────────────────────────
  const updateQuantity = (
    list: CartItem[],
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string, quantity: number) => {
    if (quantity <= 0) {
      setList((prev) => prev.filter((item) => item.product_id !== productId));
    } else {
      setList((prev) =>
        prev.map((item) =>
          item.product_id === productId ? { ...item, quantity } : item
        )
      );
    }
  };

  // ─── Оновлення собівартості → перерахунок ціни продажу ─────────
  const updateCostPrice = (
    list: CartItem[],
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string, costPrice: number) => {
    setList((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        const newPrice = costPrice > 0
          ? roundPrice(costPrice * (1 + item.markup_percent / 100))
          : 0;
        return { ...item, cost_price: costPrice, price: newPrice };
      })
    );
  };

  // ─── Оновлення націнки → перерахунок ціни продажу ──────────────
  const updateMarkup = (
    list: CartItem[],
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string, markupPercent: number) => {
    setList((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        const newPrice = item.cost_price > 0
          ? roundPrice(item.cost_price * (1 + markupPercent / 100))
          : item.price;
        return { ...item, markup_percent: markupPercent, price: newPrice };
      })
    );
  };

  // ─── Оновлення ціни продажу → перерахунок націнки ──────────────
  const updatePrice = (
    list: CartItem[],
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string, price: number) => {
    setList((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        const markup = item.cost_price > 0
          ? Math.round(((price - item.cost_price) / item.cost_price) * 100)
          : 0;
        return { ...item, price, markup_percent: markup };
      })
    );
  };

  // ─── Видалення товару ──────────────────────────────────────────
  const removeFromCart = (
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string) => {
    setList((prev) => prev.filter((item) => item.product_id !== productId));
  };

  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
  const totalCost = cart.reduce((sum, item) => sum + item.quantity * item.cost_price, 0);
  const exchangeTotalAmount = exchangeCart.reduce((sum, item) => sum + item.quantity * item.price, 0);

  // ─── Збереження ────────────────────────────────────────────────
  const handleSave = async (andConfirm: boolean = false) => {
    if (!supplierId) {
      toast.error('Виберіть постачальника');
      return;
    }
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар для повернення');
      return;
    }
    if (returnAction === 'exchange' && exchangeCart.length === 0) {
      toast.error('Для обміну додайте хоча б один товар, на який відбувається обмін');
      return;
    }

    try {
      // Спільні поля
      const basePayload: any = {
        supplier_id: supplierId,
        return_date: new Date(returnDate).toISOString(),
        return_action: returnAction,
        is_fiscal: isFiscal,
        notes: notes || undefined,
        source_invoice_id: sourceInvoiceId || undefined,
        total_amount: totalAmount,
        items: cart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          cost_price: item.cost_price,
          total: Number((item.quantity * item.price).toFixed(2)),
        })),
      };

      // Якщо обмін — додаємо exchange_items
      if (returnAction === 'exchange' && exchangeCart.length > 0) {
        basePayload.exchange_items = exchangeCart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          total: Number((item.quantity * item.price).toFixed(2)),
        }));
      }

      if (isEdit) {
        // Редагування — PUT
        await api.put(`/return-invoices/${editId}`, basePayload);
        toast.success('Повернення оновлено');
        navigate('/documents');
        return;
      }

      // Створення — POST
      const payload: any = { document_type: 'return_invoice', ...basePayload };
      const doc = await createMutation.mutateAsync(payload);

      if (andConfirm) {
        // Для exchange передаємо exchange_items прямо в API
        const confirmBody: any = { status: 'confirmed' };
        if (returnAction === 'exchange' && exchangeCart.length > 0) {
          confirmBody.exchange_items = exchangeCart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            total: Number((item.quantity * item.price).toFixed(2)),
          }));
        }
        await api.post(`/return-invoices/${doc.id}/confirm`, confirmBody);
      }

      navigate('/documents');
    } catch (e: any) {
      const detail = e?.response?.data?.detail || e?.message || 'Помилка при збереженні';
      toast.error(detail);
    }
  };

  const supplierOptions = [
    { value: '', label: 'Виберіть постачальника' },
    ...(suppliersData?.map((s) => ({
      value: String(s.id),
      label: s.name,
    })) || []),
  ];

  const showColumns = ['product', 'quantity', 'cost_price', 'price', 'markup', 'cost_total', 'total', 'actions'] as const;

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button aria-label="Назад"
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            {isEdit ? 'Редагування' : 'Нове'} повернення постачальнику
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            {isEdit ? 'Редагування повернення' : 'Повернення товарів постачальнику (номер генерується автоматично)'}
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <Input
            label="Номер (якщо не вказано — автоматично)"
            value={number}
            onChange={(e) => setNumber(e.target.value)}
            placeholder="Автоматично"
          />
          <Input
            label="Дата повернення"
            type="date"
            value={returnDate}
            onChange={(e) => setReturnDate(e.target.value)}
          />
          <div className="flex items-end pb-2">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={isFiscal}
                onChange={(e) => setIsFiscal(e.target.checked)}
                className="w-4 h-4 rounded border-gray-300 dark:border-slate-600 text-blue-600 focus:ring-blue-500"
              />
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                Фіскальний документ
              </span>
            </label>
          </div>
        </div>

        <Select
          label="Постачальник *"
          options={supplierOptions}
          value={String(supplierId || '')}
          onChange={(e) => {
            setSupplierId(e.target.value || null);
            setSourceInvoiceId(null);
          }}
        />

        {/* Опціональна прив'язка до прибуткової накладної */}
        {supplierId && supplierInvoices && supplierInvoices.length > 0 && (
          <Select
            label="Прибуткова накладна (опціонально)"
            options={sourceInvoiceOptions}
            value={sourceInvoiceId || ''}
            onChange={(e) => setSourceInvoiceId(e.target.value || null)}
            containerClassName="[&_p]:text-xs [&_p]:text-gray-400 [&_p]:mb-2"
          />
        )}

        {/* Вибір дії при підтвердженні */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Дія при підтвердженні *
          </label>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            {RETURN_ACTION_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => setReturnAction(option.value)}
                className={`flex items-start gap-3 p-4 rounded-xl border-2 text-left transition-all ${
                  returnAction === option.value
                    ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20 shadow-sm'
                    : 'border-gray-200 dark:border-slate-700 hover:border-gray-300 dark:hover:border-slate-600 bg-white dark:bg-slate-800'
                }`}
              >
                <div className={`p-2 rounded-lg ${
                  returnAction === option.value
                    ? 'bg-primary-100 dark:bg-primary-800 text-primary-600'
                    : 'bg-gray-100 dark:bg-slate-700 text-gray-500'
                }`}>
                  {option.icon}
                </div>
                <div>
                  <p className={`font-medium text-sm ${
                    returnAction === option.value
                      ? 'text-primary-700 dark:text-primary-300'
                      : 'text-gray-900 dark:text-gray-100'
                  }`}>
                    {option.label}
                  </p>
                  <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                    {option.description}
                  </p>
                </div>
              </button>
            ))}
          </div>
        </div>

        {/* ─── Секція: Товари на повернення ─────────────────────────── */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-6">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3 flex items-center gap-2">
            <Trash2 className="w-5 h-5 text-danger-500" />
            Товари на повернення
          </h3>

          <div className="relative">
            <Input
              label="Додати товар для повернення"
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              placeholder="Пошук за назвою або штрих-кодом..."
              icon={<Search className="w-4 h-4" />}
            />
            {showSearch && searchResults.length > 0 && (
              <div className="absolute z-10 w-full mt-1 bg-white dark:bg-slate-800 border border-gray-200 dark:border-slate-700 rounded-xl shadow-lg max-h-60 overflow-y-auto">
                {searchResults.map((product) => (
                  <button
                    key={product.id}
                    onClick={() => addToCart(product)}
                    className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-gray-50 dark:hover:bg-slate-700 text-left transition-colors"
                  >
                    <div>
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                        {product.title}
                      </p>
                      {product.barcode && (
                        <p className="text-xs text-gray-400">ШК: {product.barcode}</p>
                      )}
                    </div>
                    <div className="text-right">
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                        {formatCurrency(product.price)}
                      </p>
                      <p className="text-xs text-gray-400">Залишок: {product.stock}</p>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>

          {cart.length > 0 && (
            <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden mt-3">
              <table className="w-full">
                <thead>
                  <tr className="bg-gray-50 dark:bg-slate-800/50">
                    <th className="table-header">Товар</th>
                    <th className="table-header w-24 text-right">Кількість</th>
                    <th className="table-header w-28 text-right">Собівартість (з ПДВ)</th>
                    <th className="table-header w-28 text-right">Ціна продажу</th>
                    <th className="table-header w-28 text-right">Націнка</th>
                    <th className="table-header w-28 text-right">Сума собівартості</th>
                    <th className="table-header w-28 text-right">Сума продажу</th>
                    <th className="table-header w-16"></th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {cart.map((item, index) => (
                    <tr key={`${item.product_id}-${index}`}>
                      <td className="table-cell">
                        <p className="font-medium text-gray-900 dark:text-gray-100">
                          {item.product_title}
                        </p>
                        {item.product_barcode && (
                          <p className="text-xs text-gray-400">ШК: {item.product_barcode}</p>
                        )}
                      </td>
                      <td className="table-cell">
                        <DecimalInput
                          value={item.quantity}
                          onCommit={(n) => updateQuantity(cart, setCart)(item.product_id, n)}
                          className="w-20 input-field text-center px-3 no-spinner"
                        />
                      </td>
                      <td className="table-cell">
                        <DecimalInput
                          value={item.cost_price}
                          onCommit={(n) => updateCostPrice(cart, setCart)(item.product_id, n)}
                          className="w-24 input-field text-right px-3 no-spinner"
                          title="Собівартість = ціна з ПДВ з накладної"
                        />
                      </td>
                      <td className="table-cell">
                        <DecimalInput
                          value={item.price}
                          onCommit={(n) => updatePrice(cart, setCart)(item.product_id, n)}
                          className="w-24 input-field text-right px-3 no-spinner"
                          title="Ціна продажу заокруглена до гривні"
                        />
                      </td>
                      <td className="table-cell">
                        <div className="flex items-center gap-1">
                          <DecimalInput
                            value={item.markup_percent}
                            onCommit={(n) => updateMarkup(cart, setCart)(item.product_id, n)}
                            className="w-20 input-field text-right px-3 no-spinner"
                          />
                          <span className="text-sm text-gray-400">%</span>
                        </div>
                      </td>
                      <td className="table-cell font-medium">
                        {formatCurrency(item.quantity * item.cost_price)}
                      </td>
                      <td className="table-cell font-medium">
                        {formatCurrency(item.quantity * item.price)}
                      </td>
                      <td className="table-cell">
                        <button
                          onClick={() => removeFromCart(setCart)(item.product_id)}
                          className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 transition-colors"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr className="bg-gray-50 dark:bg-slate-800/50">
                    <td colSpan={5} className="px-4 py-3 text-right text-gray-500 dark:text-gray-400 text-sm">
                      Закупівельна сума:
                    </td>
                    <td colSpan={3} className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                      {formatCurrency(totalCost)}
                    </td>
                  </tr>
                  <tr className="bg-gray-100 dark:bg-slate-800 border-t border-gray-300 dark:border-slate-600">
                    <td colSpan={5} className="px-4 py-2 text-right text-gray-500 dark:text-gray-400 text-sm">
                      Сума повернення:
                    </td>
                    <td colSpan={3} className="px-4 py-2 font-bold text-lg text-gray-900 dark:text-gray-100">
                      {formatCurrency(totalAmount)}
                    </td>
                  </tr>
                </tfoot>
              </table>
            </div>
          )}
        </div>

        {/* ─── Секція: Товари для обміну (тільки якщо вибрано exchange) ── */}
        {returnAction === 'exchange' && (
          <div className="border-t border-gray-200 dark:border-slate-700 pt-6">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3 flex items-center gap-2">
              <Package className="w-5 h-5 text-purple-500" />
              Товари для обміну (нові)
              <span className="text-sm font-normal text-gray-500 dark:text-gray-400 ml-1">
                — ці товари будуть оприбутковані замість повернутих
              </span>
            </h3>

            <div className="relative">
              <Input
                label="Додати товар для обміну"
                value={exchangeSearchQuery}
                onChange={(e) => handleExchangeSearch(e.target.value)}
                placeholder="Пошук за назвою або штрих-кодом..."
                icon={<Search className="w-4 h-4" />}
              />
              {showExchangeSearch && exchangeSearchResults.length > 0 && (
                <div className="absolute z-10 w-full mt-1 bg-white dark:bg-slate-800 border border-gray-200 dark:border-slate-700 rounded-xl shadow-lg max-h-60 overflow-y-auto">
                  {exchangeSearchResults.map((product) => (
                    <button
                      key={product.id}
                      onClick={() => addToExchangeCart(product)}
                      className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-gray-50 dark:hover:bg-slate-700 text-left transition-colors"
                    >
                      <div>
                        <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                          {product.title}
                        </p>
                        {product.barcode && (
                          <p className="text-xs text-gray-400">ШК: {product.barcode}</p>
                        )}
                      </div>
                      <div className="text-right">
                        <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                          {formatCurrency(product.price)}
                        </p>
                        <p className="text-xs text-gray-400">Залишок: {product.stock}</p>
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </div>

            {exchangeCart.length > 0 && (
              <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden mt-3">
                <table className="w-full">
                  <thead>
                    <tr className="bg-gray-50 dark:bg-slate-800/50">
                      <th className="table-header">Товар</th>
                      <th className="table-header w-24 text-right">Кількість</th>
                      <th className="table-header w-28 text-right">Собівартість (з ПДВ)</th>
                      <th className="table-header w-28 text-right">Ціна продажу</th>
                      <th className="table-header w-28 text-right">Націнка</th>
                      <th className="table-header w-28 text-right">Сума собівартості</th>
                      <th className="table-header w-28 text-right">Сума продажу</th>
                      <th className="table-header w-16"></th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                    {exchangeCart.map((item, index) => (
                      <tr key={`ex-${item.product_id}-${index}`}>
                        <td className="table-cell">
                          <p className="font-medium text-gray-900 dark:text-gray-100">
                            {item.product_title}
                          </p>
                          {item.product_barcode && (
                            <p className="text-xs text-gray-400">ШК: {item.product_barcode}</p>
                          )}
                        </td>
                        <td className="table-cell">
                          <DecimalInput
                            value={item.quantity}
                            onCommit={(n) => updateQuantity(exchangeCart, setExchangeCart)(item.product_id, n)}
                            className="w-20 input-field text-center px-3 no-spinner"
                          />
                        </td>
                        <td className="table-cell">
                          <DecimalInput
                            value={item.cost_price}
                            onCommit={(n) => updateCostPrice(exchangeCart, setExchangeCart)(item.product_id, n)}
                            className="w-24 input-field text-right px-3 no-spinner"
                          />
                        </td>
                        <td className="table-cell">
                          <DecimalInput
                            value={item.price}
                            onCommit={(n) => updatePrice(exchangeCart, setExchangeCart)(item.product_id, n)}
                            className="w-24 input-field text-right px-3 no-spinner"
                          />
                        </td>
                        <td className="table-cell">
                          <div className="flex items-center gap-1">
                            <DecimalInput
                              value={item.markup_percent}
                              onCommit={(n) => updateMarkup(exchangeCart, setExchangeCart)(item.product_id, n)}
                              className="w-20 input-field text-right px-3 no-spinner"
                            />
                            <span className="text-sm text-gray-400">%</span>
                          </div>
                        </td>
                        <td className="table-cell font-medium">
                          {formatCurrency(item.quantity * item.cost_price)}
                        </td>
                        <td className="table-cell font-medium">
                          {formatCurrency(item.quantity * item.price)}
                        </td>
                        <td className="table-cell">
                          <button
                            onClick={() => removeFromCart(setExchangeCart)(item.product_id)}
                            className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 transition-colors"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                  <tfoot>
                    <tr className="bg-gray-50 dark:bg-slate-800/50">
                      <td colSpan={5} className="px-4 py-3 text-right text-gray-500 dark:text-gray-400 text-sm">
                        Закупівельна сума:
                      </td>
                      <td colSpan={3} className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                        {formatCurrency(exchangeCart.reduce((s, i) => s + i.quantity * i.cost_price, 0))}
                      </td>
                    </tr>
                    <tr className="bg-gray-100 dark:bg-slate-800 border-t border-gray-300 dark:border-slate-600">
                      <td colSpan={5} className="px-4 py-2 text-right text-gray-500 dark:text-gray-400 text-sm">
                        Сума обміну:
                      </td>
                      <td colSpan={3} className="px-4 py-2 font-bold text-lg text-gray-900 dark:text-gray-100">
                        {formatCurrency(exchangeTotalAmount)}
                      </td>
                    </tr>
                  </tfoot>
                </table>
              </div>
            )}
          </div>
        )}

        <Input
          label="Примітки"
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Причина повернення..."
        />

        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={goBack}>
            Скасувати
          </Button>
          <Button
            variant="secondary"
            onClick={() => handleSave(false)}
            icon={<Save className="w-4 h-4" />}
            isLoading={createMutation.isPending}
          >
            {isEdit ? 'Оновити чернетку' : 'Зберегти як чернетку'}
          </Button>
          <Button
            onClick={() => handleSave(true)}
            icon={<CheckCircle className="w-4 h-4" />}
            isLoading={createMutation.isPending || confirmMutation.isPending}
          >
            {isEdit ? 'Оновити та підтвердити' : 'Створити та підтвердити'}
          </Button>
        </div>
      </div>
    </div>
  );
};

export default ReturnInvoiceFormPage;
