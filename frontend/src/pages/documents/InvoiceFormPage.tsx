import React, { useState, useCallback, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle, Package, ImageUp, Loader2 } from 'lucide-react';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { useSearchProducts, useCreateProduct } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';
import api from '@/services/api';
/** Спосіб оплати з постачальником */
type PaymentMethod = 'credit' | 'bank_transfer' | 'cash' | 'other';

const PAYMENT_METHODS: { value: PaymentMethod; label: string }[] = [
  { value: 'credit', label: 'В борг постачальнику' },
  { value: 'bank_transfer', label: 'По перерахунку' },
  { value: 'cash', label: 'Готівкою з каси' },
  { value: 'other', label: 'Інший спосіб' },
];

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  /** Ціна продажу */
  price: number;
  /** Собівартість */
  cost_price: number;
  /** Відсоток націнки (розраховується автоматично) */
  markup_percent: number;
}

const InvoiceFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const { data: suppliersData } = useAllSuppliers();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();
  const createProductMutation = useCreateProduct();

  const [number, setNumber] = useState('');
  const [invoiceDate, setInvoiceDate] = useState(new Date().toISOString().split('T')[0]);
  const [isFiscal, setIsFiscal] = useState(false);
  const [supplierId, setSupplierId] = useState<string | null>(null);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod | ''>('');
  const [notes, setNotes] = useState('');
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  // Стан для модалки створення нового товару
  const [isAnalyzing, setIsAnalyzing] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [showNewProductModal, setShowNewProductModal] = useState(false);
  const [newProduct, setNewProduct] = useState({
    title: '',
    barcode: '',
    price: '',
    cost_price: '',
    unit: 'pcs',
  });

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
    const price = parseFloat(product.price) || 0;
    const costPrice = parseFloat(product.cost_price) || price;
    const markup = costPrice > 0 ? Math.round(((price - costPrice) / costPrice) * 100) : 0;

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
          price,
          cost_price: costPrice,
          markup_percent: markup,
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
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        // Перераховуємо націнку на основі собівартості
        const markup = item.cost_price > 0 ? Math.round(((price - item.cost_price) / item.cost_price) * 100) : 0;
        return { ...item, price, markup_percent: markup };
      })
    );
  };

  const updateCostPrice = (productId: string, costPrice: number) => {
    setCart((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        // Перераховуємо націнку на основі нової собівартості
        const markup = costPrice > 0 ? Math.round(((item.price - costPrice) / costPrice) * 100) : 0;
        return { ...item, cost_price: costPrice, markup_percent: markup };
      })
    );
  };

  const updateMarkup = (productId: string, markupPercent: number) => {
    setCart((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        // Перераховуємо ціну на основі собівартості та націнки
        const newPrice = item.cost_price > 0
          ? Math.round(item.cost_price * (1 + markupPercent / 100) * 100) / 100
          : item.price;
        return { ...item, markup_percent: markupPercent, price: newPrice };
      })
    );
  };

  const removeFromCart = (productId: string) => {
    setCart((prev) => prev.filter((item) => item.product_id !== productId));
  };

  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
  const totalCost = cart.reduce((sum, item) => sum + item.quantity * item.cost_price, 0);
  const totalMarkup = totalCost > 0 ? Math.round(((totalAmount - totalCost) / totalCost) * 100) : 0;

  const handleSave = async (andConfirm: boolean = false) => {
    if (!number.trim()) {
      toast.error('Введіть номер накладної');
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
        document_type: 'invoice',
        number: number.trim(),
        supplier_id: supplierId,
        invoice_date: new Date(invoiceDate).toISOString(),
        payment_method: paymentMethod || undefined,
        is_fiscal: isFiscal,
        notes: notes || undefined,
        items: cart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          cost_price: item.cost_price,
          markup_percent,
          total: item.quantity * item.price,
        })),
      });

      if (andConfirm) {
        await confirmMutation.mutateAsync({ id: doc.id, documentType: 'invoice' });
      }

      navigate('/documents');
    } catch {
      // Error handled
    }
  };

  const handleImageUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setIsAnalyzing(true);
    try {
      const formData = new FormData();
      formData.append('file', file);

      const response = await api.post('/ocr/invoice', formData);
      const result = response.data;

      if (result.success) {
        const data = result.data;

        // Заповнити поля форми
        if (data.document_number) setNumber(data.document_number);
        if (data.invoice_date) setInvoiceDate(data.invoice_date);
        if (data.is_fiscal !== null) setIsFiscal(data.is_fiscal);
        if (data.supplier_name) {
          // Знайти постачальника за назвою серед suppliersData
          const supplier = suppliersData?.find(
            s => s.name.toLowerCase().includes(data.supplier_name.toLowerCase())
          );
          if (supplier) setSupplierId(String(supplier.id));
        }
        if (data.payment_method) setPaymentMethod(data.payment_method);

        // Додати товари
        if (data.items && data.items.length > 0) {
          const newCart: CartItem[] = data.items.map((item: any) => ({
            product_id: '',  // буде заповнено після пошуку
            product_title: item.product_name,
            product_barcode: null,
            quantity: item.quantity,
            price: item.price,
            cost_price: item.cost_price,
            markup_percent: item.cost_price > 0
              ? Math.round(((item.price - item.cost_price) / item.cost_price) * 100)
              : 0,
          }));
          setCart(newCart);
          toast.success(`Знайдено ${data.items.length} товарів`);
        }

        toast.success('Накладну розпізнано!');
      } else {
        toast.error(result.error || 'Помилка аналізу накладної');
      }
    } catch (err: any) {
      const detail = err?.response?.data?.detail || err?.response?.data?.error || "Помилка з'єднання";
      toast.error(detail);
    } finally {
      setIsAnalyzing(false);
      // Скинути input, щоб можна було вибрати той самий файл повторно
      e.target.value = '';
    }
  };

  // Створення нового товару
  const handleCreateProduct = async () => {
    if (!newProduct.title.trim()) {
      toast.error('Введіть назву товару');
      return;
    }
    try {
      const product = await createProductMutation.mutateAsync({
        title: newProduct.title.trim(),
        barcode: newProduct.barcode.trim() || undefined,
        price: parseFloat(newProduct.price) || 0,
        cost_price: parseFloat(newProduct.cost_price) || 0,
        unit: (newProduct.unit || 'pcs') as any,
      });
      // Додаємо новий товар одразу в кошик
      addToCart(product);
      setShowNewProductModal(false);
      setNewProduct({ title: '', barcode: '', price: '', cost_price: '', unit: 'pcs' });
      toast.success('Товар створено та додано до накладної');
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

  const paymentMethodOptions = [
    { value: '', label: 'Не вибрано' },
    ...PAYMENT_METHODS.map((pm) => ({
      value: pm.value,
      label: pm.label,
    })),
  ];

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={goBack}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
              Прибуткова накладна
            </h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Створення прибуткової накладної
            </p>
          </div>
        </div>

        {/* Кнопка завантаження зображення */}
        <div className="relative">
          <button
            onClick={() => fileInputRef.current?.click()}
            disabled={isAnalyzing}
            className="p-2.5 rounded-xl bg-primary-50 dark:bg-primary-900/20
                       text-primary-600 dark:text-primary-400
                       hover:bg-primary-100 dark:hover:bg-primary-900/30
                       disabled:opacity-50 disabled:cursor-not-allowed
                       transition-all duration-200 shadow-sm hover:shadow-md"
            title="Завантажити зображення накладної для автоматичного заповнення"
          >
            {isAnalyzing ? (
              <Loader2 className="w-5 h-5 animate-spin" />
            ) : (
              <ImageUp className="w-5 h-5" />
            )}
          </button>
        </div>
      </div>

      {/* Прихований input для вибору файлу */}
      <input
        type="file"
        accept="image/*"
        className="hidden"
        ref={fileInputRef}
        onChange={handleImageUpload}
      />

      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
          <Input
            label="Номер накладної *"
            value={number}
            onChange={(e) => setNumber(e.target.value)}
            placeholder="Наприклад: ПН-001"
            autoFocus
          />
          <Input
            label="Дата накладної"
            type="date"
            value={invoiceDate}
            onChange={(e) => setInvoiceDate(e.target.value)}
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
                Фіскальна накладна
              </span>
            </label>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Select
            label="Постачальник *"
            options={supplierOptions}
            value={String(supplierId || '')}
            onChange={(e) => setSupplierId(e.target.value || null)}
          />
          <Select
            label="Спосіб оплати"
            options={paymentMethodOptions}
            value={paymentMethod}
            onChange={(e) => setPaymentMethod(e.target.value as PaymentMethod | '')}
          />
        </div>

        {/* Пошук товару + кнопка додати новий */}
        <div className="flex gap-3 items-end">
          <div className="flex-1 relative">
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
          <Button
            variant="secondary"
            onClick={() => setShowNewProductModal(true)}
            icon={<Package className="w-4 h-4" />}
          >
            Додати новий товар
          </Button>
        </div>

        {/* Таблиця товарів */}
        {cart.length > 0 && (
          <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
            <table className="w-full">
              <thead>
                <tr className="bg-gray-50 dark:bg-slate-800/50">
                  <th className="table-header">Товар</th>
                  <th className="table-header w-24">Кількість</th>
                  <th className="table-header w-28">Собівартість</th>
                  <th className="table-header w-28">Ціна продажу</th>
                  <th className="table-header w-28">Націнка</th>
                  <th className="table-header w-28">Сума</th>
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
                      {item.product_barcode && (
                        <p className="text-xs text-gray-400">{item.product_barcode}</p>
                      )}
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
                        value={item.cost_price}
                        onChange={(e) =>
                          updateCostPrice(item.product_id, parseFloat(e.target.value) || 0)
                        }
                        className="w-24 input-field text-right px-3"
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
                    <td className="table-cell">
                      <div className="flex items-center gap-1">
                        <input
                          type="number"
                          step="0.1"
                          min="0"
                          value={item.markup_percent}
                          onChange={(e) =>
                            updateMarkup(item.product_id, parseFloat(e.target.value) || 0)
                          }
                          className="w-28 input-field text-right px-3"
                        />
                        <span className="text-sm text-gray-400">%</span>
                      </div>
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
                <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                  <td colSpan={4} className="px-4 py-3 text-right text-gray-700 dark:text-gray-300">
                    Загальна собівартість:
                  </td>
                  <td colSpan={2} className="px-4 py-3 text-gray-900 dark:text-gray-100">
                    {formatCurrency(totalCost)}
                  </td>
                  <td></td>
                </tr>
                <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                  <td colSpan={4} className="px-4 py-3 text-right text-gray-700 dark:text-gray-300">
                    Загальна сума продажу:
                  </td>
                  <td colSpan={2} className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                    {formatCurrency(totalAmount)}
                  </td>
                  <td></td>
                </tr>
                <tr className="bg-gray-50 dark:bg-slate-800/50">
                  <td colSpan={4} className="px-4 py-3 text-right text-sm text-gray-500 dark:text-gray-400">
                    Середня націнка:
                  </td>
                  <td colSpan={2} className="px-4 py-3 text-sm font-medium text-green-600 dark:text-green-400">
                    {totalMarkup}%
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

      {/* Модалка створення нового товару */}
      {showNewProductModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-white dark:bg-slate-800 rounded-2xl shadow-xl w-full max-w-md mx-4 p-6 space-y-4">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Новий товар
            </h3>
            <Input
              label="Назва товару *"
              value={newProduct.title}
              onChange={(e) => setNewProduct((p) => ({ ...p, title: e.target.value }))}
              placeholder="Введіть назву"
              autoFocus
            />
            <Input
              label="Штрих-код"
              value={newProduct.barcode}
              onChange={(e) => setNewProduct((p) => ({ ...p, barcode: e.target.value }))}
              placeholder="Опціонально"
            />
            <div className="grid grid-cols-2 gap-3">
              <Input
                label="Собівартість"
                type="number"
                step="0.01"
                min="0"
                value={newProduct.cost_price}
                onChange={(e) => setNewProduct((p) => ({ ...p, cost_price: e.target.value }))}
                placeholder="0.00"
              />
              <Input
                label="Ціна продажу"
                type="number"
                step="0.01"
                min="0"
                value={newProduct.price}
                onChange={(e) => setNewProduct((p) => ({ ...p, price: e.target.value }))}
                placeholder="0.00"
              />
            </div>
            <Input
              label="Одиниця виміру"
              value={newProduct.unit}
              onChange={(e) => setNewProduct((p) => ({ ...p, unit: e.target.value }))}
              placeholder="шт"
            />
            <div className="flex justify-end gap-3 pt-2">
              <Button
                variant="secondary"
                onClick={() => setShowNewProductModal(false)}
              >
                Скасувати
              </Button>
              <Button
                onClick={handleCreateProduct}
                isLoading={createProductMutation.isPending}
              >
                Створити та додати
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default InvoiceFormPage;
