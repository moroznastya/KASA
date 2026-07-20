import React from 'react';
import { useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  ShoppingCart,
  Package,
  FileText,
  DollarSign,
  TrendingUp,
  Plus,
  ArrowRight,
} from 'lucide-react';
import { receiptService } from '@/services/receiptService';
import { productService } from '@/services/productService';
import { documentService } from '@/services/documentService';
import { Button } from '@/components/ui/Button';
import { formatCurrency, formatDateTime, formatPaymentMethod } from '@/utils/format';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';

const DashboardPage: React.FC = () => {
  const navigate = useNavigate();

  const { data: todayStats } = useQuery({
    queryKey: ['receipts-today-stats'],
    queryFn: () => receiptService.getTodayStats(),
  });

  const { data: productsData } = useQuery({
    queryKey: ['products', { page: 1, size: 1 }],
    queryFn: () => productService.getProducts({ page: 1, size: 1 }),
  });

  const { data: recentReceipts } = useQuery({
    queryKey: ['receipts', { page: 1, size: 5 }],
    queryFn: () => receiptService.getReceipts({ page: 1, size: 5 }),
  });

  const { data: pendingDocs } = useQuery({
    queryKey: ['documents', { page: 1, size: 1, status: 'draft' }],
    queryFn: () => documentService.getDocuments({ page: 1, size: 1, status: 'draft' }),
  });

  const statsCards = [
    {
      title: 'Продажі сьогодні',
      value: todayStats ? formatCurrency(todayStats.total) : '0,00 ₴',
      subtitle: `${todayStats?.count || 0} чеків`,
      icon: <TrendingUp className="w-6 h-6" />,
      color: 'bg-primary-50 dark:bg-primary-900/20 text-primary-600',
    },
    {
      title: 'Кількість товарів',
      value: productsData?.total?.toString() || '0',
      subtitle: 'всього в системі',
      icon: <Package className="w-6 h-6" />,
      color: 'bg-success-50 dark:bg-success-900/20 text-success-600',
    },
    {
      title: 'Активні документи',
      value: pendingDocs?.total?.toString() || '0',
      subtitle: 'очікують підтвердження',
      icon: <FileText className="w-6 h-6" />,
      color: 'bg-warning-50 dark:bg-warning-900/20 text-warning-600',
    },
    {
      title: 'Загальний дохід',
      value: todayStats ? formatCurrency(todayStats.total) : '0,00 ₴',
      subtitle: 'за сьогодні',
      icon: <DollarSign className="w-6 h-6" />,
      color: 'bg-blue-50 dark:bg-blue-900/20 text-blue-600',
    },
  ];

  return (
    <div className="space-y-6">
      {/* Page header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Панель керування
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Огляд системи Kasa POS
          </p>
        </div>
        <Button onClick={() => navigate('/pos')} icon={<ShoppingCart className="w-4 h-4" />}>
          Новий чек
        </Button>
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

      {/* Quick actions */}
      <div className="card p-5">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-4">
          Швидкі дії
        </h3>
        <div className="flex flex-wrap gap-3">
          <Button
            variant="primary"
            icon={<Plus className="w-4 h-4" />}
            onClick={() => navigate('/pos')}
          >
            Новий чек
          </Button>
          <Button
            variant="secondary"
            icon={<Plus className="w-4 h-4" />}
            onClick={() => navigate('/documents/invoice/new')}
          >
            Прибуткова накладна
          </Button>
          <Button
            variant="secondary"
            icon={<Plus className="w-4 h-4" />}
            onClick={() => navigate('/documents/transfer/new')}
          >
            Переміщення
          </Button>
          <Button
            variant="secondary"
            icon={<Plus className="w-4 h-4" />}
            onClick={() => navigate('/products/new')}
          >
            Новий товар
          </Button>
        </div>
      </div>

      {/* Recent receipts */}
      <div className="card">
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-200 dark:border-slate-700">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Останні чеки
          </h3>
          <button
            onClick={() => navigate('/reports')}
            className="text-sm text-primary-600 hover:text-primary-700 flex items-center gap-1"
          >
            Всі звіти <ArrowRight className="w-4 h-4" />
          </button>
        </div>
        <div className="overflow-x-auto">
          {!recentReceipts ? (
            <div className="flex justify-center py-8">
              <Spinner />
            </div>
          ) : recentReceipts.items.length === 0 ? (
            <div className="text-center py-8 text-gray-400">
              <ShoppingCart className="w-12 h-12 mx-auto mb-2 opacity-50" />
              <p>Ще немає чеків</p>
            </div>
          ) : (
            <table className="w-full">
              <thead>
                <tr className="bg-gray-50 dark:bg-slate-800/50">
                  <th className="table-header">№</th>
                  <th className="table-header">Сума</th>
                  <th className="table-header">Метод</th>
                  <th className="table-header">Статус</th>
                  <th className="table-header">Час</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                {recentReceipts.items.map((receipt) => (
                  <tr key={receipt.id} className="hover:bg-gray-50 dark:hover:bg-slate-700/50">
                    <td className="table-cell font-medium">{receipt.receipt_number}</td>
                    <td className="table-cell">{formatCurrency(receipt.total_amount)}</td>
                    <td className="table-cell">{formatPaymentMethod(receipt.payment_method)}</td>
                    <td className="table-cell">
                      <Badge
                        variant={
                          receipt.payment_status === 'paid'
                            ? 'success'
                            : receipt.payment_status === 'debt'
                            ? 'danger'
                            : 'warning'
                        }
                      >
                        {receipt.payment_status === 'paid'
                          ? 'Оплачено'
                          : receipt.payment_status === 'debt'
                          ? 'Борг'
                          : 'Частково'}
                      </Badge>
                    </td>
                    <td className="table-cell text-gray-500">
                      {formatDateTime(receipt.created_at)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
};

export default DashboardPage;
