import React from 'react';
import { useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  ClipboardList,
  Scale,
} from 'lucide-react';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import {
  getStockReconciliation,
  type StockReconciliationItem,
} from '@/services/stockReconciliationService';

/**
 * Звірка залишків документа: локальний (SQLite каси) vs авторитетний (репліка
 * PostgreSQL) — ADR-0007 §10.3, AT-15.
 *
 * Компонент ЛИШЕ ПОКАЗУЄ обидва числа й дельту. Вирівнювання робить наявний
 * механізм — інвентаризація (`/documents/inventory/new`), нового reconcile-
 * движка немає (заборона §10.3). Джерело даних — `stockReconciliationService`
 * (GET `/local/stock-reconciliation`, `route_local.rs:519`).
 *
 * Самоприховування (важливо для монтування у сторінці документа):
 *   • вузол не standby / документа немає в локальній черзі цієї каси /
 *     немає контексту точки → сервіс повертає `null` → панель не рендериться;
 *   • `items: []` (документ не змінив залишків) — КОРЕКТНИЙ стан, показуємо
 *     пояснення, а не помилку.
 */

export interface StockReconciliationPanelProps {
  /** `client_uuid` документа — той самий id, що в URL документа каси. */
  invoiceId: string;
}

/** Кількість: до 3 знаків (scale 3 у бекенді), без зайвих нулів. */
const formatQty = (value: number | null): string =>
  value === null ? '—' : value.toLocaleString('uk-UA', { maximumFractionDigits: 3 });

/** Дельта зі знаком: `+3`, `-1.5`, `0` (мінус додає сам toLocaleString). */
const formatDelta = (value: number | null): string =>
  value === null
    ? '—'
    : `${value > 0 ? '+' : ''}${value.toLocaleString('uk-UA', { maximumFractionDigits: 3 })}`;

const KIND_LABELS: Record<string, string> = {
  invoice: 'Прибуткова накладна',
  return_invoice: 'Повернення постачальнику',
};

const Row: React.FC<{ item: StockReconciliationItem }> = ({ item }) => {
  // Три різні стани рядка: збіг / розбіжність / немає авторитетного числа.
  const missingAuthoritative = item.authoritative_qty === null;
  const mismatch = item.delta !== null && item.delta !== 0;

  const rowClass = mismatch
    ? 'bg-danger-50 dark:bg-danger-900/20'
    : missingAuthoritative
      ? 'bg-warning-50 dark:bg-warning-900/20'
      : '';

  return (
    <tr className={rowClass}>
      <td className="px-4 py-3">
        <p className="font-medium text-gray-900 dark:text-gray-100" title={item.product_id}>
          {item.name}
        </p>
        {missingAuthoritative && (
          <p className="text-xs text-warning-700 dark:text-warning-400 mt-0.5">
            Товар не є UUID головної БД — авторитетного числа не існує
          </p>
        )}
      </td>
      <td className="px-4 py-3 text-right tabular-nums text-gray-500 dark:text-gray-400">
        {formatQty(item.local_qty)}
      </td>
      <td className="px-4 py-3 text-right tabular-nums text-gray-900 dark:text-gray-100">
        {formatQty(item.authoritative_qty)}
      </td>
      <td className="px-4 py-3 text-right tabular-nums whitespace-nowrap">
        {mismatch ? (
          <span className="inline-flex items-center gap-1 font-semibold text-danger-700 dark:text-danger-300">
            {item.delta! > 0 ? (
              <ArrowUp className="w-3.5 h-3.5" />
            ) : (
              <ArrowDown className="w-3.5 h-3.5" />
            )}
            {formatDelta(item.delta)}
          </span>
        ) : missingAuthoritative ? (
          <span className="font-medium text-warning-700 dark:text-warning-400">
            {formatDelta(item.delta)}
          </span>
        ) : (
          <span className="inline-flex items-center gap-1 font-medium text-success-600 dark:text-success-400">
            <CheckCircle2 className="w-3.5 h-3.5" />
            {formatDelta(item.delta)}
          </span>
        )}
      </td>
    </tr>
  );
};

