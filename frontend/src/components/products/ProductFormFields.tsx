import React from 'react';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { useCategoryTree } from '@/hooks/useCategories';
import { useSuppliers } from '@/hooks/useSuppliers';
import { Percent, DollarSign, Plus, Hash } from 'lucide-react';
import { VatRate, UnitOfMeasure } from '@/types/product';
import {
  ProductFormState,
  TAX_RATE_OPTIONS,
  UNIT_OPTIONS,
  buildCategorySelectOptions,
  buildSupplierSelectOptions,
  calcPriceFromCostAndMarkup,
  calcMarkupFromCostAndPrice,
} from '@/utils/productOptions';

/**
 * Повний набір полів картки товару — ЄДИНЕ джерело розмітки та зв'язки
 * «собівартість ↔ націнка ↔ ціна».
 *
 * Використовується у:
 *  - ProductFormPage (/products/new, /products/:id)
 *  - модалках створення товару (інвентаризація, накладна)
 *
 * Компонент контрольований: батько тримає ProductFormState і отримує
 * часткові оновлення через onPatch.
 */
interface ProductFormFieldsProps {
  form: ProductFormState;
  onPatch: (patch: Partial<ProductFormState>) => void;
  errors?: Record<string, string>;
  /** Кількість лише для читання (у картці товару змінюється документами). */
  stockReadOnly?: boolean;
  stockHelperText?: string;
  /** Показати поле «Рекомендований залишок» (за замовчуванням — так). */
  showRecommendedQty?: boolean;
  autoFocusTitle?: boolean;
}

