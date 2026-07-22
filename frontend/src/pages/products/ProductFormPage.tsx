import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useProduct, useCreateProduct, useUpdateProduct } from '@/hooks/useProducts';
import { useCategories } from '@/hooks/useCategories';
import { useSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Save, Percent, DollarSign, Plus, Hash } from 'lucide-react';
import { ProductCreate, VatRate, UnitOfMeasure } from '@/types/product';

import { useBackNavigation } from '@/hooks/useBackNavigation';
const taxRateOptions = [
  { value: 0, label: '0%' },
  { value: 5, label: '5%' },
  { value: 7, label: '7%' },
  { value: 20, label: '20%' },
];

const unitOptions = [
  { value: 'pcs', label: 'шт' },
  { value: 'kg', label: 'кг' },
  { value: 'l', label: 'л' },
  { value: 'm', label: 'м' },
  { value: 'box', label: 'кор' },
  { value: 'pack', label: 'уп' },
];

interface FormState {
  title: string;
  barcode: string;
  sku: string;
  uktzed: string;
  price: number;
  cost_price: number | null;
  markup: number | null;
  stock: number;
  category_id: string | null;
  supplier_id: string | null;
  tax_rate: VatRate;
  unit: UnitOfMeasure;
  is_weight: boolean;
  scan_excise: boolean;
}

/**
 * Розраховує ціну на основі собівартості та націнки.
 * Формула: Ціна = Собівартість × (1 + Націнка/100)
 *
 * Ціна округлюється до гривні (0 знаків після коми).
 */
function calcPriceFromCostAndMarkup(cost: number | null, markup: number | null): number {
  if (cost === null || cost <= 0 || markup === null || markup <= 0) return 0;
  return Math.round(cost * (1 + markup / 100));
}

/**
 * Розраховує націнку на основі собівартості та ціни.
 * Формула: Націнка = (Ціна / Собівартість - 1) × 100
 *
 * Націнка округлюється до сотих (2 знаки після коми).
 */
function calcMarkupFromCostAndPrice(cost: number | null, price: number): number | null {
  if (cost === null || cost <= 0 || price <= 0) return null;
  const markup = (price / cost - 1) * 100;
  return Math.round(markup * 100) / 100;
}

const ProductFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;

  const { data: product, isLoading: isLoadingProduct } = useProduct(id || '');
  const { data: categories } = useCategories();
  const { data: suppliersData } = useSuppliers({ page: 1, size: 100 });
  const createMutation = useCreateProduct();
  const updateMutation = useUpdateProduct();

  const [form, setForm] = useState<FormState>({
    title: '',
    barcode: '',
    sku: '',
    uktzed: '',
    price: 0,
    cost_price: null,
    markup: null,
    stock: 0,
    category_id: null,
    supplier_id: null,
    tax_rate: 0,
    unit: 'pcs',
    is_weight: false,
    scan_excise: false,
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (isEdit && product) {
      setForm({
        title: product.title,
        barcode: product.barcode || '',
        sku: product.sku || '',
        uktzed: product.uktzed || '',
        price: Math.round(parseFloat(product.price)),
        cost_price: product.cost_price ? parseFloat(product.cost_price) : null,
        markup: product.markup ? parseFloat(product.markup) : null,
        stock: parseFloat(product.stock),
        category_id: product.category_id,
        supplier_id: product.supplier_id,
        tax_rate: (parseFloat(product.tax_rate) || 0) as VatRate,
        unit: (product.unit === 'шт' ? 'pcs' : product.unit === 'кг' ? 'kg' : product.unit === 'л' ? 'l' : product.unit) as UnitOfMeasure,
        is_weight: product.is_weight,
        scan_excise: product.scan_excise,
      });
    }
  }, [isEdit, product]);

  // ─── Зміна собівартості → перераховує ціну (якщо є націнка) ───
  const handleCostPriceChange = (value: number | null) => {
    setForm((prev) => {
      const newForm = { ...prev, cost_price: value };
      if (value !== null && value > 0 && newForm.markup !== null && newForm.markup > 0) {
        newForm.price = calcPriceFromCostAndMarkup(value, newForm.markup);
      }
      return newForm;
    });
  };

  // ─── Зміна націнки → перераховує ціну (якщо є собівартість) ───
  const handleMarkupChange = (value: number | null) => {
    setForm((prev) => {
      const newForm = { ...prev, markup: value };
      if (value !== null && value > 0 && newForm.cost_price !== null && newForm.cost_price > 0) {
        newForm.price = calcPriceFromCostAndMarkup(newForm.cost_price, value);
      }
      return newForm;
    });
  };

  // ─── Зміна ціни → перераховує націнку (якщо є собівартість) ───
  const handlePriceChange = (value: number) => {
    setForm((prev) => {
      const newForm = { ...prev, price: value };
      if (value > 0 && newForm.cost_price !== null && newForm.cost_price > 0) {
        const newMarkup = calcMarkupFromCostAndPrice(newForm.cost_price, value);
        if (newMarkup !== null) {
          newForm.markup = newMarkup;
        }
      }
      return newForm;
    });
  };

  // ─── Кнопка +20%: множить собівартість на 1.2 ───
  const handleAddTwentyPercent = () => {
    setForm((prev) => {
      const cost = prev.cost_price ?? 0;
      const newCost = Math.round(cost * 1.2 * 100) / 100;
      const newForm = { ...prev, cost_price: newCost };
      if (newCost > 0 && newForm.markup !== null && newForm.markup > 0) {
        newForm.price = calcPriceFromCostAndMarkup(newCost, newForm.markup);
      }
      return newForm;
    });
  };

  const validate = (): boolean => {
    const newErrors: Record<string, string> = {};
    if (!form.title.trim()) newErrors.title = "Назва обов'язкова";
    if (form.price < 0) newErrors.price = "Ціна не може бути від'ємною";
    if (form.stock < 0) newErrors.stock = "Залишок не може бути від'ємним";
    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!validate()) return;

    const data: ProductCreate = {
      title: form.title,
      barcode: form.barcode || undefined,
      sku: form.sku || undefined,
      uktzed: form.uktzed || undefined,
      price: form.price,
      cost_price: form.cost_price ?? undefined,
      markup: form.markup ?? undefined,
      stock: form.stock,
      category_id: form.category_id,
      supplier_id: form.supplier_id,
      tax_rate: form.tax_rate,
      unit: form.unit,
      is_weight: form.is_weight,
      scan_excise: form.scan_excise,
    };

    try {
      if (isEdit && id) {
        await updateMutation.mutateAsync({
          id,
          data: { ...data, id },
        });
      } else {
        await createMutation.mutateAsync(data);
      }
      navigate('/products');
    } catch {
      // Error handled by mutation
    }
  };

  const handleChange = (field: keyof FormState, value: any) => {
    setForm((prev) => {
      const newForm = { ...prev, [field]: value };

      // Якщо встановили галочку "Ваговий товар" → одиниця виміру = кг
      if (field === 'is_weight') {
        if (value === true) {
          newForm.unit = 'kg';
        } else {
          // Якщо зняли галочку → одиниця виміру = шт
          newForm.unit = 'pcs';
        }
      }

      return newForm;
    });

    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  if (isEdit && isLoadingProduct) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="lg" />
      </div>
    );
  }

  const categoryOptions = [
    { value: '', label: 'Без категорії' },
    ...(categories?.map((cat) => ({
      value: String(cat.id),
      label: cat.name,
    })) || []),
  ];

  const supplierOptions = [
    { value: '', label: 'Без постачальника' },
    ...(suppliersData?.items?.map((sup) => ({
      value: String(sup.id),
      label: sup.name,
    })) || []),
  ];

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            {isEdit ? 'Редагувати товар' : 'Новий товар'}
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            {isEdit ? 'Змініть дані товару' : 'Заповніть інформацію про товар'}
          </p>
        </div>
      </div>

      <form onSubmit={handleSubmit} className="card p-6 space-y-5">
        {/* Основна інформація */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Input
            label="Назва товару *"
            value={form.title}
            onChange={(e) => handleChange('title', e.target.value)}
            error={errors.title}
            placeholder="Введіть назву"
          />
          <Input
            label="Штрих-код"
            value={form.barcode || ''}
            onChange={(e) => handleChange('barcode', e.target.value)}
            placeholder="13 цифр"
          />
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
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
            onChange={(e) =>
              handleChange('category_id', e.target.value || null)
            }
          />
        </div>

        {/* ═══════════════════════════════════════════════════════════════
           ЦІНИ ТА ФІНАНСИ — три взаємопов'язані поля
           ═══════════════════════════════════════════════════════════════ */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Ціни та фінанси
          </h3>

          <div className="mb-3 p-3 bg-blue-50 dark:bg-blue-900/20 rounded-lg text-xs text-blue-700 dark:text-blue-300">
            <p className="font-medium mb-1">Автоматичний розрахунок:</p>
            <p>Заповніть будь-які <strong>2 поля</strong> — третє розрахується автоматично.</p>
            <p className="mt-1">
              <strong>Ціна = Собівартість × (1 + Націнка/100)</strong>
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {/* Собівартість */}
            <div>
              <Input
                label="Собівартість"
                type="number"
                step="0.01"
                min="0"
                value={form.cost_price ?? ''}
                onChange={(e) =>
                  handleCostPriceChange(
                    e.target.value ? parseFloat(e.target.value) : null
                  )
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

            {/* Націнка (%) — з кроком 0.01 (соті) */}
            <Input
              label="Націнка (%)"
              type="number"
              step="0.01"
              min="0"
              value={form.markup ?? ''}
              onChange={(e) =>
                handleMarkupChange(
                  e.target.value ? parseFloat(e.target.value) : null
                )
              }
              icon={<Percent className="w-4 h-4 text-gray-400" />}
            />

            {/* Ціна продажу — з кроком 1 (гривні) */}
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

          {form.cost_price !== null && form.cost_price > 0 && form.markup !== null && form.markup > 0 && form.price > 0 && (
            <div className="mt-2 text-xs text-gray-500 dark:text-gray-400 space-y-1">
              <p>
                <span className="text-primary-500">●</span> Собівартість: <strong>{form.cost_price.toFixed(2)} грн</strong>
              </p>
              <p>
                <span className="text-success-500">●</span> Націнка: <strong>{form.markup.toFixed(2)}%</strong>
              </p>
              <p>
                <span className="text-warning-500">●</span> Ціна продажу: <strong>{Math.round(form.price)} грн</strong>
              </p>
              <p className="text-gray-400 italic">
                Змініть будь-яке поле — інші перерахуються автоматично
              </p>
            </div>
          )}
        </div>

        {/* Облік */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Облік
          </h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <Input
              label="Початковий залишок"
              type="number"
              min="0"
              value={form.stock}
              onChange={(e) => handleChange('stock', parseInt(e.target.value) || 0)}
              error={errors.stock}
            />
            <Select
              label="Постачальник"
              options={supplierOptions}
              value={String(form.supplier_id || '')}
              onChange={(e) =>
                handleChange('supplier_id', e.target.value || null)
              }
            />
          </div>
        </div>

        {/* Податки, УКТЗЕД та одиниці виміру */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Податки та одиниці виміру
          </h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <Select
              label="Ставка податків"
              options={taxRateOptions}
              value={form.tax_rate}
              onChange={(e) => handleChange('tax_rate', Number(e.target.value) as VatRate)}
            />
            <Select
              label="Одиниця виміру"
              options={unitOptions}
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

        {/* Додаткові опції */}
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

        {/* Кнопки */}
        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button
            type="button"
            variant="secondary"
            onClick={goBack}
          >
            Скасувати
          </Button>
          <Button
            type="submit"
            icon={<Save className="w-4 h-4" />}
            isLoading={createMutation.isPending || updateMutation.isPending}
          >
            {isEdit ? 'Зберегти зміни' : 'Створити товар'}
          </Button>
        </div>
      </form>
    </div>
  );
};

export default ProductFormPage;
