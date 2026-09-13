import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useProduct, useCreateProduct, useUpdateProduct } from '@/hooks/useProducts';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Save, Plus, Image as ImageIcon, X, Barcode as BarcodeIcon, Trash2 } from 'lucide-react';
import { ProductCreate, VatRate, UnitOfMeasure } from '@/types/product';
import { productService } from '@/services/productService';
import { ProductFormFields } from '@/components/products/ProductFormFields';
import { ProductFormState, EMPTY_PRODUCT_FORM } from '@/utils/productOptions';

import { useBackNavigation } from '@/hooks/useBackNavigation';
const ProductFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;

  const { data: product, isLoading: isLoadingProduct } = useProduct(id || '');
  const createMutation = useCreateProduct();
  const updateMutation = useUpdateProduct();

  const [form, setForm] = useState<ProductFormState>({ ...EMPTY_PRODUCT_FORM });

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

  /**
   * Точкове оновлення стану форми. Логіка зв'язки
   * «собівартість ↔ націнка ↔ ціна» живе всередині ProductFormFields —
   * тут лише застосування патча та зняття помилок зі змінених полів.
   */
  const handlePatch = (patch: Partial<ProductFormState>) => {
    setForm((prev) => ({ ...prev, ...patch }));
    setErrors((prevErrors) => {
      const next = { ...prevErrors };
      let changed = false;
      for (const key of Object.keys(patch)) {
        if (next[key]) {
          delete next[key];
          changed = true;
        }
      }
      return changed ? next : prevErrors;
    });
  };


  if (isEdit && isLoadingProduct) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="lg" />
      </div>
    );
  }


  // Перше фото товару (для прев'ю)
  const mainImage = product?.images && product.images.length > 0
    ? product.images.find(img => img.is_main) || product.images[0]
    : null;

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button aria-label="Назад"
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
                          } catch { /* ігноруємо: операція не критична */ }
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
                          } catch { /* ігноруємо: операція не критична */ }
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
                          } catch { /* ігноруємо: операція не критична */ }
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
        </div>

        <ProductFormFields
          form={form}
          onPatch={handlePatch}
          errors={errors}
          stockReadOnly
          autoFocusTitle={!isEdit}
        />


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
                          } catch { /* ігноруємо: операція не критична */ }
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
