import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  Eye,
  FileClock,
  RefreshCw,
  Search,
} from 'lucide-react';
import { adminAuditService } from '@/services/adminAuditService';
import { Table, Column } from '@/components/ui/Table';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { AUDIT_ACTION_LABELS, AuditLogItem } from '@/types/adminAudit';

/**
 * «Аудит-лог» (Етап 5 адмін-панелі, ТЗ 5.9).
 * Таблиця дій адміністрування з фільтрами (період, автор, тип дії, точка) —
 * ТІЛЬКИ перегляд (API не редагує/не видаляє записи).
 */

const fmtDt = (v: string): string => v.replace('T', ' ').slice(0, 19);

/** Скорочення payload → текст для колонки «Деталі». */
function payloadPreview(p: AuditLogItem): string {
  const parts: string[] = [];
  if (p.action === 'store_updated' && p.payload?.name) {
    parts.push(`нова назва: ${String(p.payload.name)}`);
  }
  if (p.payload?.from && p.payload?.to) {
    parts.push(`${String(p.payload.from)} → ${String(p.payload.to)}`);
  }
  if (p.action === 'activation_code_generated' && p.payload?.code_length) {
    parts.push(`код (${String(p.payload.code_length)} символів)`);
  }
  return parts.join(' · ');
}

const AuditLogPage: React.FC = () => {
  const [author, setAuthor] = useState('');
  const [action, setAction] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [page, setPage] = useState(1);
  const [applied, setApplied] = useState<{
    author: string;
    action: string;
    from: string;
    to: string;
  }>({ author: '', action: '', from: '', to: '' });

  const [data, setData] = useState<{
    items: AuditLogItem[];
    total: number;
    pages: number;
  } | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const filters = useMemo(
    () => ({
      author: applied.author || undefined,
      action: applied.action || undefined,
      from: applied.from || undefined,
      to: applied.to || undefined,
      page,
      size: 25,
    }),
    [applied, page],
  );

  const load = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const res = await adminAuditService.list(filters);
      setData({ items: res.items, total: res.total, pages: res.pages });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Не вдалося завантажити аудит-лог');
    } finally {
      setIsLoading(false);
    }
  }, [filters]);

  useEffect(() => {
    void load();
  }, [load]);

  const applyFilters = () => {
    setPage(1);
    setApplied({ author: author.trim(), action, from, to });
  };

  const columns: Column<AuditLogItem>[] = [
    {
      key: 'created_at',
      header: 'Дата і час',
      render: (it) => (
        <span className="text-gray-700 dark:text-gray-300 whitespace-nowrap font-mono text-xs">
          {fmtDt(it.created_at)}
        </span>
      ),
    },
    {
      key: 'actor_name',
      header: 'Автор',
      render: (it) => (
        <div className="min-w-[130px]">
          <p className="font-medium text-gray-900 dark:text-gray-100">{it.actor_name || '—'}</p>
          <p className="text-xs text-gray-400">{it.actor_login || ''}</p>
        </div>
      ),
    },
    {
      key: 'action',
      header: 'Тип дії',
      render: (it) => (
        <Badge variant="info">{AUDIT_ACTION_LABELS[it.action] || it.action}</Badge>
      ),
    },
    {
      key: 'store_name',
      header: 'Точка',
      render: (it) => it.store_name || <span className="text-gray-400">—</span>,
    },
    {
      key: 'entity',
      header: 'Об\'єкт',
      render: (it) => (
        <span className="text-gray-500 dark:text-gray-400 text-sm">
          {it.entity_type ? `${it.entity_type}${it.entity_id ? ` · ${it.entity_id.slice(0, 8)}…` : ''}` : '—'}
        </span>
      ),
    },
    {
      key: 'payload',
      header: 'Деталі',
      render: (it) => {
        const txt = payloadPreview(it);
        return txt ? (
          <span className="text-gray-500 dark:text-gray-400 text-sm">{txt}</span>
        ) : (
          <span className="text-gray-300 dark:text-gray-600">—</span>
        );
      },
    },
  ];

  return (
    <div className="p-6 max-w-7xl mx-auto space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="p-2 rounded-lg bg-primary-50 dark:bg-primary-900/20 text-primary-600">
            <FileClock className="w-6 h-6" />
          </div>
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Аудит-лог</h1>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
              Журнал адмін-дій по мережі · тільки перегляд
            </p>
          </div>
        </div>
        {error && (
          <button
            onClick={() => void load()}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium text-danger-600 border border-danger-200 dark:border-danger-900 hover:bg-danger-50"
          >
            <RefreshCw className="w-4 h-4" /> Повторити
          </button>
        )}
      </div>

      {/* Фільтри */}
      <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 p-4">
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-5 gap-3 items-end">
          <div>
            <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1 block">
              Автор (ім’я)
            </label>
            <Input
              value={author}
              onChange={(e) => setAuthor(e.target.value)}
              placeholder="Напр., Олена"
            />
          </div>
          <div>
            <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1 block">
              Тип дії
            </label>
            <Select
              value={action}
              onChange={(e) => setAction(e.target.value)}
              placeholder="Усі дії"
              options={[
                { value: '', label: 'Усі дії' },
                ...Object.entries(AUDIT_ACTION_LABELS).map(([value, label]) => ({
                  value,
                  label,
                })),
              ]}
            />
          </div>
          <div>
            <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1 block">
              Від
            </label>
            <Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
          </div>
          <div>
            <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1 block">
              До
            </label>
            <Input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
          </div>
          <Button onClick={applyFilters} className="h-10">
            <Search className="w-4 h-4 mr-2" /> Застосувати
          </Button>
        </div>
      </div>

      {error && (
        <div className="flex items-center gap-3 p-4 rounded-xl bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300 text-sm">
          <AlertTriangle className="w-5 h-5 shrink-0" />
          {error}
        </div>
      )}

      <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 overflow-hidden">
        <div className="px-5 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-2">
          <Eye className="w-5 h-5 text-primary-600" />
          <h2 className="font-semibold text-gray-900 dark:text-gray-100">Записи</h2>
          <Badge variant="primary" className="ml-auto">
            {data ? `${data.total}` : '…'}
          </Badge>
        </div>
        {isLoading ? (
          <div className="flex items-center justify-center py-16">
            <Spinner size="lg" />
          </div>
        ) : (
          <Table<AuditLogItem>
            columns={columns}
            data={data?.items ?? []}
            keyExtractor={(it) => it.id}
            emptyMessage="Немає записів за обраними фільтрами"
            emptyIcon={<FileClock className="w-10 h-10" />}
          />
        )}
      </div>

      {/* Пагінація */}
      {data && data.pages > 1 && (
        <div className="flex items-center justify-center gap-2">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page <= 1}
            className="p-2 rounded-lg border border-gray-200 dark:border-slate-700 disabled:opacity-40 hover:bg-gray-50 dark:hover:bg-slate-700"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>
          <span className="text-sm text-gray-600 dark:text-gray-400 px-2">
            Сторінка {page} з {data.pages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(data.pages, p + 1))}
            disabled={page >= data.pages}
            className="p-2 rounded-lg border border-gray-200 dark:border-slate-700 disabled:opacity-40 hover:bg-gray-50 dark:hover:bg-slate-700"
          >
            <ChevronRight className="w-4 h-4" />
          </button>
        </div>
      )}
    </div>
  );
};

export default AuditLogPage;
