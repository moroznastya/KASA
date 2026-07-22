import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useSupplier, useCreateSupplier, useUpdateSupplier } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Save } from 'lucide-react';
import { SupplierCreate, SupplierUpdate } from '@/types/supplier';

import { useBackNavigation } from '@/hooks/useBackNavigation';
const SupplierFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const { id } = useParams<{ id: string }>();
  const isEdit = !!id;

  const { data: supplier, isLoading: isLoadingSupplier } = useSupplier(id || '');
  const createMutation = useCreateSupplier();
  const updateMutation = useUpdateSupplier();

  const [name, setName] = useState('');
  const [edrpou, setEdrpou] = useState('');
  const [phone, setPhone] = useState('');
  const [email, setEmail] = useState('');
  const [address, setAddress] = useState('');
  const [notes, setNotes] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    if (isEdit && supplier) {
      setName(supplier.name || '');
      setEdrpou(supplier.edrpou || '');
      setPhone(supplier.phone || '');
      setEmail(supplier.email || '');
      setAddress(supplier.address || '');
      setNotes(supplier.notes || '');
    }
  }, [isEdit, supplier]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Назва обов\'язкова');
      return;
    }

    const data: SupplierCreate = {
      name: trimmed,
      edrpou: edrpou.trim() || null,
      phone: phone.trim() || null,
      email: email.trim() || null,
      address: address.trim() || null,
      notes: notes.trim() || null,
    };

    try {
      if (isEdit && id) {
        await updateMutation.mutateAsync({ id, data: data as SupplierUpdate });
      } else {
        await createMutation.mutateAsync(data);
      }
      navigate('/suppliers');
    } catch {
      // Error handled by hook
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
    <div className="max-w-xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            {isEdit ? 'Редагувати постачальника' : 'Новий постачальник'}
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            {isEdit ? 'Змініть дані постачальника' : 'Заповніть дані постачальника'}
          </p>
        </div>
      </div>

      <form onSubmit={handleSubmit} className="card p-6 space-y-5">
        <Input
          label="Назва *"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            if (error) setError('');
          }}
          error={error}
          placeholder="Введіть назву постачальника"
          autoFocus
        />

        <Input
          label="ЄДРПОУ"
          value={edrpou}
          onChange={(e) => setEdrpou(e.target.value)}
          placeholder="Код ЄДРПОУ"
        />

        <Input
          label="Номер телефону"
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
          placeholder="+380 (__) ___ __ __"
        />

        <Input
          label="Email"
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="example@mail.com"
        />

        <Input
          label="Адреса"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          placeholder="Адреса постачальника"
        />

        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
            Реквізити / Примітки
          </label>
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder="Банківські реквізити, примітки..."
            rows={4}
            className="w-full rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 px-3 py-2 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-colors resize-none"
          />
        </div>

        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={goBack}>
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

export default SupplierFormPage;
