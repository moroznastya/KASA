import React, { useState, useEffect, useRef } from 'react';
import { X, Plus, ImageOff, ShoppingCart, Scale } from 'lucide-react';
import { Modal } from '@/components/ui/Modal';
import { useDevicesStore } from '@/store/devicesStore';
import { Input } from '@/components/ui/Input';
import { Button } from '@/components/ui/Button';
import { formatCurrency, formatUnit } from '@/utils/format';

interface ProductCardModalProps {
  isOpen: boolean;
  onClose: () => void;
  product: {
    id: string;
    title: string;
    price: number | string;
    unit: string;
    stock: number | string;
    images?: Array<{ id: string; url: string; is_main: boolean }>;
    is_weight?: boolean;
    barcode?: string | null;
  } | null;
  onAdd: (product: any, quantity: number) => void;
}

const ProductCardModal: React.FC<ProductCardModalProps> = ({
  isOpen,
  onClose,
  product,
  onAdd,
}) => {
  const [quantity, setQuantity] = useState('1');
  const [quantityError, setQuantityError] = useState<string | null>(null);
  const [imageError, setImageError] = useState(false);
  const [isReadingWeight, setIsReadingWeight] = useState(false);
  const quantityInputRef = useRef<HTMLInputElement>(null);
  // Реальна вага з підключених пристроїв (оновлюється через подію "weight-updated",
  // на яку PosPage підписується через useDevicesStore.initListeners)
  const weights = useDevicesStore((s) => s.weights);

  // Скидаємо стан при відкритті
  useEffect(() => {
    if (isOpen) {
      setQuantity('1');
      setQuantityError(null);
      setImageError(false);
      setIsReadingWeight(false);
      // Фокус на поле кількості після відкриття
      setTimeout(() => {
        quantityInputRef.current?.focus();
        quantityInputRef.current?.select();
      }, 100);
    }
  }, [isOpen, product?.id]);

  if (!product) return null;

  const stock = parseFloat(String(product.stock)) || 0;
  const price = parseFloat(String(product.price)) || 0;
  const isOutOfStock = stock <= 0;

  // Знаходимо головне зображення
  const mainImage = product.images?.find((img) => img.is_main) || product.images?.[0];

  const handleAdd = () => {
    const qty = parseFloat(quantity);
    if (isNaN(qty) || qty <= 0) {
      setQuantityError('Введіть коректну кількість');
      return;
    }
    if (qty > stock) {
      setQuantityError(`Доступно лише ${stock} ${formatUnit(product.unit)}`);
      return;
    }
    onAdd(product, qty);
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleAdd();
    }
    if (e.key === 'Escape') {
      onClose();
    }
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="" size="xl">
      <div className="space-y-5" onKeyDown={handleKeyDown}>
        {/* Фото товару */}
        <div className="w-full h-56 rounded-xl overflow-hidden bg-gray-100 dark:bg-slate-700 border border-gray-200 dark:border-slate-600 flex items-center justify-center">
          {mainImage && !imageError ? (
            <img
              src={mainImage.url}
              alt={product.title}
              className="w-full h-full object-contain"
              onError={() => setImageError(true)}
            />
          ) : (
            <div className="flex flex-col items-center gap-3 text-gray-400">
              <ImageOff className="w-16 h-16" />
              <span className="text-base">Немає фото</span>
            </div>
          )}
        </div>

        {/* Назва товару */}
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            {product.title}
          </h2>
          {product.barcode && (
            <p className="text-sm text-gray-400 mt-1">
              ШК: {product.barcode}
            </p>
          )}
        </div>

        {/* Ціна та одиниця виміру */}
        <div className="flex items-center justify-between p-4 bg-primary-50 dark:bg-primary-900/20 rounded-xl">
          <div>
            <p className="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              Ціна продажу
            </p>
            <p className="text-3xl font-bold text-primary-600">
              {formatCurrency(price)}
            </p>
          </div>
          <div className="text-right">
            <p className="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              Одиниця виміру
            </p>
            <p className="text-xl font-semibold text-gray-700 dark:text-gray-300">
              {formatUnit(product.unit)}
            </p>
          </div>
        </div>

        {/* Доступний залишок */}
        <div className="flex justify-between items-center text-sm">
          <span className="text-gray-500 dark:text-gray-400">Доступно:</span>
          <span className={`font-semibold ${isOutOfStock ? 'text-danger-500' : 'text-gray-700 dark:text-gray-300'}`}>
            {isOutOfStock ? 'Немає в наявності' : `${stock} ${formatUnit(product.unit)}`}
          </span>
        </div>

        {/* Кількість */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Кількість
          </label>
          <div className="flex items-center gap-3">
            <button
              onClick={() => {
                const val = parseFloat(quantity) || 0;
                const newVal = Math.max(0.01, val - (product.is_weight ? 0.001 : 1));
                setQuantity(newVal.toFixed(product.is_weight ? 3 : 0));
                setQuantityError(null);
              }}
              className="w-12 h-12 rounded-xl flex items-center justify-center text-2xl font-bold
                text-white bg-red-500 hover:bg-red-600 transition-colors shadow-sm"
            >
              &minus;
            </button>
            <input
              ref={quantityInputRef}
              type="number"
              value={quantity}
              onChange={(e) => {
                setQuantity(e.target.value);
                setQuantityError(null);
              }}
              className="flex-1 h-12 text-center input-field text-xl font-bold no-spinner"
              min="0.01"
              max={stock || undefined}
              step={product.is_weight ? '0.001' : '1'}
              id="product-card-quantity"
              name="product-card-quantity"
              autoFocus
            />
            <button
              onClick={() => {
                const val = parseFloat(quantity) || 0;
                const newVal = val + (product.is_weight ? 0.001 : 1);
                if (newVal > stock) {
                  setQuantityError(`Максимум ${stock} ${formatUnit(product.unit)}`);
                  return;
                }
                setQuantity(newVal.toFixed(product.is_weight ? 3 : 0));
                setQuantityError(null);
              }}
              disabled={stock > 0 && (parseFloat(quantity) || 0) >= stock}
              className={`
                w-12 h-12 rounded-xl flex items-center justify-center text-2xl font-bold transition-colors shadow-sm
                ${stock > 0 && (parseFloat(quantity) || 0) >= stock
                  ? 'bg-gray-300 dark:bg-gray-600 text-gray-500 cursor-not-allowed'
                  : 'text-white bg-green-500 hover:bg-green-600 dark:bg-green-600 dark:hover:bg-green-700'
                }
              `}
            >
              +
            </button>
          </div>
          {quantityError && (
            <p className="mt-2 text-sm text-danger-500 bg-danger-50 dark:bg-danger-900/20 px-4 py-2 rounded-lg">
              {quantityError}
            </p>
          )}
          {product.is_weight && (
            <button
              onClick={async () => {
                setIsReadingWeight(true);
                try {
                  // Реальна вага: перше значення з weights (перший scale-пристрій)
                  const weightValues = Object.values(weights);
                  const weight = weightValues.length > 0 ? weightValues[0] : undefined;
                  if (weight !== undefined && weight > 0) {
                    setQuantity(String(weight));
                    setQuantityError(null);
                  } else {
                    setQuantityError('Ваги не підключені або не передають дані. Перевірте Налаштування → Пристрої.');
                  }
                } finally {
                  // Коротка затримка для UX (індикатор «Зчитування...»)
                  setTimeout(() => setIsReadingWeight(false), 300);
                }
              }}
              disabled={isReadingWeight}
              className="mt-3 w-full flex items-center justify-center gap-2 px-4 py-3 rounded-xl
                font-semibold text-sm transition-all duration-200 shadow-sm
                bg-primary-500 hover:bg-primary-600 text-white
                disabled:opacity-50 disabled:cursor-wait"
            >
              <Scale className={`w-5 h-5 ${isReadingWeight ? 'animate-spin' : ''}`} />
              {isReadingWeight ? 'Зчитування...' : 'Зчитати вагу'}
            </button>
          )}
        </div>

        {/* Сума */}
        <div className="flex justify-between items-center p-4 bg-gray-50 dark:bg-slate-700/50 rounded-xl">
          <span className="text-base font-medium text-gray-600 dark:text-gray-400">
            Сума:
          </span>
          <span className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            {formatCurrency((parseFloat(quantity) || 0) * price)}
          </span>
        </div>

        {/* Кнопки */}
        <div className="flex gap-3 pt-1">
          <Button
            variant="secondary"
            onClick={onClose}
            className="flex-1"
            size="lg"
          >
            <X className="w-5 h-5" />
            Закрити
          </Button>
          <Button
            onClick={handleAdd}
            disabled={isOutOfStock}
            className="flex-1"
            size="lg"
          >
            <ShoppingCart className="w-5 h-5" />
            Додати товар
          </Button>
        </div>
      </div>
    </Modal>
  );
};

export default ProductCardModal;
