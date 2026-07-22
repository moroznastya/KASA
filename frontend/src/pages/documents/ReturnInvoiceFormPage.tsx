import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle, Banknote, RefreshCw, BookOpen, Package } from 'lucide-react';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { formatCurrency } from '@/utils/format';
import { ReturnActionType } from '@/types/document';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';
interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
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
  const { goBack } = useBackNavigation();
  const { data: suppliersData } = useAllSuppliers();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();

  // Основні поля
  const [returnDate, setReturnDate] = useState(new Date().toISOString().split('T')[0]);
  const [isFiscal, setIsFiscal] = useState(false);
  const [supplierId, setSupplierId] = useState<string | null>(null);
  const [returnAction, setReturnAction] = useState<ReturnActionType>('deduct_from_debt');
  const [notes, setNotes] = useState('');

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

  const { data: searchData } = useSearchProducts(searchQuery);
  const { data: exchangeSearchData } = useSearchProducts(exchangeSearchQuery);

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
          product_barcode: product.barcode,
          quantity: 1,
          price: parseFloat(product.price),
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
          product_barcode: product.barcode,
          quantity: 1,
          price: parseFloat(product.price),
        },
      ]);
    }
    setExchangeSearchQuery('');
    setShowExchangeSearch(false);
  };

  // ─── Спільні функції для кошиків ───────────────────────────────
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

  const updatePrice = (
    list: CartItem[],
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string, price: number) => {
    setList((prev) =>
      prev.map((item) =>
        item.product_id === productId ? { ...item, price } : item
      )
    );
  };

  const removeFromCart = (
    setList: React.Dispatch<React.SetStateAction<CartItem[]>>
  ) => (productId: string) => {
    setList((prev) => prev.filter((item) => item.product_id !== productId));
  };

  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
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
      const payload: any = {
        document_type: 'return_invoice',
        supplier_id: supplierId,
        return_date: new Date(returnDate).toISOString(),
        return_action: returnAction,
        is_fiscal: isFiscal,
        notes: notes || undefined,
        items: cart.map(({ product_title, product_barcode, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          total: item.quantity * item.price,
        })),
      };

      // Якщо обмін — додаємо exchange_items
      if (returnAction === 'exchange' && exchangeCart.length > 0) {
        payload.exchange_items = exchangeCart.map(({ product_title, product_barcode, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          total: item.quantity * item.price,
        }));
      }

      const doc = await createMutation.mutateAsync(payload);

      if (andConfirm) {
        // При підтвердженні з обміном передаємо exchange_items
        const confirmPayload: any = { status: 'confirmed' };
        if (returnAction === 'exchange' && exchangeCart.length > 0) {
          confirmPayload.exchange_items = exchangeCart.map(({ product_title, product_barcode, ...item }) => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            total: item.quantity * item.price,
          }));
        }
        await confirmMutation.mutateAsync({ id: doc.id, documentType: 'return_invoice', ...confirmPayload });
      }

      navigate('/documents');
    } catch {
      // Error handled
    }
  };

  const supplierOptions = [
    { value: '', label: 'Виберіть постачальника' },
    ...(suppliersData?.map((s) => ({
      value: String(s.id),
      label: s.name,
    })) || []),
  ];

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Повернення постачальнику
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Повернення товарів постачальнику (номер генерується автоматично)
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
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
          onChange={(e) => setSupplierId(e.target.value || null)}
        />

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
                    <th className="table-header">Кількість</th>
                    <th className="table-header">Ціна</th>
                    <th className="table-header">Сума</th>
                    <th className="table-header w-16"></th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {cart.map((item) => (
                    <tr key={item.product_id}>
                      <td className="table-cell">
                        <p className="font-medium text-gray-900 dark:text-gray-100">
                          {item.product_title}
                        </p>
                      </td>
                      <td className="table-cell">
                        <input
                          type="number"
                          min="1"
                          value={item.quantity}
                          onChange={(e) =>
                            updateQuantity(cart, setCart)(item.product_id, parseInt(e.target.value) || 1)
                          }
                          className="w-20 input-field text-center px-3"
                        />
                      </td>
                      <td className="table-cell">
                        <input
                          type="number"
                          step="0.01"
                          min="0"
                          value={item.price}
                          onChange={(e) =>
                            updatePrice(cart, setCart)(item.product_id, parseFloat(e.target.value) || 0)
                          }
                          className="w-24 input-field text-right px-3"
                        />
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
                    <td colSpan={3} className="px-4 py-3 text-right font-semibold text-gray-700 dark:text-gray-300">
                      Сума повернення:
                    </td>
                    <td className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                      {formatCurrency(totalAmount)}
                    </td>
                    <td></td>
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
                      <th className="table-header">Кількість</th>
                      <th className="table-header">Ціна</th>
                      <th className="table-header">Сума</th>
                      <th className="table-header w-16"></th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                    {exchangeCart.map((item) => (
                      <tr key={item.product_id}>
                        <td className="table-cell">
                          <p className="font-medium text-gray-900 dark:text-gray-100">
                            {item.product_title}
                          </p>
                        </td>
                        <td className="table-cell">
                          <input
                            type="number"
                            min="1"
                            value={item.quantity}
                            onChange={(e) =>
                              updateQuantity(exchangeCart, setExchangeCart)(item.product_id, parseInt(e.target.value) || 1)
                            }
                            className="w-20 input-field text-center px-3"
                          />
                        </td>
                        <td className="table-cell">
                          <input
                            type="number"
                            step="0.01"
                            min="0"
                            value={item.price}
                            onChange={(e) =>
                              updatePrice(exchangeCart, setExchangeCart)(item.product_id, parseFloat(e.target.value) || 0)
                            }
                            className="w-24 input-field text-right px-3"
                          />
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
                      <td colSpan={3} className="px-4 py-3 text-right font-semibold text-gray-700 dark:text-gray-300">
                        Сума обміну:
                      </td>
                      <td className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                        {formatCurrency(exchangeTotalAmount)}
                      </td>
                      <td></td>
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
            Зберегти як чернетку
          </Button>
          <Button
            onClick={() => handleSave(true)}
            icon={<CheckCircle className="w-4 h-4" />}
            isLoading={createMutation.isPending || confirmMutation.isPending}
          >
            Створити та підтвердити
          </Button>
        </div>
      </div>
    </div>
  );
};

export default ReturnInvoiceFormPage;
