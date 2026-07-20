import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useProduct, useCreateProduct, useUpdateProduct } from '@/hooks/useProducts';
import { useCategories } from '@/hooks/useCategories';
import { useSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Save } from 'lucide-react';
import { ProductCreate, VatRate, UnitOfMeasure } from '@/types/product';

const vatRateOptions = [
  { value: 0, label: '0%' },
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
  name: string;
  barcode: string;
  article: string;
  price: number;
  cost_price: number | null;
  stock: number;
  category_id: string | null;
  supplier_id: string | null;
  vat_rate: VatRate;
  unit: UnitOfMeasure;
  is_weight: boolean;
  is_excise: boolean;
  is_active: boolean;
}

const ProductFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;

  const { data: product, isLoading: isLoadingProduct } = useProduct(id || '');
  const { data: categories } = useCategories();
  const { data: suppliersData } = useSuppliers({ page: 1, size: 100 });
  const createMutation = useCreateProduct();
  const updateMutation = useUpdateProduct();

  const [form, setForm] = useState<FormState>({
    name: '',
    barcode: '',
    article: '',
    price: 0,
    cost_price: null,
    stock: 0,
    category_id: null,
    supplier_id: null,
    vat_rate: 20,
    unit: 'pcs',
    is_weight: false,
    is_excise: false,
    is_active: true,
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (isEdit && product) {
      setForm({
        name: product.name,
        barcode: product.barcode || '',
        article: product.article || '',
        price: parseFloat(product.price),
        cost_price: product.cost_price ? parseFloat(product.cost_price) : null,
        stock: product.stock,
        category_id: product.category_id,
        supplier_id: product.supplier_id,
        vat_rate: product.vat_rate,
        unit: product.unit,
        is_weight: product.is_weight,
        is_excise: product.is_excise,
        is_active: product.is_active,
      });
    }
  }, [isEdit, product]);

  const validate = (): boolean => {
    const newErrors: Record<string, string> = {};
    if (!form.name.trim()) newErrors.name = "Назва обов'язкова";
    if (form.price < 0) newErrors.price = "Ціна не може бути від'ємною";
    if (form.stock < 0) newErrors.stock = "Залишок не може бути від'ємним";
    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!validate()) return;

    const data: ProductCreate = {
      name: form.name,
      barcode: form.barcode || undefined,
      article: form.article || undefined,
      price: form.price,
      cost_price: form.cost_price,
      stock: form.stock,
      category_id: form.category_id,
      supplier_id: form.supplier_id,
      vat_rate: form.vat_rate,
      unit: form.unit,
      is_weight: form.is_weight,
      is_excise: form.is_excise,
      is_active: form.is_active,
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
    setForm((prev) => ({ ...prev, [field]: value }));
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
          onClick={() => navigate('/products')}
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
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Input
            label="Назва товару *"
            value={form.name}
            onChange={(e) => handleChange('name', e.target.value)}
            error={errors.name}
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
            value={form.article || ''}
            onChange={(e) => handleChange('article', e.target.value)}
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

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <Input
            label="Ціна продажу *"
            type="number"
            step="0.01"
            min="0"
            value={form.price}
            onChange={(e) => handleChange('price', parseFloat(e.target.value) || 0)}
            error={errors.price}
          />
          <Input
            label="Собівартість"
            type="number"
            step="0.01"
            min="0"
            value={form.cost_price ?? ''}
            onChange={(e) =>
              handleChange(
                'cost_price',
                e.target.value ? parseFloat(e.target.value) : null
              )
            }
          />
          <Input
            label="Початковий залишок"
            type="number"
            min="0"
            value={form.stock}
            onChange={(e) => handleChange('stock', parseInt(e.target.value) || 0)}
            error={errors.stock}
          />
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Select
            label="ПДВ"
            options={vatRateOptions}
            value={form.vat_rate}
            onChange={(e) => handleChange('vat_rate', Number(e.target.value) as VatRate)}
          />
          <Select
            label="Одиниця виміру"
            options={unitOptions}
            value={form.unit}
            onChange={(e) => handleChange('unit', e.target.value as UnitOfMeasure)}
          />
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Select
            label="Постачальник"
            options={supplierOptions}
            value={String(form.supplier_id || '')}
            onChange={(e) =>
              handleChange('supplier_id', e.target.value || null)
            }
          />
        </div>

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
          </label>
          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={form.is_excise}
              onChange={(e) => handleChange('is_excise', e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
            />
            <span className="text-sm text-gray-700 dark:text-gray-300">
              Сканувати акцизну марку
            </span>
          </label>
          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={form.is_active}
              onChange={(e) => handleChange('is_active', e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
            />
            <span className="text-sm text-gray-700 dark:text-gray-300">Активний товар</span>
          </label>
        </div>

        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button
            type="button"
            variant="secondary"
            onClick={() => navigate('/products')}
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
