import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle, Calendar, Package } from 'lucide-react';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { DecimalInput } from '@/components/ui/DecimalInput';
import { Select } from '@/components/ui/Select';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';
interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
}

const PurchaseOrderFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const { data: suppliersData } = useAllSuppliers();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();

  // Основні поля
  const [orderDate, setOrderDate] = useState(new Date().toISOString().split('T')[0]);
  const [expectedDate, setExpectedDate] = useState('');
  const [isFiscal, setIsFiscal] = useState(false);
  const [supplierId, setSupplierId] = useState<string | null>(null);
  const [notes, setNotes] = useState('');

  // Товари
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  const { data: searchData } = useSearchProducts(searchQuery);

  // ─── Пошук товарів ──────────────────────────────────
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

  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);

  // ─── Збереження ──────────────────────────────────────
  const handleSave = async (andConfirm: boolean = false) => {
    if (!supplierId) {
      toast.error('Виберіть постачальника');
      return;
    }
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар для замовлення');
      return;
    }

    try {
      const payload: any = {
        document_type: 'purchase_order',
        supplier_id: supplierId,
        order_date: new Date(orderDate).toISOString(),
        expected_date: expectedDate ? new Date(expectedDate).toISOString() : undefined,
        is_fiscal: isFiscal,
        notes: notes || undefined,
        items: cart.map(({ product_title, product_barcode, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          total: item.quantity * item.price,
        })),
      };

      const doc = await createMutation.mutateAsync(payload);

      if (andConfirm) {
        await confirmMutation.mutateAsync({ id: doc.id, documentType: 'purchase_order' });
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
        <button aria-label="Назад"
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Замовлення постачальнику
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Створіть замовлення — при підтвердженні автоматично створиться прибуткова накладна
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <Input
            label="Дата замовлення"
            type="date"
            value={orderDate}
            onChange={(e) => setOrderDate(e.target.value)}
          />
          <Input
            label="Очікувана дата поставки"
            type="date"
            value={expectedDate}
            onChange={(e) => setExpectedDate(e.target.value)}
            icon={<Calendar className="w-4 h-4" />}
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

        {/* ─── Товари ─────────────────────────────────── */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-6">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3 flex items-center gap-2">
            <Package className="w-5 h-5 text-primary-500" />
            Товари для замовлення
          </h3>

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
                        <DecimalInput
                          value={item.quantity}
                          onCommit={(n) => updateQuantity(item.product_id, n)}
                          className="w-20 input-field text-center px-3"
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
                    <td colSpan={3} className="px-4 py-3 text-right font-semibold text-gray-700 dark:text-gray-300">
                      Загальна сума замовлення:
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

        <Input
          label="Примітки"
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Коментар до замовлення..."
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

export default PurchaseOrderFormPage;
