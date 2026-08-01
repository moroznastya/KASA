import React, { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ArrowLeft,
  Settings,
  Wifi,
  WifiOff,
  Clock,
  RefreshCw,
  PlayCircle,
  StopCircle,
  FileCheck2,
  AlertTriangle,
  Loader2,
  Receipt,
} from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { usePrroStore, startPrroStatusPolling } from '@/store/prroStore';
import { getPrroErrorMessage } from '@/types/prro';
import { formatCurrency } from '@/utils/format';
import { useBackNavigation } from '@/hooks/useBackNavigation';

/** Форматування дати/часу */
const formatDateTime = (value: string | null | undefined): string => {
  if (!value) return '—';
  const d = new Date(value);
  return d.toLocaleDateString('uk-UA', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
};

/** Чи зміна відкрита більше 24 годин */
function isShiftOver24h(openedAt: string | null | undefined): boolean {
  if (!openedAt) return false;
  const opened = new Date(openedAt).getTime();
  if (isNaN(opened)) return false;
  return Date.now() - opened > 24 * 60 * 60 * 1000;
}

const PrroPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();

  const {
    settings,
    status,
    shifts,
    queue,
    loading,
    error,
    openingShift,
    closingShift,
    syncing,
    loadAll,
    openShift,
    closeShift,
    sync,
    clearError,
  } = usePrroStore();

  // Авто-оновлення статусу кожні 30 секунд
  useEffect(() => {
    const stopPolling = startPrroStatusPolling();
    return stopPolling;
  }, []);

  // Первинне завантаження
  useEffect(() => {
    loadAll();
  }, [loadAll]);

  const handleCloseShift = async () => {
    if (!status?.open_shift) return;
    const confirmed = window.confirm(
      'Закрити зміну ПРРО? Буде сформовано Z-звіт. Після закриття новий продаж вимагатиме відкриття нової зміни.'
    );
    if (!confirmed) return;
    await closeShift();
  };

  const lastOpenShift = shifts.find((s) => s.status === 'open');
  const shiftOver24h = isShiftOver24h(lastOpenShift?.opened_at);

  return (
    <div className="max-w-6xl mx-auto space-y-6">
      {/* Заголовок */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={goBack}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">ПРРО</h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Програмний РРО — статус, зміни, фіскалізація
            </p>
          </div>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => navigate('/settings/prro')} icon={<Settings className="w-4 h-4" />}>
            Налаштування
          </Button>
          <Button variant="secondary" onClick={loadAll} isLoading={loading} icon={<RefreshCw className="w-4 h-4" />}>
            Оновити
          </Button>
        </div>
      </div>

      {/* Помилка ПРРО */}
      {error && (
        <div className="flex items-center gap-3 px-4 py-3 rounded-xl border border-danger-200 dark:border-danger-700 bg-danger-50 dark:bg-danger-900/20">
          <AlertTriangle className="w-5 h-5 text-danger-600 flex-shrink-0" />
          <p className="text-sm font-medium text-danger-700 dark:text-danger-400 flex-1">
            {getPrroErrorMessage(error)}
          </p>
          <button
            onClick={clearError}
            className="text-danger-400 hover:text-danger-600 text-xs font-medium"
          >
            Закрити
          </button>
        </div>
      )}

      {/* ─── Картка статусу ───────────────────────────────────────────── */}
      <div className="card p-6">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
            Статус ПРРО
          </h3>
          {status && (
            <div className="flex gap-2">
              {status.online ? (
                <Badge variant="success">
                  <Wifi className="w-3 h-3 mr-1" /> Онлайн
                </Badge>
              ) : (
                <Badge variant="danger">
                  <WifiOff className="w-3 h-3 mr-1" /> Офлайн
                </Badge>
              )}
              {status.open_shift ? (
                <Badge variant="primary">
                  <PlayCircle className="w-3 h-3 mr-1" /> Зміна відкрита
                </Badge>
              ) : (
                <Badge variant="warning">
                  <StopCircle className="w-3 h-3 mr-1" /> Зміна закрита
                </Badge>
              )}
            </div>
          )}
        </div>

        {loading && !status ? (
          <div className="flex justify-center py-8">
            <Loader2 className="w-8 h-8 animate-spin text-primary-500" />
          </div>
        ) : status ? (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            <div className="p-4 rounded-xl bg-gray-50 dark:bg-slate-800/50">
              <p className="text-xs text-gray-400 mb-1">Номер зміни</p>
              <p className="text-lg font-bold text-gray-900 dark:text-gray-100">
                {lastOpenShift ? `№${lastOpenShift.shift_number}` : '—'}
              </p>
            </div>
            <div className="p-4 rounded-xl bg-gray-50 dark:bg-slate-800/50">
              <p className="text-xs text-gray-400 mb-1">Підписант</p>
              <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate" title={status.last_signer || ''}>
                {status.last_signer || '—'}
              </p>
            </div>
            <div className="p-4 rounded-xl bg-gray-50 dark:bg-slate-800/50">
              <p className="text-xs text-gray-400 mb-1">Фіскальний номер</p>
              <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                {status.fn || settings?.prro_fn || '—'}
              </p>
            </div>
            <div className="p-4 rounded-xl bg-gray-50 dark:bg-slate-800/50">
              <p className="text-xs text-gray-400 mb-1">Назва / адреса</p>
              <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate" title={`${status.name || ''} ${status.addr || ''}`}>
                {status.name || '—'}
              </p>
            </div>
          </div>
        ) : (
          <p className="text-sm text-gray-400 py-4 text-center">Немає даних — налаштуйте ПРРО</p>
        )}

        {/* Попередження про зміну > 24 год */}
        {shiftOver24h && (
          <div className="mt-4 flex items-center gap-3 px-4 py-3 rounded-xl border border-warning-200 dark:border-warning-700 bg-warning-50 dark:bg-warning-900/20">
            <AlertTriangle className="w-5 h-5 text-warning-600 flex-shrink-0" />
            <p className="text-sm font-medium text-warning-700 dark:text-warning-400">
              Зміна відкрита понад 24 години — необхідно закрити (Z-звіт)!
            </p>
          </div>
        )}

        {/* Кнопки дій */}
        <div className="flex flex-wrap gap-3 mt-6 pt-4 border-t border-gray-200 dark:border-slate-700">
          {!status?.open_shift ? (
            <Button
              onClick={openShift}
              isLoading={openingShift}
              icon={<PlayCircle className="w-4 h-4" />}
            >
              Відкрити зміну
            </Button>
          ) : (
            <Button
              variant="danger"
              onClick={handleCloseShift}
              isLoading={closingShift}
              icon={<StopCircle className="w-4 h-4" />}
            >
              Закрити зміну (Z-звіт)
            </Button>
          )}
          <Button
            variant="secondary"
            onClick={sync}
            isLoading={syncing}
            icon={<RefreshCw className="w-4 h-4" />}
          >
            Синхронізувати чергу
          </Button>
        </div>
      </div>

      {/* ─── Таблиця змін ─────────────────────────────────────────────── */}
      <div className="card overflow-hidden">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
            Зміни ПРРО
          </h3>
          <Badge variant="default">{shifts.length}</Badge>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="bg-gray-50 dark:bg-slate-800/50">
                <th className="table-header">№</th>
                <th className="table-header">Відкрита</th>
                <th className="table-header">Закрита</th>
                <th className="table-header">Підписант</th>
                <th className="table-header text-center">Чеків</th>
                <th className="table-header text-right">Сума</th>
                <th className="table-header text-center">Статус</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
              {shifts.length === 0 ? (
                <tr>
                  <td colSpan={7} className="px-6 py-8 text-center text-gray-400 text-sm">
                    Зміни ще не відкривались
                  </td>
                </tr>
              ) : (
                shifts.map((shift) => (
                  <tr key={shift.id} className="hover:bg-gray-50 dark:hover:bg-slate-700/30 transition-colors">
                    <td className="table-cell font-medium">{shift.shift_number}</td>
                    <td className="table-cell">{formatDateTime(shift.opened_at)}</td>
                    <td className="table-cell">{formatDateTime(shift.closed_at)}</td>
                    <td className="table-cell">{shift.signer_name || '—'}</td>
                    <td className="table-cell text-center">{shift.receipt_count}</td>
                    <td className="table-cell text-right">{formatCurrency(Number(shift.total_amount) || 0)}</td>
                    <td className="table-cell text-center">
                      {shift.status === 'open' ? (
                        <Badge variant="primary">Відкрита</Badge>
                      ) : (
                        <Badge variant="default">Закрита</Badge>
                      )}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* ─── Журнал черги ─────────────────────────────────────────────── */}
      <div className="card overflow-hidden">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
            Журнал черги фіскалізації
          </h3>
          <Badge variant={queue.some((q) => q.status === 'failed') ? 'danger' : 'default'}>
            {queue.length}
          </Badge>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="bg-gray-50 dark:bg-slate-800/50">
                <th className="table-header">Документ</th>
                <th className="table-header">Тип</th>
                <th className="table-header">Лок. №</th>
                <th className="table-header text-center">Статус</th>
                <th className="table-header">Помилка</th>
                <th className="table-header">Час</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
              {queue.length === 0 ? (
                <tr>
                  <td colSpan={6} className="px-6 py-8 text-center text-gray-400 text-sm">
                    Черга порожня — всі документи синхронізовано
                  </td>
                </tr>
              ) : (
                queue.map((item) => (
                  <tr key={item.id} className="hover:bg-gray-50 dark:hover:bg-slate-700/30 transition-colors">
                    <td className="table-cell">
                      <span className="flex items-center gap-1.5">
                        <Receipt className="w-3.5 h-3.5 text-gray-400" />
                        {item.receipt_id ? item.receipt_id.slice(0, 8) : '—'}
                      </span>
                    </td>
                    <td className="table-cell">{item.check_type}</td>
                    <td className="table-cell">{item.local_number}</td>
                    <td className="table-cell text-center">
                      {item.status === 'sent' && <Badge variant="success">Відправлено</Badge>}
                      {item.status === 'pending' && <Badge variant="warning">Очікує</Badge>}
                      {item.status === 'failed' && <Badge variant="danger">Помилка</Badge>}
                    </td>
                    <td className="table-cell text-xs text-danger-500 max-w-[200px] truncate" title={item.error || ''}>
                      {item.error ? getPrroErrorMessage(item.error) : '—'}
                    </td>
                    <td className="table-cell text-xs text-gray-400">
                      {item.sent_at ? formatDateTime(item.sent_at) : formatDateTime(item.created_at)}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Додаткова інформація */}
      <div className="flex items-center gap-2 text-xs text-gray-400">
        <Clock className="w-3.5 h-3.5" />
        Статус ПРРО оновлюється автоматично кожні 30 секунд
      </div>
    </div>
  );
};

export default PrroPage;
