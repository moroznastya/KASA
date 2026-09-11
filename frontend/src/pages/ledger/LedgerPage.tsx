import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { BookOpen, DollarSign, ArrowUpRight, ArrowDownLeft, FileText } from 'lucide-react';
import { ledgerService } from '@/services/ledgerService';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { Select, SelectOption } from '@/components/ui/Select';
import { Input } from '@/components/ui/Input';
import { Modal } from '@/components/ui/Modal';
import { Badge } from '@/components/ui/Badge';
import { formatCurrency, formatDateTime } from '@/utils/format';
import {SupplierLedgerEntry, PaymentMethod, InvoicePaymentInfo} from '@/types/ledger';
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

/** Мапа типів операцій на шляхи перегляду документа */
const OPERATION_TYPE_ROUTES: Record<string, string> = {
  invoice: '/documents/invoice',
  return: '/documents/return',
};

const LedgerPage: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: suppliersData } = useAllSuppliers();
  const [selectedSupplierId, setSelectedSupplierId] = useState<string | null>(null);
  const [showPaymentModal, setShowPaymentModal] = useState(false);
  const [paymentForm, setPaymentForm] = useState({
    amount: '',
    payment_method: 'cash' as PaymentMethod,
    notes: '',
  });
  const [selectedInvoiceId, setSelectedInvoiceId] = useState<string | null>(null);
  const [, setInvoicePaymentInfo] = useState<InvoicePaymentInfo | null>(null);

  const { data: balance } = useQuery({
    queryKey: ['supplier-balance', selectedSupplierId],
    queryFn: () => ledgerService.getSupplierBalance(selectedSupplierId!),
    enabled: !!selectedSupplierId,
  });

  const { data: supplierInvoices } = useQuery({
    queryKey: ['supplier-invoices', selectedSupplierId],
    queryFn: () => ledgerService.getSupplierInvoices(selectedSupplierId!),
    enabled: !!selectedSupplierId,
  });

  const { data: paymentInfo } = useQuery({
    queryKey: ['invoice-payment-info', selectedInvoiceId],
    queryFn: () => ledgerService.getInvoicePaymentInfo(selectedInvoiceId!),
    enabled: !!selectedInvoiceId,
  });

  const { data: ledgerData, isLoading: isLedgerLoading } = useQuery({
    queryKey: ['supplier-ledger', selectedSupplierId],
    queryFn: () => ledgerService.getSupplierLedger(selectedSupplierId!, { page: 1, size: 50 }),
    enabled: !!selectedSupplierId,
  });

  const paymentMutation = useMutation({
    mutationFn: () => {
      const selectedInv = selectedInvoiceId
        ? supplierInvoices?.find(inv => inv.id === selectedInvoiceId)
        : null;

      return ledgerService.createPayment({
        supplier_id: selectedSupplierId!,
        amount: parseFloat(paymentForm.amount),
        payment_method: paymentForm.payment_method,
        notes: paymentForm.notes || undefined,
        document_id: selectedInvoiceId || undefined,
        document_number: selectedInv?.number || undefined,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['supplier-balance'] });
      queryClient.invalidateQueries({ queryKey: ['supplier-ledger'] });
      queryClient.invalidateQueries({ queryKey: ['supplier-invoices'] });
      toast.success('Платіж створено');
      setShowPaymentModal(false);
      setPaymentForm({ amount: '', payment_method: 'cash', notes: '' });
      setSelectedInvoiceId(null);
      setInvoicePaymentInfo(null);
    },
    onError: (error: any) => {
      // Помилка 422 (Pydantic validation) повертає масив {type, loc, msg, input}
      const detail = error?.response?.data?.detail;
      if (Array.isArray(detail)) {
        const messages = detail.map((e: any) => e.msg).join('; ');
        toast.error(messages || 'Помилка валідації даних');
      } else if (typeof detail === 'string') {
        toast.error(detail);
      } else {
        toast.error('Помилка створення платежу');
      }
    },
  });

  const supplierOptions = [
    { value: '', label: 'Виберіть постачальника' },
    ...(suppliersData?.map((s) => ({
      value: String(s.id),
      label: s.name,
    })) || []),
  ];

  /** Опції накладних для вибору в модалці */
  const invoiceOptions: SelectOption[] = [
    { value: '', label: '— Без прив\'язки до накладної —' },
    ...(supplierInvoices?.map((inv) => ({
      value: inv.id,
      label: `${inv.number} — ${formatCurrency(inv.total_amount)}`,
    })) || []),
  ];

  /** Обробник натискання на рядок таблиці */
  const handleRowClick = (item: SupplierLedgerEntry) => {
    // Відкриваємо документ тільки для накладних та повернень, якщо є document_id
    if (item.document_id && OPERATION_TYPE_ROUTES[item.operation_type]) {
      const route = `${OPERATION_TYPE_ROUTES[item.operation_type]}/${item.document_id}`;
      navigate(route);
    }
  };

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
      header: 'Накладна',
      render: (item) => {
        if (item.operation_type === 'payment' && item.document_number) {
          return (
            <span className="inline-flex items-center gap-1 text-xs text-primary-600">
              <FileText className="w-3 h-3" />
              Оплата по №{item.document_number}
            </span>
          );
        }
        if (item.operation_type === 'invoice' && item.document_number) {
          return (
            <span className="text-xs text-gray-600 dark:text-gray-400 font-medium">
              {item.document_number}
            </span>
          );
        }
        return <span className="text-xs text-gray-400">—</span>;
      },
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

      {/* Накладні постачальника зі станом оплати */}
      {selectedSupplierId && (
        <div className="card">
          <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between">
            <div>
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Накладні постачальника
              </h3>
              <p className="text-xs text-gray-400 mt-1">
                Стан оплати по кожній накладній
              </p>
            </div>
            <Button
              onClick={() => setShowPaymentModal(true)}
              size="sm"
              icon={<DollarSign className="w-4 h-4" />}
            >
              Створити платіж
            </Button>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="bg-gray-50 dark:bg-slate-800/50 border-b border-gray-200 dark:border-slate-700">
                  <th className="table-header">№</th>
                  <th className="table-header">Дата</th>
                  <th className="table-header text-right">Сума</th>
                  <th className="table-header text-right">Сплачено</th>
                  <th className="table-header text-right">Залишок</th>
                  <th className="table-header">Статус</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                {(supplierInvoices || []).map((inv) => {
                  const total = parseFloat(inv.total_amount || '0');
                  const paid = parseFloat(inv.paid_amount || '0');
                  const remaining = parseFloat(inv.remaining ?? String(Math.max(0, total - paid)));
                  const progress = total > 0 ? Math.min(100, Math.round((paid / total) * 100)) : 0;
                  const isPaid = remaining <= 0.001;
                  return (
                    <tr
                      key={inv.id}
                      className="hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors cursor-pointer"
                      onClick={() => {
                        setSelectedInvoiceId(inv.id);
                        setInvoicePaymentInfo(null);
                        setShowPaymentModal(true);
                        setPaymentForm((prev) => ({
                          ...prev,
                          amount: remaining > 0 ? String(remaining) : prev.amount,
                        }));
                      }}
                    >
                      <td className="table-cell font-medium text-primary-600 dark:text-primary-400">
                        {inv.number}
                      </td>
                      <td className="table-cell text-gray-500 text-sm">
                        {formatDateTime(inv.invoice_date)}
                      </td>
                      <td className="table-cell text-right font-medium">{formatCurrency(total)}</td>
                      <td className="table-cell text-right text-green-600 dark:text-green-400 font-medium">
                        {formatCurrency(paid)}
                      </td>
                      <td className={`table-cell text-right font-semibold ${isPaid ? 'text-green-600' : 'text-red-600'}`}>
                        {formatCurrency(remaining)}
                      </td>
                      <td className="table-cell">
                        <div className="flex items-center gap-2">
                          <div className="flex-1 max-w-[100px] h-1.5 bg-gray-200 dark:bg-slate-600 rounded-full overflow-hidden">
                            <div
                              className={`h-full rounded-full ${isPaid ? 'bg-green-500' : 'bg-amber-500'}`}
                              style={{ width: `${progress}%` }}
                            />
                          </div>
                          <Badge variant={isPaid ? 'success' : progress > 0 ? 'warning' : 'default'}>
                            {isPaid ? 'Оплачено' : progress > 0 ? `Частково (${progress}%)` : 'Не оплачено'}
                          </Badge>
                        </div>
                      </td>
                    </tr>
                  );
                })}
                {(!supplierInvoices || supplierInvoices.length === 0) && (
                  <tr>
                    <td colSpan={6} className="table-cell text-center text-gray-400 py-6">
                      Немає підтверджених накладних
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Ledger table */}
      {selectedSupplierId && (
        <div className="card">
          <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Історія операцій
            </h3>
            <p className="text-xs text-gray-400 mt-1">
              Натисніть на рядок з накладною або поверненням, щоб відкрити документ
            </p>
          </div>
          <Table
            columns={ledgerColumns}
            data={ledgerData?.items || []}
            isLoading={isLedgerLoading}
            onRowClick={handleRowClick}
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
          setSelectedInvoiceId(null);
          setInvoicePaymentInfo(null);
        }}
        title="Створити платіж"
        size="md"
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

          {/* Вибір накладної */}
          <Select
            label="Накладна (опціонально)"
            options={invoiceOptions}
            value={selectedInvoiceId || ''}
            onChange={(e) => {
              const invId = e.target.value || null;
              setSelectedInvoiceId(invId);
              setInvoicePaymentInfo(null);
            }}
          />

          {/* Інформація про оплату вибраної накладної */}
          {selectedInvoiceId && paymentInfo && (
            <div className="bg-gray-50 dark:bg-slate-700/50 rounded-lg p-3 space-y-1 text-sm">
              <div className="flex justify-between">
                <span className="text-gray-500">Сума накладної:</span>
                <span className="font-medium">{formatCurrency(paymentInfo.total_amount)}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Вже сплачено:</span>
                <span className="font-medium text-green-600">{formatCurrency(paymentInfo.paid_amount)}</span>
              </div>
              <div className="flex justify-between border-t border-gray-200 dark:border-slate-600 pt-1">
                <span className="text-gray-500">Залишок:</span>
                <span className={`font-bold ${parseFloat(paymentInfo.remaining) > 0 ? 'text-red-600' : 'text-green-600'}`}>
                  {formatCurrency(paymentInfo.remaining)}
                </span>
              </div>
              {parseFloat(paymentInfo.remaining) > 0 && (
                <button
                  type="button"
                  onClick={() =>
                    setPaymentForm((prev) => ({ ...prev, amount: paymentInfo.remaining }))
                  }
                  className="w-full mt-2 text-xs font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300 underline underline-offset-2"
                >
                  Сплатити залишок повністю
                </button>
              )}
            </div>
          )}

          <div className="flex justify-end gap-3 pt-2">
            <Button
              variant="secondary"
              onClick={() => {
                setShowPaymentModal(false);
                setPaymentForm({ amount: '', payment_method: 'cash', notes: '' });
                setSelectedInvoiceId(null);
                setInvoicePaymentInfo(null);
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
