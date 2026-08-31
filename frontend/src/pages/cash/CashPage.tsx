import React, { useMemo, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Banknote,
  CreditCard,
  MapPin,
  Wallet,
} from 'lucide-react';
import { cashService } from '@/services/cashService';
import type { CashOperationType, CashType } from '@/types/cash';
import { useStoreStore } from '@/store/storeStore';
import { formatCurrency, formatDateTime } from '@/utils/format';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Modal } from '@/components/ui/Modal';
import { Spinner } from '@/components/ui/Spinner';
import { Badge } from '@/components/ui/Badge';
import { toast } from 'react-hot-toast';

const TYPE_LABELS: Record<CashOperationType, string> = {
  deposit: 'Внесення',
  collection: 'Інкасація',
};

/** Підписи кас для інтерфейсу */
const CASH_TYPE_LABELS: Record<CashType, string> = {
  cash: 'Готівка',
  card: 'Безготівка',
};

/** Фільтр журналу: усі / тільки готівка / тільки безготівка */
type JournalFilter = 'all' | CashType;

const CashPage: React.FC = () => {
  const queryClient = useQueryClient();
  const stores = useStoreStore((state) => state.stores);
  const activeStoreId = useStoreStore((state) => state.activeStoreId);

  const activeStore = stores.find((s) => s.id === activeStoreId);

  // ── Форма операції ─────────────────────────────
  const [cashType, setCashType] = useState<CashType>('cash');
  const [operationType, setOperationType] = useState<CashOperationType>('deposit');
  const [amount, setAmount] = useState('');
  const [comment, setComment] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  // Фільтр журналу за касою
  const [journalFilter, setJournalFilter] = useState<JournalFilter>('all');

  // ── Дані кас ────────────────────────────────────
  const { data, isLoading, isError } = useQuery({
    queryKey: ['cash-operations'],
    queryFn: () => cashService.getOperations(),
  });

  // ── Мутація: створення операції ────────────────
  const createMutation = useMutation({
    mutationFn: (data: {
      operation_type: CashOperationType;
      cash_type: CashType;
      amount: number;
      comment?: string;
    }) => cashService.createOperation(data),
    onSuccess: () => {
      toast.success('Операцію виконано');
      setConfirmOpen(false);
      setAmount('');
      setComment('');
      setOperationType('deposit');
      setCashType('cash');
      queryClient.invalidateQueries({ queryKey: ['cash-operations'] });
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка виконання операції');
    },
  });

  // ── Операції (з фільтром журналу) ───────────────
  const allOperations = data?.operations ?? [];

  const visibleOperations = useMemo(() => {
    if (journalFilter === 'all') return allOperations;
    return allOperations.filter((op) => op.cash_type === journalFilter);
  }, [allOperations, journalFilter]);

  // ── Підсумки deposit/collection для обраної каси ──
  const totals = useMemo(() => {
    const ops = allOperations.filter((op) => op.cash_type === cashType);
    return {
      deposit: ops
        .filter((op) => op.operation_type === 'deposit')
        .reduce((sum, op) => sum + parseFloat(op.amount || '0'), 0),
      collection: ops
        .filter((op) => op.operation_type === 'collection')
        .reduce((sum, op) => sum + parseFloat(op.amount || '0'), 0),
    };
  }, [allOperations, cashType]);

  // ── Валідація та відкриття підтвердження ────────
  const handleSubmit = () => {
    const value = parseFloat(amount);
    if (isNaN(value) || value <= 0) {
      toast.error('Введіть суму більше нуля');
      return;
    }
    setConfirmOpen(true);
  };

  const handleConfirm = () => {
    const value = parseFloat(amount);
    if (isNaN(value) || value <= 0) {
      toast.error('Введіть суму більше нуля');
      return;
    }
    createMutation.mutate({
      operation_type: operationType,
      cash_type: cashType,
      amount: value,
      comment: comment.trim() || undefined,
    });
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <Spinner size="lg" />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <div className="text-center">
          <p className="text-red-500 font-medium">Помилка завантаження даних кас</p>
          <p className="text-sm text-gray-500 mt-1">Спробуйте пізніше</p>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* ── Заголовок ─────────────────────── */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Wallet className="w-6 h-6 text-primary-600" />
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
              Каса
            </h1>
            {activeStore && (
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-0.5 flex items-center gap-1">
                <MapPin className="w-3.5 h-3.5" />
                {activeStore.name}
              </p>
            )}
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* ── Ліва колонка: баланси + форма ── */}
        <div className="lg:col-span-1 space-y-6">
          {/* Баланси кас: готівка / безготівка */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-1 gap-4">
            {/* Каса готівка */}
            <div className="card p-5">
              <div className="flex items-center gap-3 mb-3">
                <div className="w-10 h-10 rounded-xl bg-green-50 dark:bg-green-900/20 flex items-center justify-center text-green-600 dark:text-green-400">
                  <Banknote className="w-5 h-5" />
                </div>
                <div>
                  <p className="text-xs text-gray-500 dark:text-gray-400">
                    Каса готівка
                  </p>
                  <p className="text-lg font-bold text-gray-900 dark:text-white">
                    {formatCurrency(data?.balances?.cash ?? '0')}
                  </p>
                </div>
              </div>
              <div className="space-y-1.5 text-sm">
                <div className="flex items-center justify-between text-gray-600 dark:text-gray-300">
                  <span className="flex items-center gap-1.5">
                    <ArrowDownToLine className="w-3.5 h-3.5 text-green-600" />
                    Внесення
                  </span>
                  <span className="font-medium text-green-600 dark:text-green-400">
                    +{formatCurrency(cashType === 'cash' ? totals.deposit : 0)}
                  </span>
                </div>
                <div className="flex items-center justify-between text-gray-600 dark:text-gray-300">
                  <span className="flex items-center gap-1.5">
                    <ArrowUpFromLine className="w-3.5 h-3.5 text-red-600" />
                    Інкасація
                  </span>
                  <span className="font-medium text-red-600 dark:text-red-400">
                    −{formatCurrency(cashType === 'cash' ? totals.collection : 0)}
                  </span>
                </div>
              </div>
            </div>

            {/* Каса безготівка */}
            <div className="card p-5">
              <div className="flex items-center gap-3 mb-3">
                <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-900/20 flex items-center justify-center text-blue-600 dark:text-blue-400">
                  <CreditCard className="w-5 h-5" />
                </div>
                <div>
                  <p className="text-xs text-gray-500 dark:text-gray-400">
                    Каса безготівка
                  </p>
                  <p className="text-lg font-bold text-gray-900 dark:text-white">
                    {formatCurrency(data?.balances?.card ?? '0')}
                  </p>
                </div>
              </div>
              <div className="space-y-1.5 text-sm">
                <div className="flex items-center justify-between text-gray-600 dark:text-gray-300">
                  <span className="flex items-center gap-1.5">
                    <ArrowDownToLine className="w-3.5 h-3.5 text-green-600" />
                    Внесення
                  </span>
                  <span className="font-medium text-green-600 dark:text-green-400">
                    +{formatCurrency(cashType === 'card' ? totals.deposit : 0)}
                  </span>
                </div>
                <div className="flex items-center justify-between text-gray-600 dark:text-gray-300">
                  <span className="flex items-center gap-1.5">
                    <ArrowUpFromLine className="w-3.5 h-3.5 text-red-600" />
                    Інкасація
                  </span>
                  <span className="font-medium text-red-600 dark:text-red-400">
                    −{formatCurrency(cashType === 'card' ? totals.collection : 0)}
                  </span>
                </div>
              </div>
            </div>
          </div>

          {/* Форма операції */}
          <div className="card p-6 space-y-4">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Нова операція
            </h3>

            {/* Перемикач каси: Готівка / Безготівка */}
            <div className="grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => setCashType('cash')}
                className={`
                  flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-sm font-medium border transition-all
                  ${
                    cashType === 'cash'
                      ? 'bg-green-50 border-green-300 text-green-700 dark:bg-green-900/30 dark:border-green-700 dark:text-green-400'
                      : 'bg-white dark:bg-slate-800 border-gray-200 dark:border-slate-600 text-gray-600 dark:text-gray-400 hover:border-gray-300 dark:hover:border-slate-500'
                  }
                `}
              >
                <Banknote className="w-4 h-4" />
                Готівка
              </button>
              <button
                type="button"
                onClick={() => setCashType('card')}
                className={`
                  flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-sm font-medium border transition-all
                  ${
                    cashType === 'card'
                      ? 'bg-blue-50 border-blue-300 text-blue-700 dark:bg-blue-900/30 dark:border-blue-700 dark:text-blue-400'
                      : 'bg-white dark:bg-slate-800 border-gray-200 dark:border-slate-600 text-gray-600 dark:text-gray-400 hover:border-gray-300 dark:hover:border-slate-500'
                  }
                `}
              >
                <CreditCard className="w-4 h-4" />
                Безготівка
              </button>
            </div>

            {/* Перемикач типу: Внесення / Інкасація */}
            <div className="grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => setOperationType('deposit')}
                className={`
                  flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-sm font-medium border transition-all
                  ${
                    operationType === 'deposit'
                      ? 'bg-green-50 border-green-300 text-green-700 dark:bg-green-900/30 dark:border-green-700 dark:text-green-400'
                      : 'bg-white dark:bg-slate-800 border-gray-200 dark:border-slate-600 text-gray-600 dark:text-gray-400 hover:border-gray-300 dark:hover:border-slate-500'
                  }
                `}
              >
                <ArrowDownToLine className="w-4 h-4" />
                Внесення
              </button>
              <button
                type="button"
                onClick={() => setOperationType('collection')}
                className={`
                  flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-sm font-medium border transition-all
                  ${
                    operationType === 'collection'
                      ? 'bg-red-50 border-red-300 text-red-700 dark:bg-red-900/30 dark:border-red-700 dark:text-red-400'
                      : 'bg-white dark:bg-slate-800 border-gray-200 dark:border-slate-600 text-gray-600 dark:text-gray-400 hover:border-gray-300 dark:hover:border-slate-500'
                  }
                `}
              >
                <ArrowUpFromLine className="w-4 h-4" />
                Інкасація
              </button>
            </div>

            <Input
              label="Сума"
              type="number"
              min="0.01"
              step="0.01"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              placeholder="0.00"
            />

            <Input
              label="Коментар"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              placeholder="Необов'язково"
            />

            <Button
              onClick={handleSubmit}
              className="w-full"
              icon={
                cashType === 'cash'
                  ? <Banknote className="w-4 h-4" />
                  : <CreditCard className="w-4 h-4" />
              }
            >
              Виконати
            </Button>
          </div>
        </div>

        {/* ── Права колонка: журнал операцій ── */}
        <div className="lg:col-span-2 card overflow-hidden">
          <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between gap-4 flex-wrap">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Журнал операцій
            </h3>

            {/* Фільтр за касою */}
            <div className="flex items-center gap-1 bg-gray-100 dark:bg-slate-700/60 rounded-lg p-1">
              {(
                [
                  { value: 'all', label: 'Усі' },
                  { value: 'cash', label: 'Готівка' },
                  { value: 'card', label: 'Безготівка' },
                ] as { value: JournalFilter; label: string }[]
              ).map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => setJournalFilter(opt.value)}
                  className={`
                    px-3 py-1 rounded-md text-xs font-medium transition-colors
                    ${
                      journalFilter === opt.value
                        ? 'bg-white dark:bg-slate-600 text-gray-900 dark:text-white shadow-sm'
                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
                    }
                  `}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>

          {visibleOperations.length === 0 ? (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              <Wallet className="w-12 h-12 mx-auto mb-3 opacity-50" />
              <p className="text-lg font-medium">Немає операцій</p>
              <p className="text-sm mt-1">
                {journalFilter === 'all'
                  ? 'Виконайте внесення або інкасацію готівки'
                  : `Немає операцій для каси «${CASH_TYPE_LABELS[journalFilter]}»`}
              </p>
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full">
                <thead>
                  <tr className="bg-gray-50 dark:bg-slate-700/50 border-b border-gray-200 dark:border-slate-700">
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Дата
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Каса
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Тип
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Сума
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Хто
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Коментар
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {visibleOperations.map((op) => (
                    <tr key={op.id} className="hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors">
                      <td className="px-4 py-3 text-sm text-gray-700 dark:text-gray-300 whitespace-nowrap">
                        {formatDateTime(op.created_at)}
                      </td>
                      <td className="px-4 py-3">
                        {op.cash_type === 'cash' ? (
                          <Badge variant="success" size="sm">
                            <Banknote className="w-3 h-3 mr-1" />
                            Готівка
                          </Badge>
                        ) : (
                          <Badge variant="info" size="sm">
                            <CreditCard className="w-3 h-3 mr-1" />
                            Безготівка
                          </Badge>
                        )}
                      </td>
                      <td className="px-4 py-3">
                        {op.operation_type === 'deposit' ? (
                          <Badge variant="success" size="sm">
                            <ArrowDownToLine className="w-3 h-3 mr-1" />
                            Внесення
                          </Badge>
                        ) : (
                          <Badge variant="danger" size="sm">
                            <ArrowUpFromLine className="w-3 h-3 mr-1" />
                            Інкасація
                          </Badge>
                        )}
                      </td>
                      <td className="px-4 py-3 text-sm font-semibold text-gray-900 dark:text-white whitespace-nowrap">
                        {op.operation_type === 'deposit' ? '+' : '−'}
                        {formatCurrency(op.amount)}
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-700 dark:text-gray-300">
                        {op.user_name || '—'}
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-500 dark:text-gray-400 max-w-[200px] truncate" title={op.comment || ''}>
                        {op.comment || '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>

      {/* ── Модалка підтвердження ──────────── */}
      <Modal
        isOpen={confirmOpen}
        onClose={() => setConfirmOpen(false)}
        title="Підтвердження операції"
        size="sm"
      >
        <div className="space-y-3">
          <div className="flex items-center justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Каса</span>
            <span className="font-medium text-gray-900 dark:text-gray-100">
              {CASH_TYPE_LABELS[cashType]}
            </span>
          </div>
          <div className="flex items-center justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Тип</span>
            <span className="font-medium text-gray-900 dark:text-gray-100">
              {TYPE_LABELS[operationType]}
            </span>
          </div>
          <div className="flex items-center justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Сума</span>
            <span className={`font-semibold ${operationType === 'deposit' ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
              {operationType === 'deposit' ? '+' : '−'}
              {formatCurrency(parseFloat(amount) || 0)}
            </span>
          </div>
          {comment.trim() && (
            <div className="flex items-center justify-between text-sm">
              <span className="text-gray-500 dark:text-gray-400">Коментар</span>
              <span className="font-medium text-gray-900 dark:text-gray-100 max-w-[200px] truncate" title={comment}>
                {comment}
              </span>
            </div>
          )}
        </div>
        <div className="flex items-center justify-end gap-3 pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => setConfirmOpen(false)}>
            Скасувати
          </Button>
          <Button
            onClick={handleConfirm}
            disabled={createMutation.isPending}
            variant={operationType === 'deposit' ? 'success' : 'danger'}
            icon={createMutation.isPending ? undefined : (cashType === 'cash' ? <Banknote className="w-4 h-4" /> : <CreditCard className="w-4 h-4" />)}
          >
            {createMutation.isPending ? 'Виконання...' : 'Виконати'}
          </Button>
        </div>
      </Modal>
    </div>
  );
};

export default CashPage;
