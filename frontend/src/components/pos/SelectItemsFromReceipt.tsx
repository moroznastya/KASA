import React, { useState, useEffect, useCallback, useMemo } from 'react';
import {Loader2, ShoppingCart, CheckSquare, Square, Minus, Plus} from 'lucide-react';
import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { receiptService } from '@/services/receiptService';
import { formatCurrency } from '@/utils/format';
import type { ReceiptSearchResult, ReceiptItem } from '@/types/receipt';
import { toast } from 'react-hot-toast';

// ── Інтерфейси ────────────────────────────────
export interface ReturnCartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  image_url: string | null;
  quantity: number;
  price: number;      // ціна з оригінального чеку
  original_receipt_id: string;
  tax_rate: number;
  unit: string;
}

interface SelectItemsFromReceiptProps {
  isOpen: boolean;
  onClose: () => void;
  receipt: ReceiptSearchResult;
  onProcessReturn: (items: ReturnCartItem[]) => void;
}

// ── Розширений інтерфейс товару з чекбоксом та кількістю
interface SelectableItem {
  item: ReceiptItem;
  checked: boolean;
  returnQuantity: number;
}

// ── Компонент ─────────────────────────────────
const SelectItemsFromReceipt: React.FC<SelectItemsFromReceiptProps> = ({
  isOpen,
  onClose,
  receipt,
  onProcessReturn,
}) => {
  const [, setItems] = useState<ReceiptItem[]>([]);
  const [selectableItems, setSelectableItems] = useState<SelectableItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Завантажуємо товари з чеку при відкритті
  useEffect(() => {
    if (!isOpen) return;

    const fetchItems = async () => {
      setIsLoading(true);
      setError(null);
      try {
        const receiptItems = await receiptService.getReceiptItems(receipt.id);
        setItems(receiptItems);
        // Ініціалізуємо вибір: жоден товар не вибрано, кількість = кількість в чеку
        setSelectableItems(
          receiptItems.map((item) => ({
            item,
            checked: false,
            returnQuantity: item.quantity,
          }))
        );
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Помилка завантаження товарів');
        setSelectableItems([]);
      } finally {
        setIsLoading(false);
      }
    };

    fetchItems();
  }, [isOpen, receipt.id]);

  // Скидаємо при закритті
  useEffect(() => {
    if (!isOpen) {
      setItems([]);
      setSelectableItems([]);
      setError(null);
    }
  }, [isOpen]);

  // Перемикання чекбокса
  const toggleItem = useCallback((index: number) => {
    setSelectableItems((prev) =>
      prev.map((si, i) =>
        i === index ? { ...si, checked: !si.checked } : si
      )
    );
  }, []);

  // Зміна кількості повернення
  const updateReturnQuantity = useCallback((index: number, value: number) => {
    setSelectableItems((prev) => {
      const item = prev[index];
      const maxQty = item.item.quantity;
      const clamped = Math.max(0, Math.min(maxQty, Math.floor(value)));
      return prev.map((si, i) =>
        i === index ? { ...si, returnQuantity: clamped } : si
      );
    });
  }, []);

  // Зміна кількості через інкремент/декремент
  const adjustQuantity = useCallback(
    (index: number, delta: number) => {
      setSelectableItems((prev) => {
        const si = prev[index];
        const newQty = si.returnQuantity + delta;
        const clamped = Math.max(0, Math.min(si.item.quantity, newQty));
        return prev.map((p, i) =>
          i === index ? { ...p, returnQuantity: clamped } : p
        );
      });
    },
    []
  );

  // Вибрані товари
  const selectedItems = useMemo(
    () => selectableItems.filter((si) => si.checked && si.returnQuantity > 0),
    [selectableItems]
  );

  // Загальна сума вибраних товарів
  const totalAmount = useMemo(
    () => selectedItems.reduce((sum, si) => sum + si.returnQuantity * Number(si.item.price), 0),
    [selectedItems]
  );

  // Валідація та додавання до кошика
  const handleProcessReturn = useCallback(() => {
    if (selectedItems.length === 0) {
      toast.error('Виберіть хоча б один товар для повернення');
      return;
    }

    // Перевіряємо що всі кількості не перевищують максимум
    const invalidItems = selectedItems.filter(
      (si) => si.returnQuantity > si.item.quantity
    );
    if (invalidItems.length > 0) {
      toast.error(
        `Кількість повернення не може перевищувати кількість в чеку: ${invalidItems
          .map((si) => si.item.product_name)
          .join(', ')}`
      );
      return;
    }

    const returnItems: ReturnCartItem[] = selectedItems.map((si) => ({
      product_id: si.item.product_id,
      product_title: si.item.product_name,
      product_barcode: si.item.product_barcode,
      image_url: null, // фото не зберігаємо в чеку
      quantity: si.returnQuantity,
      price: Number(si.item.price),
      original_receipt_id: receipt.id,
      tax_rate: si.item.vat_rate,
      unit: 'шт',
    }));

    onProcessReturn(returnItems);
  }, [selectedItems, receipt.id, onProcessReturn]);

  // Перевірка чи всі кількості коректні
  const hasInvalidQuantity = useMemo(
    () => selectableItems.some((si) => si.checked && si.returnQuantity > si.item.quantity),
    [selectableItems]
  );

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={`📋 Вибір товарів з чеку №${receipt.receipt_number}`}
      size="3xl"
    >
      <div className="space-y-4">
        {/* Інформація про чек */}
        <div className="flex items-center justify-between p-3 rounded-lg bg-gray-50 dark:bg-slate-700/50 text-sm">
          <div>
            <span className="text-gray-500 dark:text-gray-400">Чек №{receipt.receipt_number}</span>
            <span className="mx-2 text-gray-300 dark:text-slate-600">|</span>
            <span className="text-gray-500 dark:text-gray-400">
              {new Date(receipt.created_at).toLocaleDateString('uk-UA')}
            </span>
          </div>
          <div className="text-right">
            <span className="font-semibold text-gray-900 dark:text-gray-100">
              {formatCurrency(receipt.total_amount)}
            </span>
            <span className="ml-2 text-xs text-gray-500">({receipt.items_count} поз.)</span>
          </div>
        </div>

        {/* Помилка */}
        {error && (
          <div className="p-3 rounded-lg bg-danger-50 dark:bg-danger-900/20 border border-danger-200 dark:border-danger-800 text-sm text-danger-700 dark:text-danger-300">
            {error}
          </div>
        )}

        {/* Індикатор завантаження */}
        {isLoading && (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
            <span className="ml-3 text-sm text-gray-500">Завантаження товарів...</span>
          </div>
        )}

        {/* Список товарів */}
        {!isLoading && selectableItems.length > 0 && (
          <div className="space-y-2 max-h-96 overflow-y-auto">
            <div className="grid grid-cols-12 gap-2 px-1 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              <div className="col-span-1" />
              <div className="col-span-4">Товар</div>
              <div className="col-span-2 text-right">Ціна</div>
              <div className="col-span-2 text-right">В чеку</div>
              <div className="col-span-3 text-right">Повернути</div>
            </div>

            {selectableItems.map((si, index) => (
              <div
                key={si.item.id}
                className={`grid grid-cols-12 gap-2 items-center p-3 rounded-lg border transition-colors ${
                  si.checked
                    ? 'border-primary-200 dark:border-primary-700 bg-primary-50/50 dark:bg-primary-900/10'
                    : 'border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800'
                }`}
              >
                {/* Чекбокс */}
                <div className="col-span-1 flex justify-center">
                  <button
                    onClick={() => toggleItem(index)}
                    className="text-gray-400 hover:text-primary-600 dark:hover:text-primary-400 transition-colors"
                    title={si.checked ? 'Прибрати з повернення' : 'Додати до повернення'}
                  >
                    {si.checked ? (
                      <CheckSquare className="w-5 h-5 text-primary-600" />
                    ) : (
                      <Square className="w-5 h-5" />
                    )}
                  </button>
                </div>

                {/* Назва та штрих-код */}
                <div className="col-span-4 min-w-0">
                  <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                    {si.item.product_name}
                  </p>
                  {si.item.product_barcode && (
                    <p className="text-xs text-gray-400 truncate">
                      {si.item.product_barcode}
                    </p>
                  )}
                </div>

                {/* Ціна */}
                <div className="col-span-2 text-right">
                  <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                    {formatCurrency(Number(si.item.price))}
                  </span>
                </div>

                {/* Кількість в чеку */}
                <div className="col-span-2 text-right">
                  <Badge variant="default" size="sm">
                    {si.item.quantity}
                  </Badge>
                </div>

                {/* Кількість для повернення */}
                <div className="col-span-3 flex items-center justify-end gap-1">
                  <button
                    onClick={() => adjustQuantity(index, -1)}
                    disabled={!si.checked || si.returnQuantity <= 0}
                    className="p-1 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                  >
                    <Minus className="w-3.5 h-3.5" />
                  </button>
                  <input
                    type="number"
                    min={0}
                    max={si.item.quantity}
                    value={si.returnQuantity}
                    onChange={(e) => updateReturnQuantity(index, parseInt(e.target.value) || 0)}
                    disabled={!si.checked}
                    className={`w-14 text-center text-sm font-medium rounded-md border ${
                      si.returnQuantity > si.item.quantity
                        ? 'border-danger-400 text-danger-600'
                        : 'border-gray-300 dark:border-slate-600 text-gray-900 dark:text-gray-100'
                    } bg-white dark:bg-slate-700 py-1 disabled:opacity-50`}
                  />
                  <button
                    onClick={() => adjustQuantity(index, 1)}
                    disabled={!si.checked || si.returnQuantity >= si.item.quantity}
                    className="p-1 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                  >
                    <Plus className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Порожній стан */}
        {!isLoading && !error && selectableItems.length === 0 && (
          <div className="text-center py-12 text-gray-400">
            <ShoppingCart className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p className="text-sm">Немає товарів для відображення</p>
          </div>
        )}

        {/* Підсумок та кнопка додавання */}
        {!isLoading && selectableItems.length > 0 && (
          <div className="flex items-center justify-between pt-4 border-t border-gray-200 dark:border-slate-700">
            <div>
              <span className="text-sm text-gray-500 dark:text-gray-400">
                Вибрано: <strong className="text-gray-900 dark:text-gray-100">{selectedItems.length}</strong> товарів
              </span>
              <span className="mx-2 text-gray-300 dark:text-slate-600">|</span>
              <span className="text-sm text-gray-500 dark:text-gray-400">
                Сума повернення:{' '}
                <strong className="text-danger-600 dark:text-danger-400">
                  {formatCurrency(totalAmount)}
                </strong>
              </span>
            </div>
            <div className="flex gap-2">
              <Button variant="secondary" onClick={onClose}>
                Скасувати
              </Button>
              <Button
                onClick={handleProcessReturn}
                disabled={selectedItems.length === 0 || hasInvalidQuantity}
                icon={<ShoppingCart className="w-4 h-4" />}
              >
                Оформити повернення
              </Button>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
};

export default SelectItemsFromReceipt;
