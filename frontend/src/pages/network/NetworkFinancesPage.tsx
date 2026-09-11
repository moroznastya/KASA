import React, { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  Banknote,
  BookOpen,
  HandCoins,
  Landmark,
  RefreshCw,
  Scale,
  Wallet,
} from 'lucide-react';
import { adminReportsService } from '@/services/adminReportsService';
import { Table, Column } from '@/components/ui/Table';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { EmptyState } from '@/components/ui/EmptyState';
import { ReportPeriodBar } from '@/components/reporting/ReportPeriodBar';
import { formatCurrency } from '@/utils/format';
import { getReportRange, ReportPeriod } from '@/utils/reportPeriod';
import type {
  CashOperationStore,
  CashOperationsReport,
  SupplierLedgerReport,
  SupplierLedgerRow,
} from '@/types/adminReports';

/**
 * «Фінанси мережі» (Етап 4 адмін-панелі, ТЗ 5.6):
 *   вкладка «Каса» — deposit/collection (cash_operations) по точках за період;
 *   вкладка «Постачальники» — зведений баланс взаєморозрахунків
 *   (supplier_ledger: оборот за період + поточний balance_after).
 */

type FinancesTab = 'cash' | 'suppliers';

const NetworkFinancesPage: React.FC = () => {
  const [tab, setTab] = useState<FinancesTab>('cash');
  const [period, setPeriod] = useState<ReportPeriod>('month');
  const [customFrom, setCustomFrom] = useState('');
  const [customTo, setCustomTo] = useState('');

  const range = useMemo(
    () => getReportRange(period, customFrom, customTo),
    [period, customFrom, customTo],
  );
  const enabled = Boolean(range.from && range.to);

  const cashQuery = useQuery<CashOperationsReport>({
    queryKey: ['admin-cash-operations', range],
    queryFn: () =>
      adminReportsService.cashOperations({ from: range.from, to: range.to }),
    enabled: enabled && tab === 'cash',
  });

  const ledgerQuery = useQuery<SupplierLedgerReport>({
    queryKey: ['admin-supplier-ledger', range],
    queryFn: () =>
      adminReportsService.supplierLedger({ from: range.from, to: range.to }),
    enabled: enabled && tab === 'suppliers',
  });

  const query = tab === 'cash' ? cashQuery : ledgerQuery;
  const data = query.data as CashOperationsReport | SupplierLedgerReport | undefined;

  const tabButton = (value: FinancesTab, label: string, icon: React.ReactNode) => (
    <button
      onClick={() => setTab(value)}
      className={`
        inline-flex items-center gap-2 px-5 py-2.5 rounded-lg text-sm font-medium transition-colors
        ${
          tab === value
            ? 'bg-primary-600 text-white shadow-sm'
            : 'bg-white dark:bg-slate-800 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-slate-700 border border-gray-200 dark:border-slate-700'
        }
      `}
    >
      {icon}
      {label}
    </button>
  );

  const cashColumns: Column<CashOperationStore>[] = [
    {
      key: 'store_name',
      header: 'Точка',
      render: (s) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">{s.store_name}</span>
      ),
    },
    {
      key: 'deposit',
      header: 'Внесення',
      render: (s) => <span className="font-semibold text-success-600">{formatCurrency(s.deposit)}</span>,
    },
    {
      key: 'collection',
      header: 'Інкасації',
      render: (s) => <span className="font-semibold text-warning-600">{formatCurrency(s.collection)}</span>,
    },
    {
      key: 'operations',
      header: 'Операцій',
      render: (s) => String(s.operations),
    },
  ];

  const ledgerColumns: Column<SupplierLedgerRow>[] = [
    {
      key: 'supplier_name',
      header: 'Постачальник',
      render: (r) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">{r.supplier_name}</span>
      ),
    },
    {
      key: 'period_operations',
      header: 'Операцій',
      render: (r) => String(r.period_operations),
    },
    {
      key: 'period_inflow',
      header: 'Надходження',
      render: (r) => <span className="text-gray-700 dark:text-gray-300">{formatCurrency(r.period_inflow)}</span>,
    },
    {
      key: 'period_outflow',
      header: 'Сплати',
      render: (r) => <span className="text-gray-700 dark:text-gray-300">{formatCurrency(r.period_outflow)}</span>,
    },
    {
      key: 'period_net',
      header: 'Оборот за період',
      render: (r) => (
        <span className={`font-medium ${Number(r.period_net) < 0 ? 'text-danger-600' : 'text-success-600'}`}>
          {formatCurrency(r.period_net)}
        </span>
      ),
    },
    {
      key: 'current_balance',
      header: 'Поточний борг',
      render: (r) => (
        <span className="font-semibold">
          {formatCurrency(r.current_balance)}
        </span>
      ),
    },
  ];

  const renderStats = () => {
    if (tab === 'cash') {
      const c = data as CashOperationsReport | undefined;
      const totals = c?.totals;
      const cards = [
        {
          title: 'Внесення (deposit)',
          value: totals ? formatCurrency(totals.deposit) : '0,00 ₴',
          icon: <HandCoins className="w-6 h-6" />,
          color: 'bg-success-50 dark:bg-success-900/20 text-success-600',
        },
        {
          title: 'Інкасації (collection)',
          value: totals ? formatCurrency(totals.collection) : '0,00 ₴',
          icon: <Wallet className="w-6 h-6" />,
          color: 'bg-warning-50 dark:bg-warning-900/20 text-warning-600',
        },
        {
          title: 'Операцій по касах',
          value: String(totals?.operations ?? 0),
          icon: <Landmark className="w-6 h-6" />,
          color: 'bg-primary-50 dark:bg-primary-900/20 text-primary-600',
        },
      ];
      return (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          {cards.map((s) => (
            <div
              key={s.title}
              className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 p-5 flex items-start gap-4"
            >
              <div className={`p-3 rounded-lg ${s.color}`}>{s.icon}</div>
              <div className="min-w-0">
                <p className="text-sm text-gray-500 dark:text-gray-400 truncate">{s.title}</p>
                <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">{s.value}</p>
              </div>
            </div>
          ))}
        </div>
      );
    }
    const l = data as SupplierLedgerReport | undefined;
    const totals = l?.totals;
    const cards = [
      {
        title: 'Надходження за період',
        value: totals ? formatCurrency(totals.inflow) : '0,00 ₴',
        icon: <BookOpen className="w-6 h-6" />,
        color: 'bg-primary-50 dark:bg-primary-900/20 text-primary-600',
      },
      {
        title: 'Сплати за період',
        value: totals ? formatCurrency(totals.outflow) : '0,00 ₴',
        icon: <Banknote className="w-6 h-6" />,
        color: 'bg-warning-50 dark:bg-warning-900/20 text-warning-600',
      },
      {
        title: 'Зведений борг мережі',
        value: totals ? formatCurrency(totals.balance) : '0,00 ₴',
        icon: <Scale className="w-6 h-6" />,
        color: 'bg-danger-50 dark:bg-danger-900/20 text-danger-600',
      },
    ];
    return (
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {cards.map((s) => (
          <div
            key={s.title}
            className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 p-5 flex items-start gap-4"
          >
            <div className={`p-3 rounded-lg ${s.color}`}>{s.icon}</div>
            <div className="min-w-0">
              <p className="text-sm text-gray-500 dark:text-gray-400 truncate">{s.title}</p>
              <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">{s.value}</p>
            </div>
          </div>
        ))}
      </div>
    );
  };

  return (
    <div className="p-6 max-w-7xl mx-auto space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
          Фінанси мережі
        </h1>
        <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
          Каса мережі (внесення/інкасації) та взаєморозрахунки з постачальниками
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {tabButton('cash', 'Каса', <Wallet className="w-4 h-4" />)}
        {tabButton('suppliers', 'Постачальники', <TruckIcon />)}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3">
        <ReportPeriodBar
          period={period}
          onPeriodChange={setPeriod}
          customFrom={customFrom}
          customTo={customTo}
          onCustomFromChange={setCustomFrom}
          onCustomToChange={setCustomTo}
        />
        {query.isError && (
          <button
            onClick={() => query.refetch()}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium text-danger-600 border border-danger-200 dark:border-danger-900 hover:bg-danger-50"
          >
            <RefreshCw className="w-4 h-4" /> Повторити
          </button>
        )}
      </div>

      {query.isError && (
        <div className="flex items-center gap-3 p-4 rounded-xl bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300 text-sm">
          <AlertTriangle className="w-5 h-5 shrink-0" />
          Не вдалося завантажити звіт. Перевірте, що активовано режим адміністратора мережі.
        </div>
      )}

      {renderStats()}

      {!enabled || query.isLoading ? (
        <div className="flex items-center justify-center py-16">
          <Spinner size="lg" />
        </div>
      ) : (
        <>
          <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 overflow-hidden">
            <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-2">
              {tab === 'cash' ? (
                <Wallet className="w-5 h-5 text-primary-600" />
              ) : (
                <BookOpen className="w-5 h-5 text-primary-600" />
              )}
              <h2 className="font-semibold text-gray-900 dark:text-gray-100">
                {tab === 'cash' ? 'Каса по точках' : 'Постачальники'}
              </h2>
              <Badge variant="primary" className="ml-auto">
                {tab === 'cash'
                  ? `${(data as CashOperationsReport | undefined)?.stores.length ?? 0} точок`
                  : `${(data as SupplierLedgerReport | undefined)?.suppliers.length ?? 0} постачальників`}
              </Badge>
            </div>
            {tab === 'cash' ? (
              <Table<CashOperationStore>
                columns={cashColumns}
                data={(data as CashOperationsReport | undefined)?.stores ?? []}
                keyExtractor={(s) => s.store_id}
                emptyMessage="Немає операцій з касою за обраний період"
                emptyIcon={<Wallet className="w-10 h-10" />}
              />
            ) : (
              <Table<SupplierLedgerRow>
                columns={ledgerColumns}
                data={(data as SupplierLedgerReport | undefined)?.suppliers ?? []}
                keyExtractor={(r) => r.supplier_id}
                emptyMessage="Немає взаєморозрахунків за обраний період"
                emptyIcon={<BookOpen className="w-10 h-10" />}
              />
            )}
          </div>

          {tab === 'cash' && (data as CashOperationsReport | undefined)?.stores.length === 0 && (
            <div className="py-6">
              <EmptyState
                icon={<Wallet className="w-10 h-10" />}
                message="Операцій з касою немає"
                description="За обраний період не було внесень або інкасацій по активних точках"
              />
            </div>
          )}
        </>
      )}
    </div>
  );
};

/** Тruck-іконка постачальників (окремий компонент — lucide Truck у заголовку не дублює). */
const TruckIcon: React.FC = () => (
  <svg
    className="w-4 h-4"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M14 18V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v11a1 1 0 0 0 1 1h2" />
    <path d="M15 18h-5" />
    <path d="M19 18h2a1 1 0 0 0 1-1v-3.65a1 1 0 0 0-.22-.62l-3.48-4.35A1 1 0 0 0 17.52 8H14" />
    <circle cx="17" cy="18" r="2" />
    <circle cx="7" cy="18" r="2" />
  </svg>
);

export default NetworkFinancesPage;
