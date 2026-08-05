import React, { useState, useCallback } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Trash2, Search, ArrowLeft, Save, CheckCircle } from 'lucide-react';
import api from '@/services/api';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useSearchProducts } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { DecimalInput } from '@/components/ui/DecimalInput';
import { Input } from '@/components/ui/Input';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';

/** Елемент кошика інвентаризації */
interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  /** Чи товар ваговий (для step в інпуті) */
  is_weight?: boolean;
  /** Фактична кількість (вводить користувач) */
  actual_quantity: number;
  /** Облікова кількість (поточний залишок з Product.stock) */
  accounting_quantity: number;
  /** Різниця = actual - accounting (розраховується автоматично) */
  difference: number;
  /** Собівартість одиниці */
  cost_price: number;
  /** Ціна продажу одиниці */
  price: number;
  /** Сума собівартості = actual_quantity * cost_price */
  total_cost: number;
  /** Сума продажу = actual_quantity * price */
  total_selling: number;
  /** Сума відхилення = difference * cost_price (фінансовий вимір) */
  deviation_sum: number;
}

/** Заокруглення до 2 знаків після коми */
const round2 = (val: number): number => Math.round(val * 100) / 100;

const InventoryFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;
  const { goBack } = useBackNavigation();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();

  // ─── Основні стани ───────────────────────────────────────────────
  const [location, setLocation] = useState('');
  const [inventoryDate, setInventoryDate] = useState(new Date().toISOString().split('T')[0]);
  const [notes, setNotes] = useState('');
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  // ─── Стани модалки ───────────────────────────────────────────────
  const [modalProduct, setModalProduct] = useState<any>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [modalQuantity, setModalQuantity] = useState(1);
  const [modalCostPrice, setModalCostPrice] = useState(0);
  const [modalPrice, setModalPrice] = useState(0);

  // ─── Завантаження даних для редагування ─────────────────────
  const [isLoadingEdit, setIsLoadingEdit] = useState(false);
  const [editLoaded, setEditLoaded] = useState(false);

  React.useEffect(() => {
    if (!id || editLoaded) return;
    
    const loadInventory = async () => {
      setIsLoadingEdit(true);
      try {
        const response = await api.get(`/inventory/${id}`);
        const data = response.data;
        
        setLocation(data.location || '');
        setInventoryDate(data.inventory_date ? data.inventory_date.split('T')[0] : new Date().toISOString().split('T')[0]);
        setNotes(data.notes || '');
        
        if (data.items && data.items.length > 0) {
          const cartItems: CartItem[] = data.items.map((item: any) => {
            const actualQty = Number(item.actual_quantity || 0);
            const accountingQty = Number(item.accounting_quantity || 0);
            const diff = Number(item.difference || (actualQty - accountingQty));
            const costPrice = Number(item.cost_price || 0);
            const price = Number(item.price || 0);
            
            return {
              product_id: item.product_id || item.product?.id || '',
              product_title: item.product?.title || item.product_name || '',
              product_barcode: item.product?.barcode || null,
              is_weight: item.product?.is_weight || false,
              actual_quantity: actualQty,
              accounting_quantity: accountingQty,
              difference: round2(diff),
              cost_price: costPrice,
              price: price,
              total_cost: round2(actualQty * costPrice),
              total_selling: round2(actualQty * price),
              deviation_sum: round2(diff * costPrice),
            };
          });
          setCart(cartItems);
        }
        
        setEditLoaded(true);
      } catch (e: any) {
        toast.error('Помилка завантаження інвентаризації');
      } finally {
        setIsLoadingEdit(false);
      }
    };
    
    loadInventory();
  }, [id, editLoaded]);

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

  // ─── Відкриття модалки при виборі товару ─────────────────────────
  const openAddModal = (product: any) => {
    setModalProduct(product);
    setModalQuantity(1);
    setModalCostPrice(Number(product.cost_price) || Number(product.purchase_price) || 0);
    setModalPrice(Number(product.price) || 0);
    setIsModalOpen(true);
  };

  // ─── Підтвердження додавання через модалку ───────────────────────
  const confirmAddItem = () => {
    if (!modalProduct || modalQuantity < 0) return; // ← < 0 замість <= 0

    const existing = cart.find((item) => item.product_id === modalProduct.id);
    if (existing) {
      // ДОДАЄМО до існуючої кількості (а не замінюємо)
      setCart((prev) =>
        prev.map((item) => {
          if (item.product_id !== modalProduct.id) return item;
          const newActual = item.actual_quantity + modalQuantity; // ← ДОДАВАННЯ
          const diff = newActual - item.accounting_quantity;
          return {
            ...item,
            actual_quantity: newActual,
            difference: round2(diff),
            cost_price: modalCostPrice,
            price: modalPrice,
            total_cost: round2(newActual * modalCostPrice),
            total_selling: round2(newActual * modalPrice),
            deviation_sum: round2(diff * modalCostPrice),
          };
        })
      );
    } else {
      // Новий товар
      const accountingQty = Number(modalProduct.stock) || 0;
      const diff = modalQuantity - accountingQty;
      setCart((prev) => [
        ...prev,
        {
          product_id: modalProduct.id,
          product_title: modalProduct.title,
          product_barcode: modalProduct.barcode || null,
          is_weight: modalProduct.is_weight || false,
          actual_quantity: modalQuantity,
          accounting_quantity: accountingQty,
          difference: round2(diff),
          cost_price: modalCostPrice,
          price: modalPrice,
          total_cost: round2(modalQuantity * modalCostPrice),
          total_selling: round2(modalQuantity * modalPrice),
          deviation_sum: round2(diff * modalCostPrice),
        },
      ]);
    }

    setIsModalOpen(false);
    setModalProduct(null);
    setSearchQuery('');
    setShowSearch(false);
  };

  // ─── Оновлення кількості в таблиці ───────────────────────────────
  const updateQuantity = (productId: string, actualQty: number) => {
    // Заокруглюємо до 3 знаків для узгодженості
    actualQty = Math.round(actualQty * 1000) / 1000;
    if (actualQty < 0) actualQty = 0;
    setCart((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        const diff = actualQty - item.accounting_quantity;
        return {
          ...item,
          actual_quantity: actualQty,
          difference: round2(diff),
          total_cost: round2(actualQty * item.cost_price),
          total_selling: round2(actualQty * item.price),
          deviation_sum: round2(diff * item.cost_price),
        };
      })
    );
  };

  // ─── Видалення з кошика ──────────────────────────────────────────
  const removeFromCart = (productId: string) => {
    setCart((prev) => prev.filter((item) => item.product_id !== productId));
  };

  // ─── Підсумки ────────────────────────────────────────────────────
  const totalAccounting = round2(cart.reduce((s, i) => s + i.accounting_quantity, 0));
  const totalActual = round2(cart.reduce((s, i) => s + i.actual_quantity, 0));
  const totalDifference = round2(cart.reduce((s, i) => s + i.difference, 0));
  const totalCostSum = round2(cart.reduce((s, i) => s + i.total_cost, 0));
  const totalSellingSum = round2(cart.reduce((s, i) => s + i.total_selling, 0));
  const totalDeviationSum = round2(cart.reduce((s, i) => s + i.deviation_sum, 0));

  // ─── Збереження документу ────────────────────────────────────────
  const handleSave = async (andConfirm: boolean = false) => {
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар');
      return;
    }

    try {
      let doc;
      
      if (isEdit) {
        // Оновлення існуючої
        const response = await api.put(`/inventory/${id}`, {
          location: location.trim(),
          inventory_date: new Date(inventoryDate).toISOString(),
          notes: notes || undefined,
          items: cart.map(({ product_title, product_barcode, is_weight, total_cost, total_selling, deviation_sum, ...item }) => ({
            product_id: item.product_id,
            actual_quantity: item.actual_quantity,
            accounting_quantity: item.accounting_quantity,
            difference: item.difference,
            cost_price: item.cost_price,
            price: item.price,
          })),
        });
        doc = response.data;
        
        if (andConfirm && doc.status === 'draft') {
          await api.post(`/inventory/${id}/confirm`, { status: 'confirmed' });
        }
      } else {
        // Створення нової
        doc = await createMutation.mutateAsync({
          document_type: 'inventory',
          location: location.trim(),
          inventory_date: new Date(inventoryDate).toISOString(),
          notes: notes || undefined,
          items: cart.map(
            ({ product_title, product_barcode, is_weight, total_cost, total_selling, deviation_sum, ...item }) => ({
              product_id: item.product_id,
              actual_quantity: item.actual_quantity,
              accounting_quantity: item.accounting_quantity,
              difference: item.difference,
              cost_price: item.cost_price,
              price: item.price,
            })
          ),
        } as any);

        if (andConfirm) {
          await confirmMutation.mutateAsync({ id: doc.id, documentType: 'inventory' as any });
        }
      }

      navigate('/documents');
    } catch {
      // Error handled
    }
  };

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-slate-900">
      <div className="p-4 md:p-6 lg:p-8 space-y-6">
        {/* ═══ Заголовок ═══════════════════════════════════════════ */}
        <div className="flex items-center gap-4">
          <button aria-label="Назад"
            onClick={goBack}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
              {isEdit ? 'Редагування інвентаризації' : 'Інвентаризація товарів'}
            </h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              {isEdit ? `№${id?.slice(0, 8)}...` : 'Проведення інвентаризації'}
            </p>
          </div>
        </div>

        <div className="card p-6 space-y-6">
          {/* ─── Поля: Локація та Дата ──────────────────────────── */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <Input
              label="Місце проведення"
              value={location}
              onChange={(e) => setLocation(e.target.value)}
              placeholder="Наприклад: Основний склад"
              autoFocus
            />
            <Input
              label="Дата інвентаризації"
              type="date"
              value={inventoryDate}
              onChange={(e) => setInventoryDate(e.target.value)}
            />
          </div>

          {/* ─── Пошук товару ───────────────────────────────────── */}
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
                    onClick={() => {
                      setSearchQuery('');
                      setShowSearch(false);
                      openAddModal(product);
                    }}
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

          {/* ─── Таблиця товарів ────────────────────────────────── */}
          {cart.length > 0 && (
            <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-x-auto">
              <table className="w-full min-w-[1000px]">
                <thead>
                  <tr className="bg-gray-50 dark:bg-slate-800/50">
                    <th className="table-header">Товар</th>
                    <th className="table-header text-right">Облікова к-сть</th>
                    <th className="table-header text-right">Фактична к-сть</th>
                    <th className="table-header text-right">Різниця</th>
                    <th className="table-header text-right">Собівартість</th>
                    <th className="table-header text-right">Ціна продажу</th>
                    <th className="table-header text-right">Сума собівартості</th>
                    <th className="table-header text-right">Сума продажу</th>
                    <th className="table-header text-right">Сума відхилення</th>
                    <th className="table-header w-16"></th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {cart.map((item) => (
                    <tr key={item.product_id}>
                      {/* Товар (назва + ШК) */}
                      <td className="table-cell">
                        <p className="font-medium text-gray-900 dark:text-gray-100">
                          {item.product_title}
                        </p>
                        {item.product_barcode && (
                          <p className="text-xs text-gray-400">ШК: {item.product_barcode}</p>
                        )}
                      </td>
                      {/* Облікова кількість (read-only, сірий фон) */}
                      <td className="table-cell text-right">
                        <span className="inline-block w-20 px-3 py-1.5 text-center rounded-lg bg-gray-100 dark:bg-slate-700 text-gray-700 dark:text-gray-300 font-medium">
                          {item.accounting_quantity}
                        </span>
                      </td>
                      {/* Фактична кількість (редагується) */}
                      <td className="table-cell text-right">
                        <DecimalInput
                          value={item.actual_quantity}
                          onCommit={(n) => updateQuantity(item.product_id, n)}
                          className="w-20 input-field text-center px-3 no-spinner"
                        />
                      </td>
                      {/* Різниця (кольорова) */}
                      <td className="table-cell text-right">
                        <span
                          className={`inline-block w-20 px-3 py-1.5 text-center rounded-lg font-medium ${
                            item.difference > 0
                              ? 'bg-green-100 dark:bg-green-900/20 text-green-700 dark:text-green-400'
                              : item.difference < 0
                              ? 'bg-red-100 dark:bg-red-900/20 text-red-700 dark:text-red-400'
                              : 'bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400'
                          }`}
                        >
                          {item.difference > 0 ? '+' : ''}
                          {item.difference}
                        </span>
                      </td>
                      {/* Собівартість */}
                      <td className="table-cell text-right font-medium text-gray-900 dark:text-gray-100">
                        {formatCurrency(item.cost_price)}
                      </td>
                      {/* Ціна продажу */}
                      <td className="table-cell text-right font-medium text-gray-900 dark:text-gray-100">
                        {formatCurrency(item.price)}
                      </td>
                      {/* Сума собівартості */}
                      <td className="table-cell text-right font-medium text-blue-600 dark:text-blue-400">
                        {formatCurrency(item.total_cost)}
                      </td>
                      {/* Сума продажу */}
                      <td className="table-cell text-right font-medium text-emerald-600 dark:text-emerald-400">
                        {formatCurrency(item.total_selling)}
                      </td>
                      {/* Сума відхилення (кольорова: зелений/червоний/звичайний) */}
                      <td className="table-cell text-right">
                        <span
                          className={`font-medium ${
                            item.deviation_sum > 0
                              ? 'text-green-600 dark:text-green-400'
                              : item.deviation_sum < 0
                              ? 'text-red-600 dark:text-red-400'
                              : 'text-gray-900 dark:text-gray-100'
                          }`}
                        >
                          {formatCurrency(item.deviation_sum)}
                        </span>
                      </td>
                      {/* Дії (видалити) */}
                      <td className="table-cell">
                        <button
                          onClick={() => removeFromCart(item.product_id)}
                          className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 dark:hover:bg-danger-900/20 transition-colors"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
                {/* ═══ ПІДСУМКИ ═══ */}
                <tfoot>
                  <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                    <td className="px-4 py-3 text-gray-700 dark:text-gray-300">Загалом:</td>
                    <td className="px-4 py-3 text-right text-gray-900 dark:text-gray-100">
                      {totalAccounting}
                    </td>
                    <td className="px-4 py-3 text-right text-gray-900 dark:text-gray-100">
                      {totalActual}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <span
                        className={`font-bold text-lg ${
                          totalDifference > 0
                            ? 'text-green-600 dark:text-green-400'
                            : totalDifference < 0
                            ? 'text-red-600 dark:text-red-400'
                            : 'text-gray-900 dark:text-gray-100'
                        }`}
                      >
                        {totalDifference > 0 ? '+' : ''}
                        {totalDifference}
                      </span>
                    </td>
                    <td></td>
                    <td></td>
                    <td className="px-4 py-3 text-right font-bold text-blue-700 dark:text-blue-400 text-lg">
                      {formatCurrency(totalCostSum)}
                    </td>
                    <td className="px-4 py-3 text-right font-bold text-emerald-700 dark:text-emerald-400 text-lg">
                      {formatCurrency(totalSellingSum)}
                    </td>
                    <td
                      className="px-4 py-3 text-right font-bold text-lg"
                      style={{
                        color:
                          totalDeviationSum > 0
                            ? '#16a34a'
                            : totalDeviationSum < 0
                            ? '#dc2626'
                            : 'inherit',
                      }}
                    >
                      {formatCurrency(totalDeviationSum)}
                    </td>
                    <td></td>
                  </tr>
                </tfoot>
              </table>
            </div>
          )}

          {/* ─── Примітки ───────────────────────────────────────── */}
          <Input
            label="Примітки"
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder="Додаткова інформація..."
          />

          {/* ─── Кнопки дій ─────────────────────────────────────── */}
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
              Зберегти чернетку
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

      {/* ═══════════════════════════════════════════════════════════ */}
      {/* ═══ МОДАЛКА ДОДАВАННЯ ТОВАРУ ════════════════════════════ */}
      {/* ═══════════════════════════════════════════════════════════ */}
      {isModalOpen && modalProduct && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          onClick={() => setIsModalOpen(false)}
        >
          <div
            className="bg-white dark:bg-slate-800 rounded-2xl shadow-2xl w-full max-w-md mx-4 p-6 space-y-4"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Назва товару та ШК */}
            <div className="text-center">
              <h3 className="text-lg font-bold text-gray-900 dark:text-gray-100">
                {modalProduct.title}
              </h3>
              {modalProduct.barcode && (
                <p className="text-sm text-gray-400">ШК: {modalProduct.barcode}</p>
              )}
            </div>

            {/* Поля модалки */}
            <div className="space-y-4">
              {/* Фактична кількість */}
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Фактична кількість *
                </label>
                <DecimalInput
                  value={modalQuantity}
                  onCommit={setModalQuantity}
                  className="input-field w-full text-center text-lg font-bold no-spinner"
                  autoFocus
                />
              </div>

              {/* Собівартість */}
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Собівартість (грн)
                </label>
                <DecimalInput
                  value={modalCostPrice}
                  onCommit={setModalCostPrice}
                  className="input-field w-full no-spinner"
                />
              </div>

              {/* Ціна продажу */}
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Ціна продажу (грн)
                </label>
                <DecimalInput
                  value={modalPrice}
                  onCommit={setModalPrice}
                  className="input-field w-full no-spinner"
                />
              </div>

              {/* Попередній перегляд сум */}
              <div className="grid grid-cols-2 gap-4 pt-2 border-t border-gray-200 dark:border-slate-700">
                <div className="text-center">
                  <p className="text-xs text-gray-400">Сума собівартості</p>
                  <p className="text-lg font-bold text-blue-600 dark:text-blue-400">
                    {formatCurrency(modalQuantity * modalCostPrice)}
                  </p>
                </div>
                <div className="text-center">
                  <p className="text-xs text-gray-400">Сума продажу</p>
                  <p className="text-lg font-bold text-emerald-600 dark:text-emerald-400">
                    {formatCurrency(modalQuantity * modalPrice)}
                  </p>
                </div>
              </div>
            </div>

            {/* Кнопки */}
            <div className="flex gap-3 pt-2">
              <Button
                variant="secondary"
                className="flex-1"
                onClick={() => setIsModalOpen(false)}
              >
                Скасувати
              </Button>
              <Button
                className="flex-1"
                onClick={confirmAddItem}
                disabled={modalQuantity < 0}
              >
                Додати
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default InventoryFormPage;
