import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import {Trash2, Search, ArrowLeft, Save, CheckCircle} from 'lucide-react';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Select, SelectOption } from '@/components/ui/Select';
import { DecimalInput } from '@/components/ui/DecimalInput';
import { Input } from '@/components/ui/Input';
import { isTauri } from '@/hooks/useTauri';
import { saveTransferOffline, syncNow } from '@/services/tauri/offline';
import { useStoreStore } from '@/store/storeStore';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';
interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  cost_price: number;
  price: number;
}

const TransferFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const stores = useStoreStore((s) => s.stores);
  const activeStoreId = useStoreStore((s) => s.activeStoreId);

  // ЕТАП 6: переміщення — між ТОЧКАМИ (store_id), не текстові локації.
  const [fromStoreId, setFromStoreId] = useState<string>(activeStoreId ?? '');
  const [toStoreId, setToStoreId] = useState<string>('');
  const [saving, setSaving] = useState(false);
  const [notes, setNotes] = useState('');
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  const { data: searchData } = useSearchProducts(searchQuery);

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
          cost_price: parseFloat(product.cost_price) || 0,
          price: parseFloat(product.price) || 0,
        },
      ]);
    }
    setSearchQuery('');
    setShowSearch(false);
  };

  const updateQuantity = (productId: string, quantity: number) => {
    if (quantity <= 0) {
      setCart((prev) => prev.filter((item) => item.product_id !== productId));
    } else {
      setCart((prev) =>
        prev.map((item) =>
          item.product_id === productId ? { ...item, quantity } : item
        )
      );
    }
  };

  const updateCostPrice = (productId: string, costPrice: number) => {
    setCart((prev) =>
      prev.map((item) =>
        item.product_id === productId ? { ...item, cost_price: costPrice } : item
      )
    );
  };

  const updatePrice = (productId: string, price: number) => {
    setCart((prev) =>
      prev.map((item) =>
        item.product_id === productId ? { ...item, price } : item
      )
    );
  };

  const removeFromCart = (productId: string) => {
    setCart((prev) => prev.filter((item) => item.product_id !== productId));
  };

  const totalCost = cart.reduce((sum, item) => sum + item.quantity * item.cost_price, 0);
  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);

  const handleSave = async (_andConfirm: boolean = false) => {
    if (!fromStoreId) {
      toast.error('Вкажіть точку-відправника');
      return;
    }
    if (!toStoreId) {
      toast.error('Вкажіть точку-отримувача');
      return;
    }
    if (fromStoreId === toStoreId) {
      toast.error('Точки мають відрізнятися');
      return;
    }
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар');
      return;
    }
    const cashStoreId = useStoreStore.getState().activeStoreId;
    if (!cashStoreId) {
      toast.error('Не визначено активну точку каси');
      return;
    }

    const payload = {
      document_type: 'transfer' as const,
      from_store_id: fromStoreId,
      to_store_id: toStoreId,
      notes: notes || undefined,
      items: cart.map(({ ...item }) => ({
        product_id: item.product_id,
        quantity: item.quantity,
        cost_price: item.cost_price,
        price: item.price,
      })),
    };

    // ЕТАП 6 (offline-first): переміщення — ЛОКАЛЬНО (SQLite агрегат 0006 +
    // stock ±qty атомарно). Rust сам визначає сторону каси: from=каса → −qty
    // (відправлення), to=каса → +qty (прийом). Серверний /transfers (v1) не
    // реалізований у documentService — локальний запис єдиний шлях створення.
    setSaving(true);
    try {
      const clientUuid = await saveTransferOffline(payload, cashStoreId);
      void syncNow(); // негайний push (мережа) / pending в outbox (офлайн)
      toast.success(`Переміщення збережено локально (№ ${clientUuid.slice(0, 8)})`);
      navigate('/documents');
    } catch (e) {
      console.warn('save_transfer_offline недоступна:', e);
      toast.error(
        isTauri()
          ? 'Не вдалося зберегти переміщення локально'
          : 'Переміщення доступне лише в десктопному застосунку',
      );
    } finally {
      setSaving(false);
    }
  };

  const storeOptions: SelectOption[] = stores.map((s) => ({
    value: s.id,
    label: s.name,
  }));
  const toStoreOptions: SelectOption[] = stores
    .filter((s) => s.id !== fromStoreId)
    .map((s) => ({ value: s.id, label: s.name }));

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button aria-label="Назад"
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Переміщення товарів
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Переміщення між торговельними точками
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Select
            label="Звідки (точка-відправник) *"
            options={storeOptions}
            value={fromStoreId}
            placeholder="Оберіть точку"
            onChange={(e) => {
              setFromStoreId(e.target.value);
              if (e.target.value === toStoreId) setToStoreId('');
            }}
          />
          <Select
            label="Куди (точка-отримувач) *"
            options={toStoreOptions}
            value={toStoreId}
            placeholder="Оберіть точку"
            onChange={(e) => setToStoreId(e.target.value)}
          />
        </div>

        <div className="relative">
          <Input
            label="Додати товар"
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
                  <p className="text-xs text-gray-400">Залишок: {product.stock}</p>
                </button>
              ))}
            </div>
          )}
        </div>

        {cart.length > 0 && (
          <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
            <table className="w-full">
              <thead>
                <tr className="bg-gray-50 dark:bg-slate-800/50">
                  <th className="table-header">Товар</th>
                  <th className="table-header">Кількість</th>
                  <th className="table-header">Собівартість (з ПДВ)</th>
                  <th className="table-header">Ціна продажу</th>
                  <th className="table-header">Сума собівартості</th>
                  <th className="table-header">Сума продажу</th>
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
                      <DecimalInput
                        value={item.quantity}
                        onCommit={(n) => updateQuantity(item.product_id, n)}
                        className="w-20 input-field text-center px-3"
                      />
                    </td>
                    <td className="table-cell">
                      <DecimalInput
                        value={item.cost_price}
                        onCommit={(n) => updateCostPrice(item.product_id, n)}
                        className="w-24 input-field text-right px-3"
                      />
                    </td>
                    <td className="table-cell">
                      <DecimalInput
                        value={item.price}
                        onCommit={(n) => updatePrice(item.product_id, n)}
                        className="w-24 input-field text-right px-3"
                      />
                    </td>
                    <td className="table-cell font-medium">
                      {formatCurrency(item.quantity * item.cost_price)}
                    </td>
                    <td className="table-cell font-medium">
                      {formatCurrency(item.quantity * item.price)}
                    </td>
                    <td className="table-cell">
                      <button
                        onClick={() => removeFromCart(item.product_id)}
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
                  <td colSpan={2} className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                    {formatCurrency(totalCost)}
                  </td>
                </tr>
                <tr className="bg-gray-100 dark:bg-slate-800 border-t border-gray-300 dark:border-slate-600">
                  <td colSpan={5} className="px-4 py-2 text-right text-gray-500 dark:text-gray-400 text-sm">
                    Сума продажу:
                  </td>
                  <td colSpan={2} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400">
                    {formatCurrency(totalAmount)}
                  </td>
                </tr>
              </tfoot>
            </table>
          </div>
        )}

        <Input
          label="Примітки"
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Додаткова інформація..."
        />

        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={goBack}>
            Скасувати
          </Button>
          <Button
            variant="secondary"
            onClick={() => handleSave(false)}
            icon={<Save className="w-4 h-4" />}
            isLoading={saving}
          >
            Зберегти
          </Button>
          <Button
            onClick={() => handleSave(true)}
            icon={<CheckCircle className="w-4 h-4" />}
            isLoading={saving}
          >
            Зберегти та провести
          </Button>
        </div>
      </div>
    </div>
  );
};

export default TransferFormPage;
