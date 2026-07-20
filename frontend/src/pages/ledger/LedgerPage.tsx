import React, { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { BookOpen, DollarSign, Banknote, CreditCard } from 'lucide-react';
import { ledgerService } from '@/services/ledgerService';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { Select } from '@/components/ui/Select';
import { Input } from '@/components/ui/Input';
import { Modal } from '@/components/ui/Modal';
import { Badge } from '@/components/ui/Badge';
import { formatCurrency, formatDateTime, formatPaymentMethod } from '@/utils/format';
import { SupplierLedgerEntry, Payment, PaymentMethod } from '@/types/ledger';
import toast from 'react-hot-toast';

export const LedgerPage: React.FC = () => {
  const queryClient = useQueryClient();
  const { data: suppliersData } = useAllSuppliers();
  const [selectedSupplierId, setSelectedSupplierId] = useState<number | null>(null);
  const [showPaymentModal, setShowPaymentModal] = useState(false);
  const [paymentForm, setPaymentForm] = useState({
    amount: '',
    payment_method: 'cash' as PaymentMethod,
    notes: '',
  });

  const { data: balance } = useQuery({
    queryKey: ['supplier-balance', selectedSupplierId],
    queryFn: () => ledgerService.getSupplierBalance(selectedSupplierId!),
    enabled: !!selectedSupplierId,
  });

  const { data: ledgerData, isLoading: isLedgerLoading } = useQuery({
    queryKey: ['supplier-ledger', selectedSupplierId],
    queryFn: () => ledgerService.getSupplierLedger(selectedSupplierId!, { page: 1, size: 50 }),
    enabled: !!selectedSupplierId,
  });

  const paymentMutation = useMutation({
    mutationFn: () =>
      ledgerService.createPayment({
        supplier_id: selectedSupplierId!,
        amount: parseFloat(paymentForm.amount),
        payment_method: paymentForm.payment_method,
        notes: paymentForm.notes || undefined,
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['supplier-balance'] });
      queryClient.invalidateQueries({ queryKey: ['supplier-ledger'] });
      toast.success('Платіж створено');
      setShowPaymentModal(false);
      setPaymentForm({ amount: '', payment_method: 'cash', notes: '' });
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка створення платежу');
    },
  });

  const supplierOptions = [
    { value: '', label: 'Виберіть постачальника' },
    ...(suppliersData?.map((s) => ({
      value: String(s.id),
      label: s.name,
    })) || []),
  ];

  const ledgerColumns: Column<SupplierLedgerEntry>[] = [
    {
      key: 'created_at',
      header: 'Дата',
      render: (item) => (
        <span className="text-gray-500">{formatDateTime(item.created_at)}</span>
      ),
    },
    {
      key: 'document_number',
      header: 'Документ',
      render: (item) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">
          {item.document_number}
        </span>
      ),
    },
    {
      key: 'description',
      header: 'Опис',
      render: (item) => item.description || '-',
    },
    {
      key: 'debit',
      header: 'Дебет',
      render: (item) =>
        parseFloat(item.debit) > 0 ? (
          <span className="font-medium text-danger-600">{formatCurrency(item.debit)}</span>
        ) : (
          <span className="text-gray-400">-</span>
        ),
    },
    {
      key: 'credit',
      header: 'Кредит',
      render: (item) =>
        parseFloat(item.credit) > 0 ? (
          <span className="font-medium text-success-600">{formatCurrency(item.credit)}</span>
        ) : (
          <span className="text-gray-400">-</span>
        ),
    },
    {
      key: 'balance',
      header: 'Баланс',
      render: (item) => (
        <span
          className={`font-medium ${
            parseFloat(item.balance) > 0
              ? 'text-danger-600'
              : parseFloat(item.balance) < 0
              ? 'text-success-600'
              : ''
          }`}
        >
          {formatCurrency(item.balance)}
        </span>
      ),
    },
  ];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Взаєморозрахунки
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Баланс та історія оплат постачальникам
          </p>
        </div>
      </div>

      {/* Supplier selector */}
      <div className="card p-5">
        <div className="flex items-end gap-4">
          <div className="flex-1">
            <Select
              label="Постачальник"
              options={supplierOptions}
              value={String(selectedSupplierId || '')}
              onChange={(e) =>
                setSelectedSupplierId(e.target.value ? Number(e.target.value) : null)
              }
            />
          </div>
          {selectedSupplierId && (
            <Button
              onClick={() => setShowPaymentModal(true)}
              icon={<DollarSign className="w-4 h-4" />}
            >
              Створити платіж
            </Button>
          )}
        </div>

        {balance && (
          <div className="mt-4 p-4 bg-gray-50 dark:bg-slate-700/50 rounded-xl">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  Баланс постачальника
                </p>
                <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">
                  {formatCurrency(balance.balance)}
                </p>
              </div>
              <div className="p-3 rounded-xl bg-primary-50 dark:bg-primary-900/20 text-primary-600">
                <BookOpen className="w-6 h-6" />
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Ledger table */}
      {selectedSupplierId && (
        <div className="card">
          <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Історія операцій
            </h3>
          </div>
          <Table
            columns={ledgerColumns}
            data={ledgerData?.items || []}
            isLoading={isLedgerLoading}
            keyExtractor={(item) => item.id}
            emptyMessage="Немає операцій"
            emptyIcon={<BookOpen className="w-12 h-12" />}
          />
        </div>
      )}

      {/* Payment modal */}
      <Modal
        isOpen={showPaymentModal}
        onClose={() => {
          setShowPaymentModal(false);
          setPaymentForm({ amount: '', payment_method: 'cash', notes: '' });
        }}
        title="Створити платіж"
        size="sm"
      >
        <div className="space-y-4">
          <Input
            label="Сума *"
            type="number"
            step="0.01"
            min="0.01"
            value={paymentForm.amount}
            onChange={(e) =>
              setPaymentForm((prev) => ({ ...prev, amount: e.target.value }))
            }
            placeholder="0.00"
          />
          <Select
            label="Метод оплати"
            options={[
              { value: 'cash', label: 'Готівка' },
              { value: 'card', label: 'Картка' },
              { value: 'bank_transfer', label: 'Банківський переказ' },
            ]}
            value={paymentForm.payment_method}
            onChange={(e) =>
              setPaymentForm((prev) => ({
                ...prev,
                payment_method: e.target.value as PaymentMethod,
              }))
            }
          />
          <Input
            label="Примітки"
            value={paymentForm.notes}
            onChange={(e) =>
              setPaymentForm((prev) => ({ ...prev, notes: e.target.value }))
            }
            placeholder="Додаткова інформація"
          />
          <div className="flex justify-end gap-3 pt-2">
            <Button
              variant="secondary"
              onClick={() => {
                setShowPaymentModal(false);
                setPaymentForm({ amount: '', payment_method: 'cash', notes: '' });
              }}
            >
              Скасувати
            </Button>
            <Button
              onClick={() => paymentMutation.mutate()}
              isLoading={paymentMutation.isPending}
              disabled={!paymentForm.amount || parseFloat(paymentForm.amount) <= 0}
            >
              Створити платіж
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};
