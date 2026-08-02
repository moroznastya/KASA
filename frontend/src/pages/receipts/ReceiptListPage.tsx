import React from 'react';
import { useNavigate } from 'react-router-dom';
import { useReceipts } from '@/hooks/useReceipts';
import { useUsers } from '@/hooks/useUsers';
import { Button } from '@/components/ui/Button';
import { Select, SelectOption } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { Search, ChevronLeft, ChevronRight, Receipt } from 'lucide-react';
import { usePageState } from '@/hooks/usePageState';

const paymentMethodLabels: Record<string, string> = {
  cash: 'Готівка',
  card: 'Картка',
  mixed: 'Готівка + Картка',
};

const paymentMethodColors: Record<string, string> = {
  cash: 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400',
  card: 'bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-400',
  mixed: 'bg-purple-100 text-purple-800 dark:bg-purple-900/30 dark:text-purple-400',
};

const paymentMethodOptions: SelectOption[] = [
  { value: '', label: 'Всі способи оплати' },
  { value: 'cash', label: 'Готівка' },
  { value: 'card', label: 'Картка' },
  { value: 'mixed', label: 'Готівка + Картка' },
];

const ReceiptListPage: React.FC = () => {
  const navigate = useNavigate();
  const [pageState, setPageState] = usePageState('receipt_list', {
    page: 1,
    searchQuery: '',
    dateFrom: '',
    dateTo: '',
    paymentMethod: '',
    cashierFilter: '',
  });
  const { page, searchQuery, dateFrom, dateTo, paymentMethod, cashierFilter } = pageState;

  const { data, isLoading } = useReceipts({
    page,
    size: 20,
    ...(searchQuery ? { search: searchQuery } : {}),
    ...(dateFrom ? { date_from: dateFrom } : {}),
    ...(dateTo ? { date_to: dateTo } : {}),
    ...(paymentMethod ? { payment_method: paymentMethod } : {}),
    ...(cashierFilter ? { cashier_id: cashierFilter } : {}),
  } as any);

  const { data: usersData } = useUsers({ page: 1, size: 100 });
  const users = usersData?.items || [];

  const cashierOptions: SelectOption[] = [
    { value: '', label: 'Всі касири' },
    ...users.map((u: any) => ({ value: u.id, label: u.name })),
  ];

  const receipts = data?.items || [];
  const total = data?.total || 0;
  const totalPages = data?.pages || Math.ceil(total / 20);

  const formatDate = (dateStr: string) => {
    const d = new Date(dateStr);
    return d.toLocaleDateString('uk-UA', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const formatAmount = (amount: string) => {
    return parseFloat(amount).toLocaleString('uk-UA', {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Receipt className="w-6 h-6 text-primary-600" />
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
            Чеки продажу
          </h1>
        </div>
      </div>

      {/* Filters */}
      <div className="bg-white dark:bg-slate-800 rounded-lg shadow-sm border border-gray-200 dark:border-slate-700 p-4">
        <div className="grid grid-cols-1 md:grid-cols-6 gap-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
            <input
              type="text"
              placeholder="Пошук за номером..."
              aria-label="Пошук за номером чеку"
              value={searchQuery}
              onChange={(e) => { setPageState({ searchQuery: e.target.value, page: 1 }); }}
              className="w-full pl-10 pr-4 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-gray-900 dark:text-white text-sm focus:ring-2 focus:ring-primary-500 focus:border-transparent"
            />
          </div>
          <div>
            <input
              type="date"
              value={dateFrom}
              onChange={(e) => { setPageState({ dateFrom: e.target.value, page: 1 }); }}
              className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-gray-900 dark:text-white text-sm focus:ring-2 focus:ring-primary-500 focus:border-transparent"
              placeholder="Від дати"
              aria-label="Від дати"
            />
          </div>
          <div>
            <input
              type="date"
              value={dateTo}
              onChange={(e) => { setPageState({ dateTo: e.target.value, page: 1 }); }}
              className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-gray-900 dark:text-white text-sm focus:ring-2 focus:ring-primary-500 focus:border-transparent"
              placeholder="До дати"
              aria-label="До дати"
            />
          </div>
          <div>
            <Select
              options={paymentMethodOptions}
              value={paymentMethod}
              onChange={(e) => { setPageState({ paymentMethod: e.target.value, page: 1 }); }}
            />
          </div>
          <div>
            <Select
              options={cashierOptions}
              value={cashierFilter}
              onChange={(e) => { setPageState({ cashierFilter: e.target.value, page: 1 }); }}
            />
          </div>
          <div className="flex items-center text-sm text-gray-500 dark:text-gray-400">
            {total > 0 && (
              <span>Знайдено: <strong>{total}</strong> чеків</span>
            )}
          </div>
        </div>
      </div>

      {/* Table */}
      <div className="bg-white dark:bg-slate-800 rounded-lg shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
        {isLoading ? (
          <div className="flex justify-center py-12">
            <Spinner size="lg" />
          </div>
        ) : receipts.length === 0 ? (
          <div className="text-center py-12 text-gray-500 dark:text-gray-400">
            <Receipt className="w-12 h-12 mx-auto mb-3 opacity-50" />
            <p className="text-lg font-medium">Чеків не знайдено</p>
            <p className="text-sm mt-1">Створіть перший чек у розділі POS</p>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="bg-gray-50 dark:bg-slate-700/50 border-b border-gray-200 dark:border-slate-700">
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Номер
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Тип
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Сума
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Спосіб оплати
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Касир
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Дата
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                {receipts.map((receipt: any) => (
                  <tr
                    key={receipt.id}
                    className="hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors cursor-pointer"
                    onClick={() => navigate(`/receipts/${receipt.id}`)}
                  >
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">
                      {receipt.receipt_number}
                    </td>
                    <td className="px-4 py-3">
                      <span
                        className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
                          receipt.receipt_type === 'sale'
                            ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400'
                            : 'bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400'
                        }`}
                      >
                        {receipt.receipt_type === 'sale' ? 'ПРОДАЖ' : 'ПОВЕРНЕННЯ'}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">
                      {formatAmount(receipt.total_amount)} грн
                    </td>
                    <td className="px-4 py-3">
                      {receipt.payment_method ? (
                        <span className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${paymentMethodColors[receipt.payment_method] || 'bg-gray-100 text-gray-800'}`}>
                          {paymentMethodLabels[receipt.payment_method] || receipt.payment_method}
                        </span>
                      ) : (
                        <span className="text-xs text-gray-500 dark:text-gray-400">—</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {receipt.cashier_name || 'Невідомо'}
                    </td>
                    <td className="px-4 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {formatDate(receipt.created_at)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-between bg-white dark:bg-slate-800 rounded-lg shadow-sm border border-gray-200 dark:border-slate-700 px-4 py-3">
          <div className="text-sm text-gray-500 dark:text-gray-400">
            Сторінка {page} з {totalPages}
          </div>
          <div className="flex gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setPageState((prev: any) => ({ page: Math.max(1, prev.page - 1) }))}
              disabled={page <= 1}
            >
              <ChevronLeft className="w-4 h-4" />
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setPageState((prev: any) => ({ page: Math.min(totalPages, prev.page + 1) }))}
              disabled={page >= totalPages}
            >
              <ChevronRight className="w-4 h-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
};

export default ReceiptListPage;
