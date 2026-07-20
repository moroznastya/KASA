import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useSupplier, useCreateSupplier, useUpdateSupplier } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Save } from 'lucide-react';
import { SupplierCreate } from '@/types/supplier';

export const SupplierFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;

  const { data: supplier, isLoading: isLoadingSupplier } = useSupplier(Number(id));
  const createMutation = useCreateSupplier();
  const updateMutation = useUpdateSupplier();

  const [form, setForm] = useState<SupplierCreate>({
    name: '',
    code: '',
    contact_person: '',
    phone: '',
    email: '',
    address: '',
    edrpou: '',
    is_active: true,
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (isEdit && supplier) {
      setForm({
        name: supplier.name,
        code: supplier.code,
        contact_person: supplier.contact_person || '',
        phone: supplier.phone || '',
        email: supplier.email || '',
        address: supplier.address || '',
        edrpou: supplier.edrpou || '',
        is_active: supplier.is_active,
      });
    }
  }, [isEdit, supplier]);

  const validate = (): boolean => {
    const newErrors: Record<string, string> = {};
    if (!form.name.trim()) newErrors.name = 'Назва обов\'язкова';
    if (!form.code.trim()) newErrors.code = 'Код обов\'язковий';
    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!validate()) return;

    try {
      if (isEdit && id) {
        await updateMutation.mutateAsync({
          id: Number(id),
          data: { ...form, id: Number(id) },
        });
      } else {
        await createMutation.mutateAsync(form);
      }
      navigate('/suppliers');
    } catch {
      // Error handled
    }
  };

  const handleChange = (field: keyof SupplierCreate, value: any) => {
    setForm((prev) => ({ ...prev, [field]: value }));
    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  if (isEdit && isLoadingSupplier) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="lg" />
      </div>
    );
  }

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button
          onClick={() => navigate('/suppliers')}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            {isEdit ? 'Редагувати постачальника' : 'Новий постачальник'}
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            {isEdit ? 'Змініть дані постачальника' : 'Заповніть інформацію про постачальника'}
          </p>
        </div>
      </div>

      <form onSubmit={handleSubmit} className="card p-6 space-y-5">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Input
            label="Назва *"
            value={form.name}
            onChange={(e) => handleChange('name', e.target.value)}
            error={errors.name}
            placeholder="Назва постачальника"
          />
          <Input
            label="Код *"
            value={form.code}
            onChange={(e) => handleChange('code', e.target.value)}
            error={errors.code}
            placeholder="Унікальний код"
          />
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Input
            label="Контактна особа"
            value={form.contact_person || ''}
            onChange={(e) => handleChange('contact_person', e.target.value)}
            placeholder="ПІБ контактної особи"
          />
          <Input
            label="Телефон"
            value={form.phone || ''}
            onChange={(e) => handleChange('phone', e.target.value)}
            placeholder="+380501234567"
          />
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Input
            label="Email"
            type="email"
            value={form.email || ''}
            onChange={(e) => handleChange('email', e.target.value)}
            placeholder="email@example.com"
          />
          <Input
            label="ЄДРПОУ"
            value={form.edrpou || ''}
            onChange={(e) => handleChange('edrpou', e.target.value)}
            placeholder="8 цифр"
          />
        </div>

        <Input
          label="Адреса"
          value={form.address || ''}
          onChange={(e) => handleChange('address', e.target.value)}
          placeholder="Юридична адреса"
        />

        <label className="flex items-center gap-3 cursor-pointer">
          <input
            type="checkbox"
            checked={form.is_active}
            onChange={(e) => handleChange('is_active', e.target.checked)}
            className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
          />
          <span className="text-sm text-gray-700 dark:text-gray-300">Активний постачальник</span>
        </label>

        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => navigate('/suppliers')}>
            Скасувати
          </Button>
          <Button
            type="submit"
            icon={<Save className="w-4 h-4" />}
            isLoading={createMutation.isPending || updateMutation.isPending}
          >
            {isEdit ? 'Зберегти зміни' : 'Створити постачальника'}
          </Button>
        </div>
      </form>
    </div>
  );
};