export const ProductFormFields: React.FC<ProductFormFieldsProps> = ({
  form,
  onPatch,
  errors = {},
  stockReadOnly = true,
  stockHelperText = 'Облікова кількість. Змінюється через накладні, списання та інвентаризацію',
  showRecommendedQty = true,
  autoFocusTitle = false,
}) => {
  const { data: categoryTree } = useCategoryTree();
  const { data: suppliersData } = useSuppliers({ page: 1, size: 100 });

  const categoryOptions = buildCategorySelectOptions(categoryTree);
  const supplierOptions = buildSupplierSelectOptions(suppliersData?.items);

  // ─── Собівартість → перераховує ціну (якщо є націнка) ───
  const handleCostPriceChange = (value: number | null) => {
    const patch: Partial<ProductFormState> = { cost_price: value };
    if (value !== null && value > 0 && form.markup !== null && form.markup > 0) {
      patch.price = calcPriceFromCostAndMarkup(value, form.markup);
    }
    onPatch(patch);
  };

  // ─── Націнка → перераховує ціну (якщо є собівартість) ───
  const handleMarkupChange = (value: number | null) => {
    const patch: Partial<ProductFormState> = { markup: value };
    if (value !== null && value > 0 && form.cost_price !== null && form.cost_price > 0) {
      patch.price = calcPriceFromCostAndMarkup(form.cost_price, value);
    }
    onPatch(patch);
  };

  // ─── Ціна → перераховує націнку (якщо є собівартість) ───
  const handlePriceChange = (value: number) => {
    const patch: Partial<ProductFormState> = { price: value };
    if (value > 0 && form.cost_price !== null && form.cost_price > 0) {
      const newMarkup = calcMarkupFromCostAndPrice(form.cost_price, value);
      if (newMarkup !== null) patch.markup = newMarkup;
    }
    onPatch(patch);
  };

  // ─── Кнопка +20% (ПДВ): множить собівартість на 1.2 ───
  const handleAddTwentyPercent = () => {
    const cost = form.cost_price ?? 0;
    const newCost = Math.round(cost * 1.2 * 100) / 100;
    const patch: Partial<ProductFormState> = { cost_price: newCost };
    if (newCost > 0 && form.markup !== null && form.markup > 0) {
      patch.price = calcPriceFromCostAndMarkup(newCost, form.markup);
    }
    onPatch(patch);
  };

  // ─── Загальна зміна поля (+ зв'язка «ваговий товар» → одиниця) ───
  const handleChange = (field: keyof ProductFormState, value: unknown) => {
    const patch: Partial<ProductFormState> = { [field]: value } as Partial<ProductFormState>;
    if (field === 'is_weight') {
      patch.unit = value === true ? 'kg' : 'pcs';
    }
    onPatch(patch);
  };

  return (
    <>
      {/* ═══ Основна інформація ═══════════════════════════════════════ */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Input
          label="Назва товару *"
          value={form.title}
          onChange={(e) => handleChange('title', e.target.value)}
          error={errors.title}
          placeholder="Введіть назву"
          autoFocus={autoFocusTitle}
        />
        <Input
          label="Штрих-код"
          value={form.barcode || ''}
          onChange={(e) => handleChange('barcode', e.target.value)}
          placeholder="13 цифр"
        />
        <Input
          label="Артикул"
          value={form.sku || ''}
          onChange={(e) => handleChange('sku', e.target.value)}
          placeholder="Артикул товару"
        />
        <Select
          label="Категорія"
          options={categoryOptions}
          value={String(form.category_id || '')}
          onChange={(e) => handleChange('category_id', e.target.value || null)}
        />
      </div>

      {/* ═══ Ціни та фінанси ══════════════════════════════════════════ */}
      <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
        <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
          Ціни та фінанси
        </h3>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div>
            <Input
              label="Собівартість"
              type="number"
              step="0.01"
              min="0"
              value={form.cost_price ?? ''}
              onChange={(e) =>
                handleCostPriceChange(e.target.value ? parseFloat(e.target.value) : null)
              }
              icon={<DollarSign className="w-4 h-4 text-gray-400" />}
            />
            <button
              type="button"
              onClick={handleAddTwentyPercent}
              className="mt-1.5 inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-md
                bg-green-50 text-green-700 hover:bg-green-100
                dark:bg-green-900/20 dark:text-green-400 dark:hover:bg-green-900/30
                transition-colors"
            >
              <Plus className="w-3 h-3" />
              +20% (ПДВ)
            </button>
          </div>

          <Input
            label="Націнка (%)"
            type="number"
            step="0.01"
            min="0"
            value={form.markup ?? ''}
            onChange={(e) =>
              handleMarkupChange(e.target.value ? parseFloat(e.target.value) : null)
            }
            icon={<Percent className="w-4 h-4 text-gray-400" />}
          />

          <Input
            label="Ціна продажу"
            type="number"
            step="1"
            min="0"
            value={form.price}
            onChange={(e) => handlePriceChange(parseFloat(e.target.value) || 0)}
            error={errors.price}
          />
        </div>
      </div>

      {/* ═══ Облік ════════════════════════════════════════════════════ */}
      <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
        <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
          Облік
        </h3>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <Input
            label="Кількість"
            type="number"
            step="0.001"
            min="0"
            value={form.stock}
            disabled={stockReadOnly}
            onChange={(e) => handleChange('stock', parseFloat(e.target.value) || 0)}
            error={errors.stock}
            helperText={stockHelperText}
          />
          {showRecommendedQty && (
            <Input
              label="Рекомендований залишок"
              type="number"
              min="0"
              value={form.recommended_qty}
              onChange={(e) => handleChange('recommended_qty', parseInt(e.target.value) || 0)}
              error={errors.recommended_qty}
              helperText="Мінімальний залишок для замовлення"
            />
          )}
          <Select
            label="Постачальник"
            options={supplierOptions}
            value={String(form.supplier_id || '')}
            onChange={(e) => handleChange('supplier_id', e.target.value || null)}
          />
        </div>
      </div>

      {/* ═══ Податки та одиниці виміру ════════════════════════════════ */}
      <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
        <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
          Податки та одиниці виміру
        </h3>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Select
            label="Ставка податків"
            options={TAX_RATE_OPTIONS}
            value={form.tax_rate}
            onChange={(e) => handleChange('tax_rate', Number(e.target.value) as VatRate)}
          />
          <Select
            label="Одиниця виміру"
            options={UNIT_OPTIONS}
            value={form.unit}
            onChange={(e) => handleChange('unit', e.target.value as UnitOfMeasure)}
          />
        </div>
        <div className="mt-4">
          <Input
            label="Код УКТЗЕД"
            value={form.uktzed}
            onChange={(e) => handleChange('uktzed', e.target.value)}
            placeholder="10 цифр"
            icon={<Hash className="w-4 h-4 text-gray-400" />}
            helperText="Український класифікатор товарів зовнішньоекономічної діяльності"
          />
        </div>
      </div>

      {/* ═══ Додаткові опції ══════════════════════════════════════════ */}
      <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
        <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
          Додаткові опції
        </h3>
        <div className="space-y-3">
          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={form.is_weight}
              onChange={(e) => handleChange('is_weight', e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
            />
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Ваговий товар (продаж за вагою)
            </span>
            {form.is_weight ? (
              <span className="text-xs text-amber-500">→ одиницю виміру змінено на кг</span>
            ) : (
              <span className="text-xs text-gray-400">→ одиниця виміру: шт</span>
            )}
          </label>
          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={form.scan_excise}
              onChange={(e) => handleChange('scan_excise', e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
            />
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Сканувати акцизну марку
            </span>
          </label>
        </div>
      </div>
    </>
  );
};
