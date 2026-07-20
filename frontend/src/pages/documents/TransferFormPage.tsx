import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle } from 'lucide-react';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
}

const TransferFormPage: React.FC = () => {
  const navigate = useNavigate();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();

  const [fromLocation, setFromLocation] = useState('');
  const [toLocation, setToLocation] = useState('');
  const [notes, setNotes] = useState('');
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  const { data: searchData } = useSearchProducts(searchQuery);

  const handleSearch = useCallback(
    (query: string) => {
      setSearchQuery(query);
      if (query.length >= 2 && searchData) {
        setSearchResults(searchData);
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
          price: 0,
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

  const removeFromCart = (productId: string) => {
    setCart((prev) => prev.filter((item) => item.product_id !== productId));
  };

  const handleSave = async (andConfirm: boolean = false) => {
    if (!fromLocation.trim()) {
      toast.error('Вкажіть звідки переміщення');
      return;
    }
    if (!toLocation.trim()) {
      toast.error('Вкажіть куди переміщення');
      return;
    }
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар');
      return;
    }

    try {
      const doc = await createMutation.mutateAsync({
        document_type: 'transfer',
        from_location: fromLocation,
        to_location: toLocation,
        notes: notes || undefined,
        items: cart.map(({ product_title, product_barcode, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
        })),
      });

      if (andConfirm) {
        await confirmMutation.mutateAsync(doc.id);
      }

      navigate('/documents');
    } catch {
      // Error handled
    }
  };

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
            Переміщення товарів
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Переміщення між складами або локаціями
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Input
            label="Звідки *"
            value={fromLocation}
            onChange={(e) => setFromLocation(e.target.value)}
            placeholder="Склад А, Магазин 1..."
          />
          <Input
            label="Куди *"
            value={toLocation}
            onChange={(e) => setToLocation(e.target.value)}
            placeholder="Склад Б, Магазин 2..."
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
                        className="w-20 input-field text-center"
                      />
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

export default TransferFormPage;
