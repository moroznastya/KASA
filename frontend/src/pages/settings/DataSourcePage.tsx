import React, { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  Database,
  Download,
  FileArchive,
  Info,
  KeyRound,
  Pencil,
  Plug,
  Plus,
  Power,
  RefreshCw,
  Server,
  Trash2,
  Upload,
} from 'lucide-react';
import toast from 'react-hot-toast';
import { useNavigate } from 'react-router-dom';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { Modal } from '@/components/ui/Modal';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { dbSourcesService, extractDetail } from '@/services/dbSourcesService';
import type { DbSourceView } from '@/types/dbSources';

const formatBytes = (b: number): string => {
  if (b < 1024) return `${b} Б`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} КБ`;
  return `${(b / (1024 * 1024)).toFixed(2)} МБ`;
};

interface SourceForm {
  id: string;
  label: string;
  host: string;
  port: string;
  database: string;
  user: string;
  password: string;
}

const emptyForm: SourceForm = {
  id: '',
  label: '',
  host: '127.0.0.1',
  port: '5432',
  database: '',
  user: '',
  password: '',
};

// ═══════════════════════════════════════════════════════════════
// СТОРІНКА: Налаштування → Джерело даних (Етап 3 адмін-панелі)
// ═══════════════════════════════════════════════════════════════
const DataSourcePage: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['db-sources'],
    queryFn: () => dbSourcesService.list(),
  });
  const { data: dumps } = useQuery({
    queryKey: ['db-dumps'],
    queryFn: () => dbSourcesService.listDumps(),
  });

  const sources = data?.sources ?? [];
  const active = data?.active ?? null;
  const activeSource = sources.find((s) => s.id === active) ?? null;

  // ── Форма (створення/редагування) ────────────
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<DbSourceView | null>(null);
  const [form, setForm] = useState<SourceForm>(emptyForm);

  // ── Тест з'єднання ───────────────────────────
  const [testingId, setTestingId] = useState<string | null>(null);

  // ── Активація / видалення (confirm) ───────────
  const [activateTarget, setActivateTarget] = useState<DbSourceView | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<DbSourceView | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);

  // ── Експорт / імпорт ─────────────────────────
  const [exporting, setExporting] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importSource, setImportSource] = useState('');
  const [importFile, setImportFile] = useState('');
  const [importClean, setImportClean] = useState(false);
  const [importing, setImporting] = useState(false);

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['db-sources'] });
    queryClient.invalidateQueries({ queryKey: ['db-dumps'] });
  };

  const openCreate = () => {
    setEditing(null);
    setForm(emptyForm);
    setFormOpen(true);
  };

  const openEdit = (s: DbSourceView) => {
    setEditing(s);
    setForm({
      id: s.id,
      label: s.label,
      host: s.host,
      port: String(s.port),
      database: s.database,
      user: s.user,
      password: '',
    });
    setFormOpen(true);
  };

  const formValid = (): boolean =>
    !!form.id.trim() &&
    !!form.host.trim() &&
    !!form.database.trim() &&
    !!form.user.trim() &&
    Number(form.port) > 0 &&
    Number(form.port) < 65536;

  const saveMutation = useMutation({
    mutationFn: () => {
      const port = Number(form.port);
      if (editing) {
        const body: Record<string, unknown> = {};
        if (form.label.trim() !== editing.label) body.label = form.label.trim();
        if (form.host.trim() !== editing.host) body.host = form.host.trim();
        if (port !== editing.port) body.port = port;
        if (form.database.trim() !== editing.database) body.database = form.database.trim();
        if (form.user.trim() !== editing.user) body.user = form.user.trim();
        // Порожній рядок у полі пароля при редагуванні = «залишити без змін».
        if (form.password !== '') body.password = form.password;
        return dbSourcesService.update(editing.id, body);
      }
      return dbSourcesService.create({
        id: form.id.trim(),
        label: form.label.trim() || undefined,
        host: form.host.trim(),
        port,
        database: form.database.trim(),
        user: form.user.trim(),
        password: form.password || undefined,
      });
    },
    onSuccess: () => {
      toast.success(editing ? 'Джерело оновлено' : 'Джерело додано');
      setFormOpen(false);
      invalidate();
    },
    onError: (err: unknown) => toast.error(extractDetail(err)),
  });

  const testSource = async (s: DbSourceView) => {
    setTestingId(s.id);
    try {
      const r = await dbSourcesService.test(s.id);
      toast.success(`З'єднання успішне: ${r.latency_ms} мс (TCP + SELECT 1)`);
    } catch (err) {
      toast.error(extractDetail(err));
    } finally {
      setTestingId(null);
    }
  };

  const confirmActivate = async () => {
    if (!activateTarget) return;
    setBusyAction(`activate:${activateTarget.id}`);
    try {
      const r = await dbSourcesService.activate(activateTarget.id);
      toast.success(r.message, { duration: 6000 });
      setActivateTarget(null);
      invalidate();
    } catch (err) {
      toast.error(extractDetail(err));
      setActivateTarget(null);
    } finally {
      setBusyAction(null);
    }
  };

  const confirmDelete = async () => {
    if (!deleteTarget) return;
    setBusyAction(`delete:${deleteTarget.id}`);
    try {
      await dbSourcesService.remove(deleteTarget.id);
      toast.success(`Джерело «${deleteTarget.label}» видалено`);
      setDeleteTarget(null);
      invalidate();
    } catch (err) {
      toast.error(extractDetail(err));
      setDeleteTarget(null);
    } finally {
      setBusyAction(null);
    }
  };

  const doExport = async () => {
    setExporting(true);
    try {
      const r = await dbSourcesService.exportDump();
      toast.success(`Дамп створено: ${r.file} (${formatBytes(r.size_bytes)})`);
      queryClient.invalidateQueries({ queryKey: ['db-dumps'] });
    } catch (err) {
      toast.error(extractDetail(err));
    } finally {
      setExporting(false);
    }
  };

  const doImport = async () => {
    if (!importSource || !importFile) {
      toast.error('Оберіть джерело-приймач і файл дампу');
      return;
    }
    setImporting(true);
    try {
      const r = await dbSourcesService.importDump({
        source_id: importSource,
        file: importFile,
        clean: importClean,
      });
      toast.success(`Дамп «${r.file}» імпортовано у джерело «${importSource}»`);
      setImportOpen(false);
      queryClient.invalidateQueries({ queryKey: ['db-dumps'] });
    } catch (err) {
      toast.error(extractDetail(err));
    } finally {
      setImporting(false);
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Spinner />
      </div>
    );
  }
  if (isError) {
    return (
      <div className="flex flex-col items-center gap-4 py-16 text-gray-500 dark:text-gray-400">
        <AlertTriangle className="w-10 h-10" />
        <p>Не вдалося завантажити джерела даних</p>
        <Button variant="secondary" onClick={() => refetch()}>
          <RefreshCw className="w-4 h-4" /> Спробувати ще
        </Button>
      </div>
    );
  }

  const importOptions: SelectOption[] = sources.map((s) => ({
    value: s.id,
    label: `${s.label} (${s.host}:${s.port}/${s.database})`,
  }));
  const dumpOptions: SelectOption[] = (dumps ?? []).map((d) => ({
    value: d.file,
    label: `${d.file} — ${formatBytes(d.size_bytes)} (${d.modified_at})`,
  }));

  return (
    <div className="max-w-5xl mx-auto px-6 py-8 space-y-6">
      <button
        onClick={() => navigate('/settings')}
        className="inline-flex items-center gap-1 text-sm text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
      >
        <ArrowLeft className="w-4 h-4" /> Назад до налаштувань
      </button>

      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
            <Database className="w-6 h-6 text-primary-600 dark:text-primary-400" />
            Джерело даних
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Основна PostgreSQL-база сервісу. Файл конфігурації:{' '}
            <code className="px-1.5 py-0.5 bg-gray-100 dark:bg-slate-700 rounded text-xs">
              {data?.config_path}
            </code>
          </p>
        </div>
        <Button onClick={openCreate} icon={<Plus className="w-4 h-4" />}>
          Додати джерело
        </Button>
      </div>

      {/* Інформаційний блок: поведінка активації (stability_first) */}
      <div className="flex gap-3 p-4 rounded-xl border border-amber-200 bg-amber-50 dark:border-amber-900/40 dark:bg-amber-900/10 text-sm text-amber-800 dark:text-amber-200">
        <Info className="w-5 h-5 flex-shrink-0 mt-0.5" />
        <div className="space-y-1">
          <p>
            <b>Перемикання — після перезапуску сервісу</b> (гаряче перепідключення
            пулів навмисно не реалізовано заради стабільності). Перед активацією сервер
            обов'язково перевіряє з'єднання з новим джерелом.
          </p>
          <p>
            <b>Паролі шифруються</b> (AES-256-GCM; ключ — env{' '}
            <code className="px-1 rounded bg-amber-100 dark:bg-amber-900/40">TORGASHKA_DBKEY</code>{' '}
            або файл <code className="px-1 rounded bg-amber-100 dark:bg-amber-900/40">.dbkey</code>{' '}
            поряд із конфігом, права 0600). Пароль ніколи не зберігається у відкритому
            вигляді.
          </p>
        </div>
      </div>

      {/* Список джерел */}
      <div className="space-y-3">
        {sources.length === 0 && (
          <div className="text-center py-10 text-gray-500 dark:text-gray-400 border border-dashed border-gray-300 dark:border-slate-600 rounded-xl">
            Джерел ще немає. Додайте перше джерело даних.
          </div>
        )}
        {sources.map((s) => (
          <div
            key={s.id}
            className={`bg-white dark:bg-slate-800 rounded-xl border shadow-sm p-4 ${
              s.is_active
                ? 'border-success-300 dark:border-success-700'
                : 'border-gray-200 dark:border-slate-700'
            }`}
          >
            <div className="flex items-start justify-between gap-3 flex-wrap">
              <div className="min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <h3 className="font-semibold text-gray-900 dark:text-white">{s.label}</h3>
                  {s.is_active && (
                    <Badge variant="success">
                      <CheckCircle2 className="w-3 h-3 mr-1 inline" /> Активне
                    </Badge>
                  )}
                  <Badge variant="default">{s.id}</Badge>
                </div>
                <div className="mt-1 text-sm text-gray-600 dark:text-gray-300 flex items-center gap-1.5 flex-wrap">
                  <Server className="w-3.5 h-3.5 text-gray-400" />
                  {s.host}:{s.port} / {s.database}
                  <span className="text-gray-400 dark:text-gray-500">·</span>
                  користувач: {s.user}
                  <KeyRound
                    className={`w-3.5 h-3.5 ${s.has_password ? 'text-emerald-500' : 'text-gray-300 dark:text-gray-600'}`}
                  />
                </div>
                {s.is_active && activeSource && (
                  <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                    Застосується після перезапуску сервісу.
                  </p>
                )}
              </div>
              <div className="flex items-center gap-1.5 flex-wrap flex-shrink-0">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => testSource(s)}
                  disabled={testingId === s.id || !!busyAction}
                  isLoading={testingId === s.id}
                  icon={<Plug className="w-3.5 h-3.5" />}
                >
                  Перевірити
                </Button>
                {!s.is_active && (
                  <Button
                    variant="success"
                    size="sm"
                    onClick={() => setActivateTarget(s)}
                    disabled={!!busyAction}
                    icon={<Power className="w-3.5 h-3.5" />}
                  >
                    Зробити активним
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => openEdit(s)}
                  disabled={!!busyAction}
                  icon={<Pencil className="w-3.5 h-3.5" />}
                >
                  Редагувати
                </Button>
                <Button
                  variant="danger"
                  size="sm"
                  onClick={() => setDeleteTarget(s)}
                  disabled={s.is_active || !!busyAction}
                  title={s.is_active ? 'Активне джерело не можна видалити' : 'Видалити джерело'}
                  icon={<Trash2 className="w-3.5 h-3.5" />}
                />
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* Дамп активного джерела */}
      <div className="bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700 p-5 space-y-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h2 className="font-semibold text-gray-900 dark:text-white flex items-center gap-2">
              <FileArchive className="w-5 h-5 text-primary-600 dark:text-primary-400" />
              Резервне копіювання (дамп активної БД)
            </h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
              pg_dump (plain SQL) поточного активного джерела{activeSource ? ` — «${activeSource.label}»` : ''}.{' '}
              Файли дампів зберігаються в <code className="px-1 rounded bg-gray-100 dark:bg-slate-700 text-xs">dumps/</code> біля конфігу.
            </p>
          </div>
          <Button
            onClick={doExport}
            isLoading={exporting}
            disabled={!activeSource}
            icon={<Download className="w-4 h-4" />}
          >
            Експортувати дамп
          </Button>
        </div>

        <div className="flex items-start gap-3 p-3 rounded-lg border border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-900/40 text-sm text-gray-600 dark:text-gray-300">
          <Upload className="w-4 h-4 flex-shrink-0 mt-0.5" />
          <div className="space-y-1">
            <p>
              <b>Імпорт дампу</b> — у вибране збережене джерело (нова порожня БД або наявна).
              БД призначення має існувати на сервері; для відновлення в нову БД спершу
              створіть її та додайте як джерело.
            </p>
            <p className="text-xs text-gray-400 dark:text-gray-500">
              Формат — plain SQL (сумісний із psql будь-якої версії; pg_dump ≥17 не ламає імпорт на PG15/16).
            </p>
          </div>
        </div>

        {dumps && dumps.length > 0 && (
          <div>
            <div className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2 uppercase tracking-wide">
              Збережені дампи ({dumps.length})
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
              {dumps.map((d) => (
                <div
                  key={d.file}
                  className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-gray-50 dark:bg-slate-900/40 border border-gray-200 dark:border-slate-700 text-sm"
                >
                  <span className="truncate text-gray-700 dark:text-gray-200" title={d.file}>
                    {d.file}
                  </span>
                  <span className="text-gray-400 dark:text-gray-500 whitespace-nowrap text-xs">
                    {formatBytes(d.size_bytes)} · {d.modified_at}
                  </span>
                </div>
              ))}
            </div>
            <Button
              variant="secondary"
              size="sm"
              className="mt-3"
              onClick={() => {
                setImportFile(dumps[0]?.file ?? '');
                setImportSource(sources.find((s) => !s.is_active)?.id ?? '');
                setImportClean(false);
                setImportOpen(true);
              }}
              disabled={sources.length === 0 || dumps.length === 0}
              icon={<Upload className="w-3.5 h-3.5" />}
            >
              Імпортувати дамп…
            </Button>
          </div>
        )}
      </div>

      {/* Модалка: додати/редагувати джерело */}
      <Modal
        isOpen={formOpen}
        onClose={() => setFormOpen(false)}
        title={editing ? `Редагувати джерело «${editing.label}»` : 'Додати джерело даних'}
        size="lg"
      >
        <div className="space-y-4">
          {!editing && (
            <Input
              label="ID (ключ у db_sources.toml)"
              value={form.id}
              onChange={(e) => setForm({ ...form, id: e.target.value })}
              placeholder="primary / backup_restore / main_office"
              helperText="Літери, цифри, _ та - (до 64 символів)"
            />
          )}
          <Input
            label="Назва"
            value={form.label}
            onChange={(e) => setForm({ ...form, label: e.target.value })}
            placeholder="Основна БД сервера"
          />
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <Input
              label="Хост"
              value={form.host}
              onChange={(e) => setForm({ ...form, host: e.target.value })}
              placeholder="127.0.0.1"
            />
            <Input
              label="Порт"
              type="number"
              value={form.port}
              onChange={(e) => setForm({ ...form, port: e.target.value })}
            />
          </div>
          <Input
            label="База даних"
            value={form.database}
            onChange={(e) => setForm({ ...form, database: e.target.value })}
            placeholder="pos_system"
          />
          <Input
            label="Користувач"
            value={form.user}
            onChange={(e) => setForm({ ...form, user: e.target.value })}
            placeholder="postgres"
          />
          <Input
            label="Пароль"
            type="password"
            value={form.password}
            onChange={(e) => setForm({ ...form, password: e.target.value })}
            placeholder={editing ? 'Залишити без змін' : 'Пароль БД'}
            helperText={
              editing
                ? 'Порожнє поле = пароль не змінюється. Пароль буде зашифровано (AES-256-GCM).'
                : 'Пароль буде зашифровано (AES-256-GCM) перед записом у файл.'
            }
          />
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="secondary" onClick={() => setFormOpen(false)}>
              Скасувати
            </Button>
            <Button
              onClick={() => saveMutation.mutate()}
              isLoading={saveMutation.isPending}
              disabled={!formValid()}
            >
              {editing ? 'Зберегти' : 'Додати'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Підтвердження активації */}
      <ConfirmDialog
        isOpen={!!activateTarget}
        onClose={() => setActivateTarget(null)}
        onConfirm={confirmActivate}
        title="Зробити активним джерелом?"
        message={
          activateTarget
            ? `З'єднання з «${activateTarget.label}» буде перевірено (TCP + SELECT 1). Після збереження активного джерела каси та сервіс працюватимуть з новою базою ПІСЛЯ ПЕРЕЗАПУСКУ; каси на старому джерелі тимчасово втратять зв'язок після перемикання. Продовжити?`
            : ''
        }
        confirmText="Перевірити та активувати"
        variant="warning"
        isLoading={busyAction?.startsWith('activate:')}
      />

      {/* Підтвердження видалення */}
      <ConfirmDialog
        isOpen={!!deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onConfirm={confirmDelete}
        title="Видалити джерело?"
        message={
          deleteTarget
            ? `Джерело «${deleteTarget.label}» (${deleteTarget.host}:${deleteTarget.port}/${deleteTarget.database}) буде видалено з db_sources.toml. Сама база даних на сервері НЕ видаляється.`
            : ''
        }
        confirmText="Видалити"
        variant="danger"
        isLoading={busyAction?.startsWith('delete:')}
      />

      {/* Модалка імпорту */}
      <Modal
        isOpen={importOpen}
        onClose={() => setImportOpen(false)}
        title="Імпортувати дамп"
        size="lg"
      >
        <div className="space-y-4">
          <div className="flex items-start gap-2 p-3 rounded-lg bg-amber-50 dark:bg-amber-900/10 border border-amber-200 dark:border-amber-900/40 text-sm text-amber-800 dark:text-amber-200">
            <AlertTriangle className="w-4 h-4 flex-shrink-0 mt-0.5" />
            <p>
              Дані у БД призначення будуть доповнені/відновлені з дампу. Для відновлення з
              нуля використайте прапорець «очистити схему» нижче або нову порожню БД.
            </p>
          </div>
          <Select
            label="Джерело-приймач"
            value={importSource}
            onChange={(e) => setImportSource(e.target.value)}
            options={importOptions}
            placeholder="Оберіть джерело…"
          />
          <Select
            label="Файл дампу"
            value={importFile}
            onChange={(e) => setImportFile(e.target.value)}
            options={dumpOptions}
            placeholder="Оберіть файл…"
          />
          <label className="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-300 cursor-pointer">
            <input
              type="checkbox"
              checked={importClean}
              onChange={(e) => setImportClean(e.target.checked)}
              className="mt-0.5"
            />
            <span>
              <b>Очистити схему перед імпортом</b> (DROP SCHEMA public CASCADE; CREATE SCHEMA) —
              всі наявні дані в БД призначення будуть видалені.
            </span>
          </label>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="secondary" onClick={() => setImportOpen(false)}>
              Скасувати
            </Button>
            <Button
              onClick={doImport}
              isLoading={importing}
              disabled={!importSource || !importFile}
              icon={<Upload className="w-4 h-4" />}
            >
              Імпортувати
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};

export default DataSourcePage;
