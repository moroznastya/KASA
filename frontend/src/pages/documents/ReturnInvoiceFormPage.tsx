import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle } from 'lucide-react';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
}

const ReturnInvoiceFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { data: suppliersData } = useAllSuppliers();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();

  const [number, setNumber] = useState('');
  const [returnDate, setReturnDate] = useState(new Date().toISOString().split('T')[0]);
  const [isFiscal, setIsFiscal] = useState(false);
  const [supplierId, setSupplierId] = useState<string | null>(null);
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

  const handleSave = async (andConfirm: boolean = false) => {
    if (!number.trim()) {
      toast.error('Введіть номер документа');
      return;
    }
    if (!supplierId) {
      toast.error('Виберіть постачальника');
      return;
    }
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар');
      return;
    }

    try {
      const doc = await createMutation.mutateAsync({
        document_type: 'return_invoice',
        number: number.trim(),
        supplier_id: supplierId,
        return_date: new Date(returnDate).toISOString(),
        is_fiscal: isFiscal,
        notes: notes || undefined,
        items: cart.map(({ product_title, product_barcode, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          total: item.quantity * item.price,
        })),
      });

      if (andConfirm) {
        await confirmMutation.mutateAsync({ id: doc.id, documentType: 'return_invoice' });
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
          onClick={() => navigate('/documents')}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Повернення постачальнику
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Повернення товарів постачальнику
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <Input
            label="Номер документа *"
            value={number}
            onChange={(e) => setNumber(e.target.value)}
            placeholder="Наприклад: ПВ-001"
            autoFocus
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
          onChange={(e) => setSupplierId(e.target.value || null)}
        />

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
          <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
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
                          updateQuantity(item.product_id, parseInt(e.target.value) || 1)
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
                          updatePrice(item.product_id, parseFloat(e.target.value) || 0)
                        }
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
                    Загальна сума:
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

        <Input
          label="Примітки"
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Причина повернення..."
        />

        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => navigate('/documents')}>
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
