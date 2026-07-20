import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Search, Plus, Minus, Trash2, ShoppingCart, CreditCard, Banknote, Barcode, Loader2 } from 'lucide-react';
import { useSearchProducts } from '@/hooks/useProducts';
import { useBarcodeSearch } from '@/hooks/useBarcodeSearch';
import { receiptService } from '@/services/receiptService';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { formatCurrency, formatUnit } from '@/utils/format';
import { ReceiptCreate, PaymentMethod } from '@/types/receipt';
import toast from 'react-hot-toast';

interface CartItem {
  product_id: string;
  product_name: string;
  product_barcode: string | null;
  quantity: number;
  price: number;
  vat_rate: number;
  is_weight: boolean;
}

const PosPage: React.FC = () => {
  const [cart, setCart] = useState<CartItem[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [barcodeInput, setBarcodeInput] = useState('');
  const [showPayment, setShowPayment] = useState(false);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod>('cash');
  const [cashAmount, setCashAmount] = useState('');
  const [cardAmount, setCardAmount] = useState('');
  const [isDebt, setIsDebt] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [showQuantityModal, setShowQuantityModal] = useState(false);
  const [selectedProduct, setSelectedProduct] = useState<any>(null);
  const [quantityInput, setQuantityInput] = useState('1');
  const searchInputRef = useRef<HTMLInputElement>(null);

  const { data: searchData } = useSearchProducts(searchQuery);

  const handleBarcodeFound = useCallback((product: any) => {
    addToCart(product);
    setBarcodeInput('');
    toast.success(`Додано: ${product.name}`);
  }, []);

  const { 
    barcode, 
    product: barcodeProduct, 
    isSearching: isBarcodeSearching, 
    error: barcodeError,
    setBarcode: handleBarcodeChange,
    reset: resetBarcode 
  } = useBarcodeSearch({
    onProductFound: handleBarcodeFound,
  });

  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  const addToCart = useCallback((product: any, quantity: number = 1) => {
    setCart((prev) => {
      const existing = prev.find((item) => item.product_id === product.id);
      if (existing) {
        return prev.map((item) =>
          item.product_id === product.id
            ? { ...item, quantity: item.quantity + quantity }
            : item
        );
      }
      return [
        ...prev,
        {
          product_id: product.id,
          product_name: product.name,
          product_barcode: product.barcode,
          quantity,
          price: parseFloat(product.price),
          vat_rate: product.vat_rate || 20,
          is_weight: product.is_weight || false,
        },
      ];
    });
  }, []);

  const handleProductSelect = (product: any) => {
    if (product.is_weight) {
      setSelectedProduct(product);
      setQuantityInput('1');
      setShowQuantityModal(true);
    } else {
      addToCart(product);
      setSearchQuery('');
    }
  };

  const handleQuantityConfirm = () => {
    if (selectedProduct) {
      const qty = parseFloat(quantityInput);
      if (qty > 0) {
        addToCart(selectedProduct, qty);
        setShowQuantityModal(false);
        setSelectedProduct(null);
        setSearchQuery('');
      }
    }
  };

  const updateQuantity = (productId: string, delta: number) => {
    setCart((prev) =>
      prev
        .map((item) =>
          item.product_id === productId
            ? { ...item, quantity: Math.max(0.001, item.quantity + delta) }
            : item
        )
        .filter((item) => item.quantity > 0)
    );
  };

  const setItemQuantity = (productId: string, quantity: number) => {
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

  const clearCart = () => {
    setCart([]);
  };

  const subtotal = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
  const vatAmount = cart.reduce(
    (sum, item) => sum + (item.quantity * item.price * item.vat_rate) / (100 + item.vat_rate),
    0
  );

  const handlePayment = async () => {
    if (cart.length === 0) {
      toast.error('Кошик порожній');
      return;
    }

    setIsProcessing(true);
    try {
      const receiptData: ReceiptCreate = {
        receipt_number: '',
        receipt_type: 'SALE',
        cashier_id: '',
        total_amount: subtotal.toFixed(2),
        items: cart.map((item) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
        })),
        payment_method: paymentMethod,
        cash_amount: paymentMethod === 'cash' ? subtotal : paymentMethod === 'mixed' ? (parseFloat(cashAmount) || 0) : 0,
        card_amount: paymentMethod === 'card' ? subtotal : paymentMethod === 'mixed' ? (parseFloat(cardAmount) || 0) : 0,
        is_debt: isDebt,
      };

      await receiptService.createReceipt(receiptData);
      toast.success('Чек створено успішно');
      setCart([]);
      setShowPayment(false);
      setCashAmount('');
      setCardAmount('');
      setIsDebt(false);
      setPaymentMethod('cash');
    } catch (error: any) {
      toast.error(error?.response?.data?.detail || 'Помилка створення чеку');
    } finally {
      setIsProcessing(false);
    }
  };

  const changeAmount =
    paymentMethod === 'cash' && parseFloat(cashAmount) >= subtotal
      ? parseFloat(cashAmount) - subtotal
      : 0;

  const searchResults = searchQuery.length >= 2 ? (searchData || []) : [];

  return (
    <div className="flex h-[calc(100vh-8rem)] gap-4">
      {/* Left panel - Product search */}
      <div className="w-80 flex flex-col gap-4">
        <div className="card p-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
            <input
              ref={searchInputRef}
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Пошук товару..."
              className="input-field pl-10"
              id="product-search"
              name="product-search"
              autoComplete="off"
            />
          </div>
          <div className="mt-2 relative">
            <Barcode className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
            <input
              type="text"
              value={barcodeInput}
              onChange={(e) => {
                const value = e.target.value;
                setBarcodeInput(value);
                handleBarcodeChange(value);
              }}
              placeholder="Штрих-код (сканер)..."
              className="input-field pl-10"
              id="barcode-input"
              name="barcode-input"
              autoComplete="off"
            />
            {isBarcodeSearching && (
              <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-primary-500 animate-spin" />
            )}
          </div>
          {/* Barcode error message */}
          {barcodeError && barcodeInput.length >= 8 && (
            <div className="mt-2 text-xs text-danger-500 bg-danger-50 dark:bg-danger-900/20 px-3 py-2 rounded-lg">
              {barcodeError}
            </div>
          )}
          {/* Barcode success - product found */}
          {barcodeProduct && !barcodeError && barcodeInput.length >= 8 && (
            <div className="mt-2 text-xs text-success-600 bg-success-50 dark:bg-success-900/20 px-3 py-2 rounded-lg">
              Знайдено: {barcodeProduct.name} — {formatCurrency(parseFloat(barcodeProduct.price))}
            </div>
          )}
        </div>

        {/* Search results */}
        <div className="flex-1 overflow-y-auto space-y-2">
          {searchResults.map((product: any) => (
            <button
              key={product.id}
              onClick={() => handleProductSelect(product)}
              className="w-full card p-3 text-left hover:border-primary-300 dark:hover:border-primary-600 transition-all group"
            >
              <p className="font-medium text-sm text-gray-900 dark:text-gray-100 group-hover:text-primary-600">
                {product.name}
              </p>
              <div className="flex items-center justify-between mt-1">
                <span className="text-xs text-gray-400">
                  {product.barcode || 'Без ШК'} | {product.stock} {formatUnit(product.unit)}
                </span>
                <span className="font-bold text-primary-600">
                  {formatCurrency(product.price)}
                </span>
              </div>
            </button>
          ))}
          {searchQuery.length >= 2 && searchResults.length === 0 && (
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

        <div className="flex-1 overflow-y-auto">
          {cart.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-full text-gray-400">
              <ShoppingCart className="w-16 h-16 mb-3 opacity-30" />
              <p className="text-sm">Кошик порожній</p>
              <p className="text-xs mt-1">Знайдіть товар або відскануйте штрих-код</p>
            </div>
          ) : (
            <div className="divide-y divide-gray-100 dark:divide-slate-700">
              {cart.map((item) => (
                <div key={item.product_id} className="p-4 hover:bg-gray-50 dark:hover:bg-slate-700/30">
                  <div className="flex items-start justify-between">
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                        {item.product_name}
                      </p>
                      <p className="text-xs text-gray-400">
                        {formatCurrency(item.price)} / шт
                      </p>
                    </div>
                    <button
                      onClick={() => removeFromCart(item.product_id)}
                      className="p-1 text-gray-300 hover:text-danger-500 transition-colors"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                  <div className="flex items-center justify-between mt-2">
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => updateQuantity(item.product_id, -1)}
                        className="w-7 h-7 rounded-lg border border-gray-200 dark:border-slate-600 flex items-center justify-center text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700"
                      >
                        <Minus className="w-3 h-3" />
                      </button>
                      <input
                        type="number"
                        value={item.quantity}
                        onChange={(e) =>
                          setItemQuantity(item.product_id, parseFloat(e.target.value) || 0)
                        }
                        className="w-16 text-center input-field text-sm py-1"
                        min="0"
                        step={item.is_weight ? '0.1' : '1'}
                        id={`cart-qty-${item.product_id}`}
                        name={`cart-qty-${item.product_id}`}
                      />
                      <button
                        onClick={() => updateQuantity(item.product_id, 1)}
                        className="w-7 h-7 rounded-lg border border-gray-200 dark:border-slate-600 flex items-center justify-center text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700"
                      >
                        <Plus className="w-3 h-3" />
                      </button>
                    </div>
                    <span className="font-semibold text-gray-900 dark:text-gray-100">
                      {formatCurrency(item.quantity * item.price)}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Cart summary */}
        {cart.length > 0 && (
          <div className="border-t border-gray-200 dark:border-slate-700 p-4 space-y-2">
            <div className="flex justify-between text-sm text-gray-500">
              <span>ПДВ</span>
              <span>{formatCurrency(vatAmount)}</span>
            </div>
            <div className="flex justify-between text-lg font-bold text-gray-900 dark:text-gray-100">
              <span>До сплати</span>
              <span className="text-primary-600">{formatCurrency(subtotal)}</span>
            </div>
            <Button
              className="w-full mt-2"
              size="lg"
              onClick={() => setShowPayment(true)}
            >
              ОПЛАТА
            </Button>
          </div>
        )}
      </div>

      {/* Quantity modal for weight products */}
      <Modal
        isOpen={showQuantityModal}
        onClose={() => {
          setShowQuantityModal(false);
          setSelectedProduct(null);
        }}
        title="Введіть кількість"
        size="sm"
      >
        <div className="space-y-4">
          {selectedProduct && (
            <p className="text-sm text-gray-600 dark:text-gray-400">
              {selectedProduct.name}
            </p>
          )}
          <Input
            type="number"
            step="0.1"
            min="0.001"
            value={quantityInput}
            onChange={(e) => setQuantityInput(e.target.value)}
            placeholder="Кількість"
            autoFocus
            id="weight-quantity"
            name="weight-quantity"
          />
          <div className="flex justify-end gap-3">
            <Button
              variant="secondary"
              onClick={() => {
                setShowQuantityModal(false);
                setSelectedProduct(null);
              }}
            >
              Скасувати
            </Button>
            <Button onClick={handleQuantityConfirm}>Додати</Button>
          </div>
        </div>
      </Modal>

      {/* Payment modal */}
      <Modal
        isOpen={showPayment}
        onClose={() => setShowPayment(false)}
        title="Оплата"
        size="lg"
      >
        <div className="space-y-6">
          <div className="text-center">
            <p className="text-3xl font-bold text-gray-900 dark:text-gray-100">
              {formatCurrency(subtotal)}
            </p>
            <p className="text-sm text-gray-500">Сума до сплати</p>
          </div>

          <Select
            label="Метод оплати"
            options={[
              { value: 'cash', label: 'Готівка' },
              { value: 'card', label: 'Картка' },
              { value: 'mixed', label: 'Змішаний' },
            ]}
            value={paymentMethod}
            onChange={(e) => setPaymentMethod(e.target.value as PaymentMethod)}
          />

          {paymentMethod === 'cash' && (
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
          )}

          {paymentMethod === 'mixed' && (
            <div className="grid grid-cols-2 gap-4">
              <Input
                label="Готівка"
                type="number"
                step="0.01"
                min="0"
                value={cashAmount}
                onChange={(e) => setCashAmount(e.target.value)}
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
                onChange={(e) => setCardAmount(e.target.value)}
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

          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={isDebt}
              onChange={(e) => setIsDebt(e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
              id="is-debt"
              name="is-debt"
            />
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Оплата в борг
            </span>
          </label>

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => setShowPayment(false)}>
              Скасувати
            </Button>
            <Button
              onClick={handlePayment}
              isLoading={isProcessing}
              size="lg"
            >
              {isDebt ? 'Оформити борг' : 'Сплатити'}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};

export default PosPage;
