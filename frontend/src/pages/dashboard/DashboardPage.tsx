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
  Receipt,
  PiggyBank,
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
      value: todayStats ? formatCurrency(todayStats.total_sales) : '0,00 ₴',
      subtitle: `${todayStats?.receipts_count || 0} чеків, ${todayStats?.items_sold || 0} товарів`,
      icon: <TrendingUp className="w-6 h-6" />,
      color: 'bg-primary-50 dark:bg-primary-900/20 text-primary-600',
    },
    {
      title: 'Чистий прибуток',
      value: todayStats ? formatCurrency(todayStats.total_profit) : '0,00 ₴',
      subtitle: 'за сьогодні',
      icon: <PiggyBank className="w-6 h-6" />,
      color: 'bg-success-50 dark:bg-success-900/20 text-success-600',
    },
    {
      title: 'Кількість товарів',
      value: productsData?.total?.toString() || '0',
      subtitle: 'всього в системі',
      icon: <Package className="w-6 h-6" />,
      color: 'bg-blue-50 dark:bg-blue-900/20 text-blue-600',
    },
    {
      title: 'Активні документи',
      value: pendingDocs?.total?.toString() || '0',
      subtitle: 'очікують підтвердження',
      icon: <FileText className="w-6 h-6" />,
      color: 'bg-warning-50 dark:bg-warning-900/20 text-warning-600',
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
            Огляд системи Torgashka
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
      {recentReceipts?.items && recentReceipts.items.length > 0 && (
        <div className="bg-white dark:bg-slate-800 rounded-lg shadow-sm border border-gray-200 dark:border-slate-700 p-6">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-2">
              <Receipt className="w-5 h-5 text-primary-600" />
              <h3 className="text-lg font-semibold text-gray-900 dark:text-white">Останні чеки</h3>
            </div>
            <button
              onClick={() => navigate('/receipts')}
              className="text-sm text-primary-600 hover:text-primary-700 flex items-center gap-1"
            >
              Всі чеки <ArrowRight className="w-4 h-4" />
            </button>
          </div>
          <div className="space-y-2">
            {recentReceipts.items.map((receipt: any) => (
              <div
                key={receipt.id}
                onClick={() => navigate(`/receipts/${receipt.id}`)}
                className="flex items-center justify-between p-3 rounded-lg hover:bg-gray-50 dark:hover:bg-slate-700/50 cursor-pointer transition-colors border border-gray-100 dark:border-slate-700"
              >
                <div className="flex items-center gap-3">
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {receipt.receipt_number}
                  </span>
                  <span className={`inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium ${
                    receipt.receipt_type === 'sale'
                      ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400'
                      : 'bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400'
                  }`}>
                    {receipt.receipt_type === 'sale' ? 'ПРОДАЖ' : 'ПОВЕРНЕННЯ'}
                  </span>
                </div>
                <div className="flex items-center gap-4">
                  <span className="text-sm text-gray-500 dark:text-gray-400">
                    {receipt.cashier_name || 'Невідомо'}
                  </span>
                  <span className="text-sm font-semibold text-gray-900 dark:text-white">
                    {parseFloat(receipt.total_amount).toLocaleString('uk-UA', { minimumFractionDigits: 2 })} грн
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

export default DashboardPage;
