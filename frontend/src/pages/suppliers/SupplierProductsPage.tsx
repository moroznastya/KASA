import React, { useState, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  ArrowLeft, Package, Search, TrendingUp, TrendingDown,
  DollarSign, Box, BarChart3, ShoppingCart, Plus, Minus,
  Trash2, Check, X, FileText, AlertCircle, ExternalLink
} from 'lucide-react';
import api from '@/services/api';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { Input } from '@/components/ui/Input';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';
interface ProductItem {
  id: string;
  barcode: string | null;
  sku: string | null;
  title: string;
  price: string;
  cost_price: string;
  stock: string;
  unit: string | null;
  category_name: string | null;
}

interface MovementItem {
  id: string;
  date: string;
  document_type: string;
  document_number: string;
  document_id: string;
  quantity: string;
  price: string | null;
  total: string | null;
  notes: string | null;
}

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
  cost_price: number;
}

const DOCUMENT_TYPE_LABELS: Record<string, string> = {
  invoice: 'Прибуткова накладна',
  return_invoice: 'Повернення',
  receipt: 'Чек',
  write_off: 'Списання',
  transfer: 'Переміщення',
};

const DOCUMENT_TYPE_COLORS: Record<string, string> = {
  invoice: 'text-green-600 bg-green-50 dark:bg-green-900/20 border-green-200',
  return_invoice: 'text-red-600 bg-red-50 dark:bg-red-900/20 border-red-200',
  receipt: 'text-blue-600 bg-blue-50 dark:bg-blue-900/20 border-blue-200',
  write_off: 'text-orange-600 bg-orange-50 dark:bg-orange-900/20 border-orange-200',
  transfer: 'text-purple-600 bg-purple-50 dark:bg-purple-900/20 border-purple-200',
};


