import React, { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  BarChart3,
  PackageSearch,
  PiggyBank,
  ReceiptText,
  RefreshCw,
  TrendingDown,
  TrendingUp,
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
  NetworkSalesReport,
  NetworkSalesStore,
  NetworkTopProduct,
} from '@/types/adminReports';

/**
 * «Дашборд мережі» (Етап 4 адмін-панелі, ТЗ 5.5):
 *   - селектор періоду (today/week/month/custom);
 *   - картки підсумків по мережі (продажі/повернення/нетто/чеки);
 *   - порівняльний список точок (продажі/повернення/нетто/чеки);
 *   - топ-N товарів мережі за сумою.
 */

interface StatCardProps {
  title: string;
  value: string;
  subtitle?: string;
  icon: React.ReactNode;
  color: string;
}

const StatCard: React.FC<StatCardProps> = ({ title, value, subtitle, icon, color }) => (
  <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 p-5 flex items-start gap-4">
    <div className={`p-3 rounded-lg ${color}`}>{icon}</div>
    <div className="min-w-0">
      <p className="text-sm text-gray-500 dark:text-gray-400 truncate">{title}</p>
      <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">{value}</p>
      {subtitle && (
        <p className="text-xs text-gray-400 dark:text-gray-500 mt-1">{subtitle}</p>
      )}
    </div>
  </div>
);