export const StockReconciliationPanel: React.FC<StockReconciliationPanelProps> = ({
  invoiceId,
}) => {
  const navigate = useNavigate();

  const { data, isLoading } = useQuery({
    queryKey: ['stock-reconciliation', invoiceId],
    queryFn: () => getStockReconciliation(invoiceId),
    enabled: !!invoiceId,
    // 404 — очікувана відповідь (primary-вузол / немає в черзі): без ретраїв.
    retry: false,
    staleTime: 30_000,
  });

  if (isLoading) {
    return (
      <div className="flex justify-center py-6">
        <Spinner size="md" />
      </div>
    );
  }

  // Звірка не застосовна (не standby / документа немає в черзі цієї каси).
  if (!data) return null;

  const { items, summary } = data;
  const mismatching = summary.mismatching > 0;

  return (
    <section className="rounded-xl border border-gray-200 dark:border-slate-700 p-4 sm:p-5 space-y-4">
      {/* ─── Заголовок + підсумок ─────────────────────────────────────── */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex items-center gap-2">
          <Scale className="w-5 h-5 text-gray-400 shrink-0" />
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Звірка залишків після офлайну
          </h3>
          <Badge variant={mismatching ? 'danger' : 'success'}>
            {KIND_LABELS[data.kind] ?? data.kind}
            {data.number ? ` №${data.number}` : ''}
          </Badge>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="px-2 py-1 rounded-full bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300">
            Позицій: {summary.total}
          </span>
          <span className="px-2 py-1 rounded-full bg-success-100 dark:bg-success-900/30 text-success-700 dark:text-success-400">
            Збігається: {summary.matching}
          </span>
          <span
            className={
              mismatching
                ? 'px-2 py-1 rounded-full bg-danger-100 dark:bg-danger-900/30 text-danger-700 dark:text-danger-400 font-semibold'
                : 'px-2 py-1 rounded-full bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300'
            }
          >
            Розбіжностей: {summary.mismatching}
          </span>
        </div>
      </div>

      {/* ─── Попередження §10.3: локальне число — оцінка, не істина ────── */}
      <div className="flex gap-3 rounded-lg border border-warning-200 dark:border-warning-800 bg-warning-50 dark:bg-warning-900/20 p-3">
        <AlertTriangle className="w-5 h-5 text-warning-600 dark:text-warning-400 shrink-0 mt-0.5" />
        <div className="text-sm text-warning-800 dark:text-warning-200 space-y-1">
          <p className="font-semibold">
            «Локальний» залишок — це ОЦІНКА каси, а не істина.
          </p>
          <p>{data.note}</p>
          <p>
            Авторитетне число — з репліки PostgreSQL (
            <span className="font-mono text-xs">{data.authoritative_source}</span>), локальне — з
            черги SQLite каси (<span className="font-mono text-xs">{data.local_source}</span>).
          </p>
          <p>
            Вирівнювати розбіг потрібно{' '}
            <span className="font-semibold">інвентаризацією</span> (перерахунок факту) — окремого
            движка звірки не існує, ця панель лише показує числа.
          </p>
        </div>
      </div>

      {/* ─── Таблиця позицій / порожній стан ──────────────────────────── */}
      {items.length === 0 ? (
        <div className="rounded-lg border border-dashed border-gray-300 dark:border-slate-600 py-8 text-center">
          <CheckCircle2 className="w-5 h-5 mx-auto mb-2 text-gray-400" />
          <p className="text-sm text-gray-600 dark:text-gray-300">
            Документ не змінив залишків — звіряти нічого.
          </p>
          <p className="text-xs text-gray-400 mt-1">Це коректний стан, а не помилка.</p>
        </div>
      ) : (
        <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
          <table className="w-full">
            <thead>
              <tr className="bg-gray-50 dark:bg-slate-800/50">
                <th className="table-header">Товар</th>
                <th className="table-header w-40 text-right">
                  Локальний
                  {data.local_is_estimate && (
                    <span className="ml-1 font-normal text-warning-700 dark:text-warning-400">
                      (оцінка)
                    </span>
                  )}
                </th>
                <th className="table-header w-40 text-right">Авторитетний (PG)</th>
                <th className="table-header w-28 text-right">Дельта</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
              {items.map((item) => (
                <Row key={item.product_id} item={item} />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* ─── Вирівнювання — наявною інвентаризацією ───────────────────── */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-xs text-gray-500 dark:text-gray-400">
          Після перерахунку факту авторитетний залишок оновиться, і дельта стане 0.
        </p>
        <Button
          variant="secondary"
          size="sm"
          icon={<ClipboardList className="w-4 h-4" />}
          onClick={() => navigate('/documents/inventory/new')}
        >
          Вирівняти інвентаризацією
        </Button>
      </div>
    </section>
  );
};

export default StockReconciliationPanel;
