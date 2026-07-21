import React, { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { BookOpen, DollarSign, ArrowUpRight, ArrowDownLeft } from 'lucide-react';
import { ledgerService } from '@/services/ledgerService';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { Select } from '@/components/ui/Select';
import { Input } from '@/components/ui/Input';
import { Modal } from '@/components/ui/Modal';
import { Badge } from '@/components/ui/Badge';
import { formatCurrency, formatDateTime } from '@/utils/format';
import { SupplierLedgerEntry, Payment, PaymentMethod } from '@/types/ledger';
import toast from 'react-hot-toast';

const OPERATION_TYPE_LABELS: Record<string, string> = {
  invoice: 'Прибуткова накладна',
  payment: 'Оплата',
  return: 'Повернення',
  correction: 'Коригування',
};

const OPERATION_TYPE_VARIANTS: Record<string, 'info' | 'success' | 'danger' | 'warning'> = {
  invoice: 'info',
  payment: 'success',
  return: 'danger',
  correction: 'warning',
};

const LedgerPage: React.FC = () => {
  const queryClient = useQueryClient();
  const { data: suppliersData } = useAllSuppliers();
  const [selectedSupplierId, setSelectedSupplierId] = useState<string | null>(null);
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
      key: 'operation_date',
      header: 'Дата',
      render: (item) => (
        <span className="text-gray-500 text-sm">
          {formatDateTime(item.operation_date)}
        </span>
      ),
    },
    {
      key: 'operation_type',
      header: 'Тип',
      render: (item) => (
        <Badge variant={OPERATION_TYPE_VARIANTS[item.operation_type] || 'default'}>
          {OPERATION_TYPE_LABELS[item.operation_type] || item.operation_type}
        </Badge>
      ),
    },
    {
      key: 'document_number',
      header: 'Документ',
      render: (item) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">
          {item.document_number || '-'}
        </span>
      ),
    },
    {
      key: 'notes',
      header: 'Опис',
      render: (item) => (
        <span className="text-gray-600 dark:text-gray-400 text-sm">
          {item.notes || '-'}
        </span>
      ),
    },
    {
      key: 'amount',
      header: 'Сума',
      render: (item) => {
        const amount = parseFloat(item.amount);
        const isPositive = amount > 0;
        const isNegative = amount < 0;
        return (
          <div className="flex items-center gap-1.5">
            {isPositive && <ArrowUpRight className="w-4 h-4 text-danger-500" />}
            {isNegative && <ArrowDownLeft className="w-4 h-4 text-success-500" />}
            <span
              className={`font-medium ${
                isPositive
                  ? 'text-danger-600'
                  : isNegative
                  ? 'text-success-600'
                  : 'text-gray-500'
              }`}
            >
              {isPositive ? '+' : ''}
              {formatCurrency(item.amount)}
            </span>
          </div>
        );
      },
    },
    {
      key: 'balance_after',
      header: 'Баланс',
      render: (item) => {
        const balance = parseFloat(item.balance_after);
        return (
          <span
            className={`font-semibold ${
              balance > 0
                ? 'text-danger-600'
                : balance < 0
                ? 'text-success-600'
                : 'text-gray-500'
            }`}
          >
            {formatCurrency(item.balance_after)}
          </span>
        );
      },
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
            Баланс та історія операцій з постачальниками
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
                setSelectedSupplierId(e.target.value || null)
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
                  {parseFloat(balance.current_balance) > 0
                    ? 'Борг перед постачальником'
                    : parseFloat(balance.current_balance) < 0
                    ? 'Переплата постачальнику'
                    : 'Баланс постачальника'}
                </p>
                <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">
                  {formatCurrency(balance.current_balance)}
                </p>
                {balance.last_updated && (
                  <p className="text-xs text-gray-400 mt-1">
                    Остання операція: {formatDateTime(balance.last_updated)}
                  </p>
                )}
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

export default LedgerPage;