const NetworkReportsPage: React.FC = () => {
  const [period, setPeriod] = useState<ReportPeriod>('today');
  const [customFrom, setCustomFrom] = useState('');
  const [customTo, setCustomTo] = useState('');

  const range = useMemo(
    () => getReportRange(period, customFrom, customTo),
    [period, customFrom, customTo],
  );
  const enabled = Boolean(range.from && range.to);

  const { data, isLoading, isError, refetch } = useQuery<NetworkSalesReport>({
    queryKey: ['admin-network-sales', range],
    queryFn: () =>
      adminReportsService.networkSales({ from: range.from, to: range.to }),
    enabled,
  });

  const totals = data?.totals;
  const stats: StatCardProps[] = [
    {
      title: 'Продажі (нетто)',
      value: totals ? formatCurrency(totals.net_sales) : '0,00 ₴',
      subtitle: 'продажі мінус повернення',
      icon: <TrendingUp className="w-6 h-6" />,
      color: 'bg-success-50 dark:bg-success-900/20 text-success-600',
    },
    {
      title: 'Продажі',
      value: totals ? formatCurrency(totals.sales) : '0,00 ₴',
      subtitle: 'сума чеків sale',
      icon: <PiggyBank className="w-6 h-6" />,
      color: 'bg-primary-50 dark:bg-primary-900/20 text-primary-600',
    },
    {
      title: 'Повернення',
      value: totals ? formatCurrency(totals.returns) : '0,00 ₴',
      subtitle: 'сума чеків return',
      icon: <TrendingDown className="w-6 h-6" />,
      color: 'bg-danger-50 dark:bg-danger-900/20 text-danger-600',
    },
    {
      title: 'Чеків продаж',
      value: String(totals?.sales_checks ?? 0),
      subtitle: `повернень: ${totals?.returns_checks ?? 0}`,
      icon: <ReceiptText className="w-6 h-6" />,
      color: 'bg-blue-50 dark:bg-blue-900/20 text-blue-600',
    },
  ];

  const storeColumns: Column<NetworkSalesStore>[] = [
    {
      key: 'store_name',
      header: 'Точка',
      render: (s) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">{s.store_name}</span>
      ),
    },
    {
      key: 'sales',
      header: 'Продажі',
      render: (s) => formatCurrency(s.sales),
    },
    {
      key: 'returns',
      header: 'Повернення',
      render: (s) => (
        <span className="text-danger-600">{formatCurrency(s.returns)}</span>
      ),
    },
    {
      key: 'net_sales',
      header: 'Нетто',
      render: (s) => <span className="font-semibold">{formatCurrency(s.net_sales)}</span>,
    },
    {
      key: 'sales_checks',
      header: 'Чеків',
      render: (s) => String(s.sales_checks),
    },
    {
      key: 'returns_checks',
      header: 'Повернень',
      render: (s) => String(s.returns_checks),
    },
  ];

  const topColumns: Column<NetworkTopProduct>[] = [
    {
      key: 'product_name',
      header: 'Товар',
      render: (p) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">{p.product_name}</span>
      ),
    },
    {
      key: 'total',
      header: 'Сума (нетто)',
      render: (p) => <span className="font-semibold">{formatCurrency(p.total)}</span>,
    },
  ];

  return (
    <div className="p-6 max-w-7xl mx-auto space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Дашборд мережі
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Продажі по точках мережі та топ товарів за період
          </p>
        </div>
        {isError && (
          <button
            onClick={() => refetch()}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium text-danger-600 border border-danger-200 dark:border-danger-900 hover:bg-danger-50"
          >
            <RefreshCw className="w-4 h-4" /> Повторити
          </button>
        )}
      </div>

      <ReportPeriodBar
        period={period}
        onPeriodChange={setPeriod}
        customFrom={customFrom}
        customTo={customTo}
        onCustomFromChange={setCustomFrom}
        onCustomToChange={setCustomTo}
      />

      {isError && (
        <div className="flex items-center gap-3 p-4 rounded-xl bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300 text-sm">
          <AlertTriangle className="w-5 h-5 shrink-0" />
          Не вдалося завантажити звіт. Перевірте, що активовано режим адміністратора мережі.
        </div>
      )}

      {!isError && !enabled && (
        <div className="flex items-center gap-3 p-4 rounded-xl bg-warning-50 dark:bg-warning-900/20 text-warning-700 dark:text-warning-300 text-sm">
          <AlertTriangle className="w-5 h-5 shrink-0" />
          Оберіть період «Період» (від і до), щоб побачити звіт за довільні дати.
        </div>
      )}

      {/* Картки підсумків */}
      <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-4">
        {stats.map((s) => (
          <StatCard key={s.title} {...s} />
        ))}
      </div>

      {!enabled || isLoading ? (
        <div className="flex items-center justify-center py-16">
          <Spinner size="lg" />
        </div>
      ) : (
        <>
          {/* Продажі по точках */}
          <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 overflow-hidden">
            <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-2">
              <BarChart3 className="w-5 h-5 text-primary-600" />
              <h2 className="font-semibold text-gray-900 dark:text-gray-100">
                Продажі по точках
              </h2>
              <Badge variant="primary" className="ml-auto">
                {data?.stores.length ?? 0} точок
              </Badge>
            </div>
            <Table<NetworkSalesStore>
              columns={storeColumns}
              data={data?.stores ?? []}
              keyExtractor={(s) => s.store_id}
              emptyMessage="Немає чеків за обраний період"
              emptyIcon={<BarChart3 className="w-10 h-10" />}
            />
          </div>

          {/* Топ товарів мережі */}
          <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 overflow-hidden">
            <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-2">
              <PackageSearch className="w-5 h-5 text-primary-600" />
              <h2 className="font-semibold text-gray-900 dark:text-gray-100">
                Топ товарів мережі
              </h2>
              <Badge variant="primary" className="ml-auto">
                {(data?.top_products.length ?? 0) > 0
                  ? `топ ${data?.top_products.length}`
                  : ''}
              </Badge>
            </div>
            {data?.top_products.length ? (
              <Table<NetworkTopProduct>
                columns={topColumns}
                data={data.top_products}
                keyExtractor={(p) => p.product_id}
              />
            ) : (
              <div className="py-12">
                <EmptyState
                  icon={<PackageSearch className="w-10 h-10" />}
                  message="Немає продажів"
                  description="За обраний період товари не продавались"
                />
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
};

export default NetworkReportsPage;
