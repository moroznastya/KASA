import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useProduct, useCreateProduct, useUpdateProduct } from '@/hooks/useProducts';
import { useCategoryTree } from '@/hooks/useCategories';
import { useSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Save, Percent, DollarSign, Plus, Hash, Image as ImageIcon, X, Barcode as BarcodeIcon, Trash2 } from 'lucide-react';
import { ProductCreate, VatRate, UnitOfMeasure, Category } from '@/types/product';
import { productService } from '@/services/productService';

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
  recommended_qty: number;
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

/**
 * Рекурсивно будує список SelectOption для випадаючого списку категорій.
 * Основні категорії — жирним шрифтом, не вибираються (disabled).
 * Підкатегорії — з відступом, вибираються.
 * Тільки кінцеві підкатегорії (без дітей) можна вибрати.
 */
function buildCategoryOptions(categories: Category[], depth: number = 0): SelectOption[] {
  const options: SelectOption[] = [];

  for (const cat of categories) {
    const hasChildren = cat.children && cat.children.length > 0;

    if (hasChildren) {
      // Основна категорія — не вибирається, показуємо як заголовок
      options.push({
        value: cat.id,
        label: `${'  '.repeat(depth)}▶ ${cat.name}`,
        disabled: true,
      });
      // Додаємо підкатегорії
      options.push(...buildCategoryOptions(cat.children!, depth + 1));
    } else {
      // Кінцева підкатегорія — вибирається
      options.push({
        value: cat.id,
        label: `${'  '.repeat(depth)}└── ${cat.name}`,
        disabled: false,
      });
    }
  }

  return options;
}

const ProductFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;

  const { data: product, isLoading: isLoadingProduct } = useProduct(id || '');
  const { data: categoryTree } = useCategoryTree();
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
    recommended_qty: 0,
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
        recommended_qty: parseFloat(product.recommended_qty || '0'),
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
    if (form.recommended_qty < 0) newErrors.recommended_qty = "Рекомендований залишок не може бути від'ємною";
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
      recommended_qty: form.recommended_qty,
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

  // Будуємо ієрархічний список категорій
  const categoryOptions: SelectOption[] = [
    { value: '', label: 'Без категорії' },
    ...(categoryTree ? buildCategoryOptions(categoryTree) : []),
  ];

  const supplierOptions = [
    { value: '', label: 'Без постачальника' },
    ...(suppliersData?.items?.map((sup) => ({
      value: String(sup.id),
      label: sup.name,
    })) || []),
  ];

  // Перше фото товару (для прев'ю)
  const mainImage = product?.images && product.images.length > 0
    ? product.images.find(img => img.is_main) || product.images[0]
    : null;

  return (
    <div className="max-w-4xl mx-auto space-y-6">
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
        {/* ═══════════════════════════════════════════════════════════════
           Фото товару та Основна інформація — в один рядок
           ═══════════════════════════════════════════════════════════════ */}
        <div className="flex gap-6">
          {/* Фото товару — ліворуч */}
          <div className="flex-shrink-0">
            {isEdit ? (
              <div className="relative group">
                {mainImage ? (
                  <div className="relative">
                    <img
                      src={mainImage.url}
                      alt="Фото товару"
                      className="w-32 h-32 object-cover rounded-xl border border-gray-200 dark:border-slate-600"
                    />
                    <button
                      type="button"
                      onClick={async () => {
                        if (confirm('Видалити фото?')) {
                          try {
                            await productService.deleteImage(id!, mainImage.id);
                            window.location.reload();
                          } catch {}
                        }
                      }}
                      className="absolute -top-2 -right-2 p-1 bg-red-500 text-white rounded-full opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </div>
                ) : (
                  <label className="flex flex-col items-center justify-center w-32 h-32 rounded-xl border-2 border-dashed border-gray-300 dark:border-slate-600 bg-gray-50 dark:bg-slate-800/50 cursor-pointer hover:border-primary-400 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors">
                    <ImageIcon className="w-8 h-8 text-gray-400" />
                    <span className="mt-1 text-xs text-gray-400">Фото</span>
                    <input
                      type="file"
                      accept="image/*"
                      className="hidden"
                      onChange={async (e) => {
                        const file = e.target.files?.[0];
                        if (file && id) {
                          try {
                            await productService.uploadImage(id, file, true);
                            window.location.reload();
                          } catch {}
                        }
                      }}
                    />
                  </label>
                )}
                {/* Кнопка замінити/додати */}
                {mainImage && (
                  <label className="absolute bottom-1 right-1 p-1.5 bg-gray-900/70 text-white rounded-lg opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer">
                    <ImageIcon className="w-4 h-4" />
                    <input
                      type="file"
                      accept="image/*"
                      className="hidden"
                      onChange={async (e) => {
                        const file = e.target.files?.[0];
                        if (file && id) {
                          try {
                            await productService.uploadImage(id, file);
                            window.location.reload();
                          } catch {}
                        }
                      }}
                    />
                  </label>
                )}
              </div>
            ) : (
              <div className="w-32 h-32 rounded-xl border-2 border-dashed border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800/30 flex flex-col items-center justify-center">
                <ImageIcon className="w-8 h-8 text-gray-300 dark:text-gray-600" />
                <span className="mt-1 text-xs text-gray-300 dark:text-gray-600">Фото</span>
              </div>
            )}
          </div>

          {/* Поля інформації — праворуч */}
          <div className="flex-1 space-y-4">
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
          </div>
        </div>

        {/* ═══════════════════════════════════════════════════════════════
           ЦІНИ ТА ФІНАНСИ — три взаємопов'язані поля
           ═══════════════════════════════════════════════════════════════ */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Ціни та фінанси
          </h3>

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
        </div>

        {/* Облік */}
        <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Облік
          </h3>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <Input
              label="Кількість"
              type="number"
              min="0"
              value={form.stock}
              onChange={(e) => handleChange('stock', parseInt(e.target.value) || 0)}
              error={errors.stock}
            />
            <Input
              label="Рекомендований залишок"
              type="number"
              min="0"
              value={form.recommended_qty}
              onChange={(e) => handleChange('recommended_qty', parseInt(e.target.value) || 0)}
              error={errors.recommended_qty}
              helperText="Мінімальний залишок для замовлення"
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

        {/* Додаткові коди */}
        {isEdit && (
          <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
            <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
              Додаткові коди
            </h3>
            {/* Список додаткових кодів */}
            {product?.barcodes && product.barcodes.length > 0 && (
              <div className="space-y-2 mb-3">
                {product.barcodes.map((bc) => (
                  <div key={bc.id} className="flex items-center justify-between px-3 py-2 bg-gray-50 dark:bg-slate-800/50 rounded-lg">
                    <div className="flex items-center gap-2">
                      <BarcodeIcon className="w-4 h-4 text-gray-400" />
                      <span className="text-sm font-mono text-gray-900 dark:text-gray-100">{bc.barcode}</span>
                      {bc.is_primary && (
                        <span className="px-1.5 py-0.5 bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300 text-[10px] rounded font-medium">
                          Основний
                        </span>
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={async () => {
                        if (confirm('Видалити штрих-код?')) {
                          try {
                            await productService.deleteBarcode(id!, bc.id);
                            window.location.reload();
                          } catch {}
                        }
                      }}
                      className="p-1 text-gray-400 hover:text-red-500 transition-colors"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                ))}
              </div>
            )}
            {/* Додавання нового коду */}
            <div className="flex gap-2">
              <input
                type="text"
                placeholder="Введіть додатковий штрих-код"
                className="flex-1 px-3 py-2 text-sm border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-primary-500 focus:border-transparent outline-none"
                id="new-barcode-input"
              />
              <Button
                type="button"
                variant="secondary"
                icon={<Plus className="w-4 h-4" />}
                onClick={async () => {
                  const input = document.getElementById('new-barcode-input') as HTMLInputElement;
                  const barcode = input?.value?.trim();
                  if (barcode && id) {
                    try {
                      await productService.addBarcode(id, barcode);
                      input.value = '';
                      window.location.reload();
                    } catch (err: any) {
                      alert(err?.response?.data?.detail || 'Помилка при додаванні коду');
                    }
                  }
                }}
              >
                Додати
              </Button>
            </div>
          </div>
        )}

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
