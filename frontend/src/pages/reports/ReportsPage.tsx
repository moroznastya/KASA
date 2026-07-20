import React, { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { BarChart3, TrendingUp, Package, DollarSign, Calendar } from 'lucide-react';
import { receiptService } from '@/services/receiptService';
import { productService } from '@/services/productService';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { formatCurrency, formatDateTime, formatPaymentMethod } from '@/utils/format';
import { Badge } from '@/components/ui/Badge';
import { Receipt } from '@/types/receipt';

type Period = 'today' | 'week' | 'month' | 'custom';

function getDateRange(period: Period): { date_from?: string; date_to?: string } {
  const now = new Date();
  const today = now.toISOString().split('T')[0];

  switch (period) {
    case 'today':
      return { date_from: today, date_to: today };
    case 'week': {
      const weekAgo = new Date(now);
      weekAgo.setDate(weekAgo.getDate() - 7);
      return { date_from: weekAgo.toISOString().split('T')[0], date_to: today };
    }
    case 'month': {
      const monthAgo = new Date(now);
      monthAgo.setMonth(monthAgo.getMonth() - 1);
      return { date_from: monthAgo.toISOString().split('T')[0], date_to: today };
    }
    case 'custom':
    default:
      return {};
  }
}

const ReportsPage: React.FC = () => {
  const [period, setPeriod] = useState<Period>('today');

  const dateRange = useMemo(() => getDateRange(period), [period]);

  const queryParams = useMemo(() => ({
    page: 1,
    size: 50,
    ...(dateRange.date_from ? { date_from: dateRange.date_from } : {}),
    ...(dateRange.date_to ? { date_to: dateRange.date_to } : {}),
  }), [dateRange]);

  const { data: receiptsData, isLoading } = useQuery({
    queryKey: ['receipts', queryParams],
    queryFn: () => receiptService.getReceipts(queryParams),
  });

  const { data: productsData } = useQuery({
    queryKey: ['products', { page: 1, size: 1 }],
    queryFn: () => productService.getProducts({ page: 1, size: 1 }),
  });

  const totalRevenue = receiptsData?.items?.reduce(
    (sum, r) => sum + parseFloat(r.total_amount),
    0
  ) || 0;

  const totalVat = receiptsData?.items?.reduce(
    (sum, r) => sum + parseFloat(r.vat_amount),
    0
  ) || 0;

  const statsCards = [
    {
      title: 'Загальний дохід',
      value: formatCurrency(totalRevenue),
      subtitle: `за обраний період`,
      icon: <DollarSign className="w-6 h-6" />,
      color: 'bg-primary-50 dark:bg-primary-900/20 text-primary-600',
    },
    {
      title: 'ПДВ',
      value: formatCurrency(totalVat),
      subtitle: 'податок',
      icon: <TrendingUp className="w-6 h-6" />,
      color: 'bg-warning-50 dark:bg-warning-900/20 text-warning-600',
    },
    {
      title: 'Кількість чеків',
      value: String(receiptsData?.total || 0),
      subtitle: 'за обраний період',
      icon: <BarChart3 className="w-6 h-6" />,
      color: 'bg-success-50 dark:bg-success-900/20 text-success-600',
    },
    {
      title: 'Товарів в системі',
      value: String(productsData?.total || 0),
      subtitle: 'всього',
      icon: <Package className="w-6 h-6" />,
      color: 'bg-blue-50 dark:bg-blue-900/20 text-blue-600',
    },
  ];

  const periodButtons: { value: Period; label: string }[] = [
    { value: 'today', label: 'Сьогодні' },
    { value: 'week', label: 'Тиждень' },
    { value: 'month', label: 'Місяць' },
    { value: 'custom', label: 'Період' },
  ];

  const columns: Column<Receipt>[] = [
    {
      key: 'receipt_number',
      header: '№ чеку',
      render: (item) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">
          {item.receipt_number}
        </span>
      ),
    },
    {
      key: 'total_amount',
      header: 'Сума',
      render: (item) => (
        <span className="font-medium">{formatCurrency(item.total_amount)}</span>
      ),
    },
    {
      key: 'payment_method',
      header: 'Метод',
      render: (item) => (
        <Badge variant="primary">{formatPaymentMethod(item.payment_method)}</Badge>
      ),
    },
    {
      key: 'payment_status',
      header: 'Статус',
      render: (item) => (
        <Badge
          variant={
            item.payment_status === 'paid'
              ? 'success'
              : item.payment_status === 'debt'
              ? 'danger'
              : 'warning'
          }
        >
          {item.payment_status === 'paid'
            ? 'Оплачено'
            : item.payment_status === 'debt'
            ? 'Борг'
            : 'Частково'}
        </Badge>
      ),
    },
    {
      key: 'created_by_name',
      header: 'Касир',
      render: (item) => item.created_by_name || '-',
    },
    {
      key: 'created_at',
      header: 'Час',
      render: (item) => (
        <span className="text-gray-500">{formatDateTime(item.created_at)}</span>
      ),
    },
  ];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Звіти</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Аналітика та звітність
          </p>
        </div>
      </div>

      {/* Period selector */}
      <div className="flex gap-2">
        {periodButtons.map((btn) => (
          <button
            key={btn.value}
            onClick={() => setPeriod(btn.value)}
            className={`
              px-4 py-2 rounded-lg text-sm font-medium transition-colors
              ${
                period === btn.value
                  ? 'bg-primary-600 text-white'
                  : 'bg-white dark:bg-slate-800 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-slate-700 border border-gray-200 dark:border-slate-700'
              }
            `}
          >
            {btn.label}
          </button>
        ))}
      </div>

      {/* Stats cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {statsCards.map((card) => (
          <div key={card.title} className="card p-5">
            <div className="flex items-start justify-between">
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">{card.title}</p>
                <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">
                  {card.value}
                </p>
                <p className="text-xs text-gray-400 dark:text-gray-500 mt-1">
                  {card.subtitle}
                </p>
              </div>
              <div className={`p-3 rounded-xl ${card.color}`}>{card.icon}</div>
            </div>
          </div>
        ))}
      </div>

      {/* Sales table */}
      <div className="card">
        <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Продажі
          </h3>
        </div>
        <Table
          columns={columns}
          data={receiptsData?.items || []}
          isLoading={isLoading}
          keyExtractor={(item) => item.id}
          emptyMessage="Немає продажів за обраний період"
          emptyIcon={<BarChart3 className="w-12 h-12" />}
        />
      </div>
    </div>
  );
};

export default ReportsPage;
