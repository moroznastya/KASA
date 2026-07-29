import React, { useMemo, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Clock, User, DollarSign, Calendar } from 'lucide-react';
import { useAuthStore } from '@/store/authStore';
import {
  workSessionService,
  WorkSession,
  WorkSessionReport,
  MySessionsResponse,
} from '@/services/workSessionService';
import { formatCurrency } from '@/utils/format';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { Select, SelectOption } from '@/components/ui/Select';
import toast from 'react-hot-toast';

const monthNames: string[] = [
  'Січень', 'Лютий', 'Березень', 'Квітень', 'Травень', 'Червень',
  'Липень', 'Серпень', 'Вересень', 'Жовтень', 'Листопад', 'Грудень',
];

const WorkTimePage: React.FC = () => {
  const user = useAuthStore((state) => state.user);
  const queryClient = useQueryClient();

  const now = new Date();
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);
  const [selectedYear, setSelectedYear] = useState(now.getFullYear());
  const [editingRate, setEditingRate] = useState<Record<string, string>>({});

  const isAdmin = user?.role === 'admin';

  // --- Опції для селекторів ---
  const monthOptions: SelectOption[] = useMemo(
    () => monthNames.map((name, idx) => ({ value: idx + 1, label: name })),
    []
  );

  const yearOptions: SelectOption[] = useMemo(
    () =>
      Array.from({ length: 5 }, (_, i) => now.getFullYear() - 2 + i).map((y) => ({
        value: y,
        label: String(y),
      })),
    [now]
  );

  // --- Звіт для адміна ---
  const {
    data: reportData,
    isLoading: reportLoading,
    isError: reportError,
  } = useQuery<WorkSessionReport>({
    queryKey: ['work-sessions', 'report', selectedMonth, selectedYear],
    queryFn: () => workSessionService.getReport(selectedMonth, selectedYear),
    enabled: isAdmin,
  });

  // --- Мої сесії для касира ---
  const {
    data: mySessionsData,
    isLoading: sessionsLoading,
    isError: sessionsError,
  } = useQuery<MySessionsResponse>({
    queryKey: ['work-sessions', 'my', selectedMonth, selectedYear],
    queryFn: () => workSessionService.getMySessions(selectedMonth, selectedYear),
    enabled: !isAdmin,
  });

  // --- Мутація зміни ставки ---
  const rateMutation = useMutation({
    mutationFn: ({ userId, rate }: { userId: string; rate: number }) =>
      workSessionService.setHourlyRate(userId, rate),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-sessions', 'report', selectedMonth, selectedYear] });
      toast.success('Ставку оновлено');
    },
    onError: () => {
      toast.error('Помилка при оновленні ставки');
    },
  });

  // --- Дані для касира ---
  const sessions = mySessionsData?.sessions || [];
  const totalHours = mySessionsData?.total_hours || 0;
  const hourlyRate = mySessionsData?.hourly_rate;

  // --- Форматування ---
  const formatHours = (hours: number | null | undefined): string => {
    if (hours == null) return '—';
    return hours.toFixed(2) + ' год';
  };

  const formatTime = (dateStr: string): string => {
    const d = new Date(dateStr);
    return d.toLocaleTimeString('uk-UA', { hour: '2-digit', minute: '2-digit' });
  };

  const formatDateShort = (dateStr: string): string => {
    const d = new Date(dateStr);
    return d.toLocaleDateString('uk-UA', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
    });
  };

  // --- Обробник збереження ставки ---
  const handleSaveRate = (userId: string) => {
    const raw = editingRate[userId];
    if (raw === undefined || raw === '') return;
    const rate = parseFloat(raw);
    if (isNaN(rate) || rate < 0) {
      toast.error('Введіть коректну ставку');
      return;
    }
    rateMutation.mutate({ userId, rate });
  };

  // --- Рендер селектора місяця/року ---
  const renderMonthYearSelector = () => (
    <div className="flex items-center gap-3">
      <div className="flex items-center gap-2">
        <Calendar className="w-5 h-5 text-primary-600 flex-shrink-0" />
        <div className="w-40">
          <Select
            options={monthOptions}
            value={selectedMonth}
            onChange={(e) => setSelectedMonth(Number(e.target.value))}
          />
        </div>
      </div>
      <div className="w-32">
        <Select
          options={yearOptions}
          value={selectedYear}
          onChange={(e) => setSelectedYear(Number(e.target.value))}
        />
      </div>
    </div>
  );

  // --- Адмін: таблиця звіту ---
  const renderAdminView = () => {
    if (reportLoading) {
      return (
        <div className="flex justify-center py-12">
          <Spinner size="lg" />
        </div>
      );
    }

    if (reportError) {
      return (
        <div className="text-center py-12 text-danger-600">
          <p>Помилка завантаження звіту</p>
        </div>
      );
    }

    const items = reportData?.items || [];

    if (items.length === 0) {
      return (
        <div className="text-center py-12 text-gray-500 dark:text-gray-400">
          <Clock className="w-12 h-12 mx-auto mb-3 opacity-50" />
          <p className="text-lg font-medium">Немає даних за обраний період</p>
          <p className="text-sm mt-1">За обраний місяць не знайдено робочих сесій</p>
        </div>
      );
    }

    return (
      <div className="card overflow-hidden">
        <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Звіт за {monthNames[selectedMonth - 1].toLowerCase()} {selectedYear}
          </h3>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="bg-gray-50 dark:bg-slate-700/50 border-b border-gray-200 dark:border-slate-700">
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Касир
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Всього годин
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Ставка (грн/год)
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  Зарплата
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
              {items.map((item) => {
                const isEditing = editingRate[item.user_id] !== undefined;
                const displayRate = isEditing
                  ? editingRate[item.user_id]
                  : (item.hourly_rate != null ? item.hourly_rate.toFixed(2) : '');

                return (
                  <tr
                    key={item.user_id}
                    className="hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors"
                  >
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-3">
                        <div className="w-8 h-8 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center flex-shrink-0">
                          <User className="w-4 h-4 text-primary-600 dark:text-primary-400" />
                        </div>
                        <span className="text-sm font-medium text-gray-900 dark:text-white">
                          {item.user_name}
                        </span>
                      </div>
                    </td>
                    <td className="px-4 py-3 text-sm text-gray-700 dark:text-gray-300">
                      {formatHours(item.total_hours)}
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-2 max-w-[180px]">
                        <input
                          type="number"
                          step="0.01"
                          min="0"
                          value={displayRate}
                          onChange={(e) => {
                            setEditingRate((prev) => ({
                              ...prev,
                              [item.user_id]: e.target.value,
                            }));
                          }}
                          onBlur={() => handleSaveRate(item.user_id)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') {
                              handleSaveRate(item.user_id);
                            }
                          }}
                          className="w-full px-2 py-1.5 border border-gray-300 dark:border-slate-600 rounded-md bg-white dark:bg-slate-700 text-gray-900 dark:text-white text-sm focus:ring-2 focus:ring-primary-500 focus:border-transparent"
                          placeholder="0.00"
                        />
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <span className="text-sm font-semibold text-gray-900 dark:text-white">
                        {item.salary != null ? formatCurrency(item.salary) : (
                          item.hourly_rate != null && item.total_hours > 0
                            ? formatCurrency(item.total_hours * item.hourly_rate)
                            : '—'
                        )}
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    );
  };

  // --- Касир: перегляд власних сесій ---
  const renderCashierView = () => {
    if (sessionsLoading) {
      return (
        <div className="flex justify-center py-12">
          <Spinner size="lg" />
        </div>
      );
    }

    if (sessionsError) {
      return (
        <div className="text-center py-12 text-danger-600">
          <p>Помилка завантаження даних</p>
        </div>
      );
    }

    return (
      <div className="space-y-6">
        {/* Картка з загальними годинами */}
        <div className="card p-6">
          <div className="flex items-center gap-6">
            <div className="w-16 h-16 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center">
              <Clock className="w-8 h-8 text-primary-600 dark:text-primary-400" />
            </div>
            <div>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                Відпрацьовано за {monthNames[selectedMonth - 1].toLowerCase()}
              </p>
              <p className="text-3xl font-bold text-gray-900 dark:text-white mt-1">
                {formatHours(totalHours)}
              </p>
              {hourlyRate != null && (
                <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">
                  Ставка: {formatCurrency(hourlyRate)}/год
                </p>
              )}
            </div>
          </div>
        </div>

        {/* Таблиця сесій */}
        <div className="card overflow-hidden">
          <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Робочі сесії
            </h3>
          </div>
          {sessions.length === 0 ? (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              <Clock className="w-12 h-12 mx-auto mb-3 opacity-50" />
              <p className="text-lg font-medium">Немає сесій за обраний місяць</p>
              <p className="text-sm mt-1">Увійдіть в систему, щоб розпочати робочу сесію</p>
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
                      Увійшов
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Вийшов
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Тривалість
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {sessions.map((session) => (
                    <tr
                      key={session.id}
                      className="hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors"
                    >
                      <td className="px-4 py-3 text-sm text-gray-700 dark:text-gray-300">
                        {formatDateShort(session.login_time)}
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-700 dark:text-gray-300">
                        {formatTime(session.login_time)}
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-700 dark:text-gray-300">
                        {session.logout_time ? formatTime(session.logout_time) : (
                          <span className="text-green-600 font-medium">Активна</span>
                        )}
                      </td>
                      <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-white">
                        {formatHours(session.duration_hours)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="space-y-6">
      {/* Заголовок */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Clock className="w-6 h-6 text-primary-600" />
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
            {isAdmin ? 'Облік робочого часу' : 'Мій робочий час'}
          </h1>
        </div>
        {renderMonthYearSelector()}
      </div>

      {/* Контент */}
      {isAdmin ? renderAdminView() : renderCashierView()}
    </div>
  );
};

export default WorkTimePage;