/** Мапа типів документів на шляхи перегляду */
const DOCUMENT_TYPE_ROUTES: Record<string, string> = {
  invoice: "/documents/invoice",
  return_invoice: "/documents/return",
  receipt: "/receipts",
  write_off: "/documents/write-off",
  transfer: "/documents/transfer",
};
const SupplierProductsPage: React.FC = () => {
  const { id: supplierId } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const [search, setSearch] = useState('');
  const [selectedProductId, setSelectedProductId] = useState<string | null>(null);

  // Стан кошика для замовлення
  const [cart, setCart] = useState<CartItem[]>([]);
  const [showCart, setShowCart] = useState(false);

  // Запит товарів постачальника
  const { data: productsData, isLoading: productsLoading } = useQuery({
    queryKey: ['supplier-products', supplierId, search],
    queryFn: async () => {
      const params: any = {};
      if (search) params.search = search;
      const response = await api.get(`/suppliers/${supplierId}/products`, { params });
      return response.data;
    },
    enabled: !!supplierId,
  });

  // Запит руху вибраного товару
  const { data: movementsData, isLoading: movementsLoading } = useQuery({
    queryKey: ['supplier-product-movements', supplierId, selectedProductId],
    queryFn: async () => {
      const response = await api.get(`/suppliers/${supplierId}/products/${selectedProductId}/movements`);
      return response.data;
    },
    enabled: !!supplierId && !!selectedProductId,
  });

  const products: ProductItem[] = productsData?.products || [];
  const movements: MovementItem[] = movementsData?.movements || [];
  const selectedProduct = movementsData?.product || products.find(p => p.id === selectedProductId);

  // ─── Функції роботи з кошиком ───────────────────────

  const addToCart = useCallback((product: ProductItem) => {
    setCart(prev => {
      const existing = prev.find(item => item.product_id === product.id);
      if (existing) {
        return prev.map(item =>
          item.product_id === product.id
            ? { ...item, quantity: item.quantity + 1 }
            : item
        );
      }
      return [...prev, {
        product_id: product.id,
        product_title: product.title,
        product_barcode: product.barcode,
        quantity: 1,
        price: Number(product.price) || 0,
        cost_price: Number(product.cost_price) || 0,
      }];
    });
    setShowCart(true);
    toast.success(`"${product.title}" додано до замовлення`);
  }, []);

  const updateCartQuantity = useCallback((productId: string, quantity: number) => {
    if (quantity <= 0) {
      setCart(prev => prev.filter(item => item.product_id !== productId));
    } else {
      setCart(prev =>
        prev.map(item =>
          item.product_id === productId ? { ...item, quantity } : item
        )
      );
    }
  }, []);

  const removeFromCart = useCallback((productId: string) => {
    setCart(prev => prev.filter(item => item.product_id !== productId));
    const item = cart.find(i => i.product_id === productId);
    if (item) toast.success(`"${item.product_title}" видалено із замовлення`);
  }, [cart]);

  const clearCart = useCallback(() => {
    setCart([]);
    setShowCart(false);
  }, []);

  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
  const totalCost = cart.reduce((sum, item) => sum + item.quantity * item.cost_price, 0);

  // Перехід до створення накладної
  const goToCreateInvoice = useCallback(() => {
    if (cart.length === 0) {
      toast.error('Додайте хоча б один товар до замовлення');
      return;
    }
    // Зберігаємо кошик у sessionStorage і переходимо на сторінку нової накладної
    const cartData = cart.map(item => ({
      product_id: item.product_id,
      product_title: item.product_title,
      product_barcode: item.product_barcode,
      quantity: item.quantity,
      price: item.price,
      cost_price: item.cost_price,
    }));
    sessionStorage.setItem('invoice_cart', JSON.stringify(cartData));
    if (supplierId) {
      sessionStorage.setItem('invoice_supplier_id', supplierId);
    }
    navigate('/documents/invoice/new');
  }, [cart, supplierId, navigate]);

  return (
    <div className="space-y-6">
      {/* Header */}
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
              Товари постачальника
            </h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              {productsData?.supplier_name || 'Завантаження...'}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          {productsData && (
            <div className="flex items-center gap-4 text-sm mr-4">
              <div className="flex items-center gap-1.5 text-gray-500">
                <Package className="w-4 h-4" />
                <span>Товарів: <strong>{productsData.total_products}</strong></span>
              </div>
              <div className="flex items-center gap-1.5 text-gray-500">
                <DollarSign className="w-4 h-4" />
                <span>Вартість залишків: <strong>{formatCurrency(productsData.total_stock_value)}</strong></span>
              </div>
            </div>
          )}

          {/* Кнопка кошика */}
          <div className="relative">
            <Button
              onClick={() => setShowCart(!showCart)}
              variant={cart.length > 0 ? 'primary' : 'secondary'}
              icon={<ShoppingCart className="w-4 h-4" />}
            >
              Замовлення
              {cart.length > 0 && (
                <span className="ml-1.5 inline-flex items-center justify-center w-5 h-5 text-xs font-bold bg-white text-primary-700 rounded-full">
                  {cart.length}
                </span>
              )}
            </Button>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* ─── Список товарів ─────────────────────────── */}
        <div className={`${showCart ? 'lg:col-span-1' : 'lg:col-span-1'} card p-4`}>
          <div className="flex items-center justify-between mb-3">
            <h3 className="font-semibold text-gray-900 dark:text-gray-100 flex items-center gap-2">
              <Box className="w-4 h-4 text-primary-500" />
              Товари
            </h3>
          </div>

          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Пошук товару..."
            icon={<Search className="w-4 h-4" />}
            className="mb-3"
          />

          {productsLoading ? (
            <div className="flex justify-center py-8">
              <Spinner />
            </div>
          ) : products.length === 0 ? (
            <div className="text-center py-8 text-gray-400">
              <Package className="w-8 h-8 mx-auto mb-2" />
              <p className="text-sm">Немає товарів</p>
            </div>
          ) : (
            <div className="space-y-1 max-h-[60vh] overflow-y-auto">
              {products.map((product) => {
                const inCart = cart.find(item => item.product_id === product.id);
                return (
                  <div
                    key={product.id}
                    className={`group relative rounded-xl transition-colors border ${
                      selectedProductId === product.id
                        ? 'bg-primary-50 dark:bg-primary-900/20 border-primary-200 dark:border-primary-800'
                        : 'hover:bg-gray-50 dark:hover:bg-slate-700/50 border-transparent hover:border-gray-200 dark:hover:border-slate-600'
                    }`}
                  >
                    <button
                      onClick={() => setSelectedProductId(product.id)}
                      className="w-full text-left px-3 py-2.5 pr-12"
                    >
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                        {product.title}
                      </p>
                      <div className="flex items-center justify-between mt-1">
                        <span className="text-xs text-gray-400">
                          {product.barcode || product.sku || '—'}
                        </span>
                        <span className="text-xs font-medium text-gray-600 dark:text-gray-300">
                          {Number(product.stock).toFixed(3)} {product.unit || ''}
                        </span>
                      </div>
                      <div className="flex items-center justify-between mt-0.5">
                        <span className="text-xs text-gray-400">
                          {product.category_name || '—'}
                        </span>
                        <span className="text-xs text-gray-500">
                          {formatCurrency(product.cost_price)}
                        </span>
                      </div>
                      {inCart && (
                        <div className="mt-1.5 flex items-center gap-1">
                          <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded text-xs font-medium bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300">
                            <ShoppingCart className="w-3 h-3" />
                            {inCart.quantity} шт.
                          </span>
                        </div>
                      )}
                    </button>

                    {/* Кнопка додати до замовлення */}
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        addToCart(product);
                      }}
                      className="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 rounded-lg
                        text-gray-300 hover:text-primary-600 hover:bg-primary-50
                        dark:hover:text-primary-400 dark:hover:bg-primary-900/20
                        opacity-0 group-hover:opacity-100 transition-all"
                      title="Додати до замовлення"
                    >
                      <Plus className="w-4 h-4" />
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* ─── Права частина: рух товару або кошик ──── */}
        <div className={`${showCart ? 'lg:col-span-2' : 'lg:col-span-2'} space-y-4`}>
          {/* Панель кошика */}
          {showCart && (
            <div className="card p-4">
              <div className="flex items-center justify-between mb-4">
                <h3 className="font-semibold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                  <ShoppingCart className="w-4 h-4 text-primary-500" />
                  Замовлення постачальнику
                  {cart.length > 0 && (
                    <span className="text-sm font-normal text-gray-400">
                      ({cart.length} позицій)
                    </span>
                  )}
                </h3>
                <div className="flex items-center gap-2">
                  {cart.length > 0 && (
                    <button
                      onClick={clearCart}
                      className="text-xs text-gray-400 hover:text-danger-500 transition-colors flex items-center gap-1"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                      Очистити
                    </button>
                  )}
                  <button
                    onClick={() => setShowCart(false)}
                    className="p-1 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>
              </div>

              {cart.length === 0 ? (
                <div className="text-center py-8 text-gray-400">
                  <ShoppingCart className="w-10 h-10 mx-auto mb-2" />
                  <p className="text-sm">Кошик порожній</p>
                  <p className="text-xs mt-1">Натисніть + біля товару, щоб додати</p>
                </div>
              ) : (
                <div className="space-y-2">
                  {cart.map((item) => (
                    <div
                      key={item.product_id}
                      className="flex items-center justify-between p-3 rounded-xl bg-gray-50 dark:bg-slate-800/50 border border-gray-200 dark:border-slate-700"
                    >
                      <div className="min-w-0 flex-1">
                        <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                          {item.product_title}
                        </p>
                        <p className="text-xs text-gray-400 mt-0.5">
                          {formatCurrency(item.cost_price)} × {item.quantity}
                        </p>
                      </div>

                      <div className="flex items-center gap-3 ml-4">
                        {/* Регулювання кількості */}
                        <div className="flex items-center gap-1">
                          <button
                            onClick={() => updateCartQuantity(item.product_id, item.quantity - 1)}
                            className="p-1 rounded-md text-gray-400 hover:text-gray-600 hover:bg-gray-200 dark:hover:bg-slate-600 transition-colors"
                          >
                            <Minus className="w-3.5 h-3.5" />
                          </button>
                          <span className="w-8 text-center text-sm font-medium text-gray-900 dark:text-gray-100">
                            {item.quantity}
                          </span>
                          <button
                            onClick={() => updateCartQuantity(item.product_id, item.quantity + 1)}
                            className="p-1 rounded-md text-gray-400 hover:text-gray-600 hover:bg-gray-200 dark:hover:bg-slate-600 transition-colors"
                          >
                            <Plus className="w-3.5 h-3.5" />
                          </button>
                        </div>

                        <span className="text-sm font-semibold text-gray-900 dark:text-gray-100 w-20 text-right">
                          {formatCurrency(item.quantity * item.price)}
                        </span>

                        <button
                          onClick={() => removeFromCart(item.product_id)}
                          className="p-1.5 rounded-lg text-gray-300 hover:text-danger-500 hover:bg-danger-50 dark:hover:bg-danger-900/20 transition-colors"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </div>
                    </div>
                  ))}

                  {/* Підсумки */}
                  <div className="border-t border-gray-200 dark:border-slate-700 pt-3 mt-3">
                    <div className="flex items-center justify-between text-sm">
                      <span className="text-gray-500">Собівартість:</span>
                      <span className="font-medium text-gray-700 dark:text-gray-300">
                        {formatCurrency(totalCost)}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-sm mt-1">
                      <span className="text-gray-500">Сума продажу:</span>
                      <span className="font-medium text-gray-700 dark:text-gray-300">
                        {formatCurrency(totalAmount)}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-base font-bold mt-2 pt-2 border-t border-gray-200 dark:border-slate-700">
                      <span className="text-gray-900 dark:text-gray-100">До сплати:</span>
                      <span className="text-primary-600 dark:text-primary-400">
                        {formatCurrency(totalAmount)}
                      </span>
                    </div>
                  </div>

                  {/* Кнопка створення накладної */}
                  <Button
                    onClick={goToCreateInvoice}
                    className="w-full mt-4"
                    size="lg"
                    icon={<FileText className="w-5 h-5" />}
                  >
                    Створити прибуткову накладну
                  </Button>
                </div>
              )}
            </div>
          )}

          {/* Рух товару */}
          {!selectedProductId ? (
            !showCart && (
              <div className="card p-4">
                <div className="flex flex-col items-center justify-center py-16 text-gray-400">
                  <BarChart3 className="w-12 h-12 mb-3" />
                  <p className="text-lg font-medium">Виберіть товар</p>
                  <p className="text-sm mt-1">Щоб переглянути історію руху</p>
                </div>
              </div>
            )
          ) : movementsLoading ? (
            <div className="card p-4">
              <div className="flex justify-center py-16">
                <Spinner size="lg" />
              </div>
            </div>
          ) : (
            <div className="card p-4">
              <div className="space-y-4">
                {/* Інформація про товар */}
                {selectedProduct && (
                  <div className="bg-gray-50 dark:bg-slate-800/50 rounded-xl p-4">
                    <div className="flex items-start justify-between">
                      <div>
                        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                          {selectedProduct.title}
                        </h3>
                        <p className="text-xs text-gray-400 mt-0.5">
                          {selectedProduct.barcode || selectedProduct.sku || '—'}
                        </p>
                      </div>
                      <Button
                        onClick={() => addToCart(selectedProduct)}
                        size="sm"
                        icon={<Plus className="w-3.5 h-3.5" />}
                      >
                        Додати до замовлення
                      </Button>
                    </div>
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mt-3">
                      <div>
                        <p className="text-xs text-gray-500">Поточний залишок</p>
                        <p className="text-lg font-bold text-gray-900 dark:text-gray-100">
                          {Number(selectedProduct.stock).toFixed(3)} <span className="text-sm font-normal text-gray-500">{selectedProduct.unit || ''}</span>
                        </p>
                      </div>
                      <div>
                        <p className="text-xs text-gray-500">Собівартість</p>
                        <p className="text-lg font-bold text-gray-900 dark:text-gray-100">
                          {formatCurrency(selectedProduct.cost_price)}
                        </p>
                      </div>
                      <div>
                        <p className="text-xs text-gray-500">Роздрібна ціна</p>
                        <p className="text-lg font-bold text-gray-900 dark:text-gray-100">
                          {formatCurrency(selectedProduct.price)}
                        </p>
                      </div>
                      <div>
                        <p className="text-xs text-gray-500">Категорія</p>
                        <p className="text-lg font-bold text-gray-900 dark:text-gray-100">
                          {selectedProduct.category_name || '—'}
                        </p>
                      </div>
                    </div>
                  </div>
                )}

                {/* Рух товару */}
                <h4 className="font-semibold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                  <TrendingUp className="w-4 h-4 text-primary-500" />
                  Історія руху
                  {movementsData && (
                    <span className="text-sm font-normal text-gray-400">
                      ({movementsData.total_movements} записів)
                    </span>
                  )}
                </h4>

                {movements.length === 0 ? (
                  <div className="text-center py-8 text-gray-400">
                    <p>Немає записів руху для цього товару</p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {movements.map((movement) => {
                      const qty = Number(movement.quantity);
                      const isIncome = qty > 0;
                      const typeLabel = DOCUMENT_TYPE_LABELS[movement.document_type] || movement.document_type;
                      const typeColor = DOCUMENT_TYPE_COLORS[movement.document_type] || 'text-gray-600 bg-gray-50';

                      return (
                        <div
                          key={movement.id}
                          className="flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-slate-700 hover:bg-primary-50 dark:hover:bg-primary-900/10 cursor-pointer transition-colors group"
                          onClick={() => {
                            const route = DOCUMENT_TYPE_ROUTES[movement.document_type];
                            if (route && movement.document_id) {
                              navigate(`${route}/${movement.document_id}`);
                            }
                          }}
                          title={movement.document_id ? 'Відкрити документ' : undefined}
                        >
                          <div className="flex items-center gap-3 min-w-0 flex-1">
                            <div className={`flex-shrink-0 w-10 h-10 rounded-full flex items-center justify-center ${
                              isIncome
                                ? 'bg-green-50 dark:bg-green-900/20 text-green-600'
                                : 'bg-red-50 dark:bg-red-900/20 text-red-600'
                            }`}>
                              {isIncome ? (
                                <TrendingDown className="w-5 h-5 rotate-180" />
                              ) : (
                                <TrendingUp className="w-5 h-5" />
                              )}
                            </div>

                            <div className="min-w-0">
                              <div className="flex items-center gap-2">
                                <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium border ${typeColor}`}>
                                  {typeLabel}
                                </span>
                                <span className="text-sm font-medium text-primary-600 dark:text-primary-400 group-hover:underline flex items-center gap-1">
                                  №{movement.document_number}
                                  <ExternalLink className="w-3 h-3 text-gray-300 group-hover:text-primary-500 transition-colors opacity-0 group-hover:opacity-100" />
                                </span>
                              </div>
                              <p className="text-xs text-gray-400 mt-0.5">
                                {new Date(movement.date).toLocaleDateString('uk-UA', {
                                  day: 'numeric',
                                  month: 'long',
                                  year: 'numeric',
                                  hour: '2-digit',
                                  minute: '2-digit',
                                })}
                              </p>
                            </div>
                          </div>

                          <div className="text-right flex-shrink-0 ml-4">
                            <p className={`text-lg font-bold ${
                              isIncome
                                ? 'text-green-600 dark:text-green-400'
                                : 'text-red-600 dark:text-red-400'
                            }`}>
                              {isIncome ? '+' : ''}{qty.toFixed(3)}
                            </p>
                            {movement.total && (
                              <p className="text-xs text-gray-400">
                                {formatCurrency(Math.abs(Number(movement.total)))}
                              </p>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default SupplierProductsPage;
