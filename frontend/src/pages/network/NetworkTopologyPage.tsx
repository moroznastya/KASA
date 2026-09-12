import React, { useCallback, useEffect, useState } from 'react';
import {
  Server,
  Laptop,
  RefreshCw,
  Archive,
  Copy,
  Loader2,
  Plus,
  Database,
  Clock,
  HardDrive,
  AlertTriangle,
} from 'lucide-react';
import toast from 'react-hot-toast';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Modal } from '@/components/ui/Modal';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import {
  NetworkNode,
  NodeStatus,
  listNetworkNodes,
  createNetworkNode,
  archiveNetworkNode,
  NodeCreateResult,
} from '@/services/networkNodeService';
import { formatRelativeTime, formatBytes } from '@/utils/format';

/**
 * «Вузли мережі» — топологія реплікації (ЕТАП 15-17, plan-network-replication).
 *
 * Відмінність від «Каси мережі» (DevicesPage — логічні пристрої синхронізації):
 * тут — ФІЗИЧНІ standby-копії БД (pg_basebackup + streaming replication).
 * Кожен standby — повноцінний PostgreSQL-вузол, що приймає трафік офлайн.
 *
 * Дії:
 *  - «Додати вузол» → створює запис + показує одноразовий join-код (TTL 30 хв)
 *    та primary_host_hint для підключення нового комп'ютера.
 *  - Архівувати → вимкнути вузол з мережі (статус archived).
 *  - Примусовий ресинк → перезапуск basebackup для вузла (кнопка на standby).
 */

const STATUS_META: Record<NodeStatus, { label: string; badgeClass: string }> = {
  provisioning: { label: 'Провіжинінг', badgeClass: 'bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-300' },
  syncing: { label: 'Синхронізація', badgeClass: 'bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-300' },
  active: { label: 'Активний', badgeClass: 'bg-success-50 dark:bg-success-900/20 text-success-600 dark:text-success-300' },
  lagging: { label: 'Відстає', badgeClass: 'bg-warning-50 dark:bg-warning-900/20 text-warning-600 dark:text-warning-300' },
  offline: { label: 'Офлайн', badgeClass: 'bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300' },
  archived: { label: 'Архівований', badgeClass: 'bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400' },
};

const NetworkTopologyPage: React.FC = () => {
  const [nodes, setNodes] = useState<NetworkNode[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Модалка створення вузла.
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [newName, setNewName] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [created, setCreated] = useState<NodeCreateResult | null>(null);

  // Деструктивні дії.
  const [archiveTarget, setArchiveTarget] = useState<NetworkNode | null>(null);
  const [actionBusyId, setActionBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const data = await listNetworkNodes();
      setNodes(data);
      setLoadError(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Не вдалося завантажити вузли';
      setLoadError(msg);
      toast.error(msg);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    // Авто-оновлення статусів (heartbeat-и оновлюються фоном на сервері).
    const t = setInterval(() => void load(), 15_000);
    return () => clearInterval(t);
  }, [load]);

  const handleCreate = async () => {
    const name = newName.trim();
    if (!name) {
      toast.error('Вкажіть назву вузла');
      return;
    }
    setIsCreating(true);
    try {
      const res = await createNetworkNode(name);
      setCreated(res);
      toast.success('Вузол створено — передайте код на новий пристрій');
      setNewName('');
      void load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Помилка створення');
    } finally {
      setIsCreating(false);
    }
  };

  const handleArchive = async () => {
    if (!archiveTarget) return;
    setActionBusyId(archiveTarget.id);
    try {
      await archiveNetworkNode(archiveTarget.id);
      toast.success(`Вузол «${archiveTarget.name}» архівовано`);
      setArchiveTarget(null);
      void load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Помилка архівації');
    } finally {
      setActionBusyId(null);
    }
  };

  const copyCode = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code);
      toast.success('Скопійовано');
    } catch {
      toast.error('Не вдалося скопіювати');
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Вузли мережі</h1>
          <p className="text-sm text-gray-500 dark:text-slate-400">
            Фізичні standby-копії БД (реплікація PostgreSQL). Тестування на 2 пристроях — після реалізації всіх етапів.
          </p>
        </div>
        <Button onClick={() => { setCreated(null); setIsCreateOpen(true); }}>
          <Plus className="w-4 h-4 mr-2" />
          Додати вузол
        </Button>
      </div>

      {loadError && (
        <div className="flex items-center gap-3 p-4 rounded-lg border border-danger-200 bg-danger-50 dark:bg-danger-900/10 text-danger-700 dark:text-danger-300">
          <AlertTriangle className="w-5 h-5" />
          <span className="flex-1">{loadError}</span>
          <Button variant="secondary" onClick={() => void load()}>
            <RefreshCw className="w-4 h-4 mr-1" /> Повторити
          </Button>
        </div>
      )}

      {isLoading ? (
        <div className="flex justify-center py-16">
          <Loader2 className="w-8 h-8 animate-spin text-primary" />
        </div>
      ) : nodes.length === 0 ? (
        <div className="text-center py-16 border-2 border-dashed rounded-xl text-gray-400">
          <Server className="w-12 h-12 mx-auto mb-3" />
          <p>Ще немає жодного вузла мережі.</p>
          <p className="text-sm mt-1">Додайте перший standby-вузол або підключіть комп'ютер через join-код.</p>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {nodes.map((node) => {
            const meta = STATUS_META[node.status] ?? STATUS_META.offline;
            const isPrimary = node.role === 'primary';
            return (
              <div
                key={node.id}
                className="p-5 rounded-xl border border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 shadow-sm"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-3 min-w-0">
                    <div className={`p-2.5 rounded-lg ${isPrimary ? 'bg-primary-50 dark:bg-primary-900/20 text-primary' : 'bg-slate-100 dark:bg-slate-700 text-slate-500'}`}>
                      {isPrimary ? <Server className="w-5 h-5" /> : <Laptop className="w-5 h-5" />}
                    </div>
                    <div className="min-w-0">
                      <p className="font-semibold truncate">{node.name}</p>
                      <p className="text-xs text-gray-500 capitalize">
                        {isPrimary ? 'Primary (сервер)' : 'Standby (копія)'}
                      </p>
                    </div>
                  </div>
                  <span className={`shrink-0 px-2.5 py-1 rounded-full text-xs font-medium ${meta.badgeClass}`}>
                    {meta.label}
                  </span>
                </div>

                <div className="mt-4 space-y-1.5 text-sm">
                  <div className="flex items-center justify-between text-gray-600 dark:text-slate-300">
                    <span className="flex items-center gap-1.5">
                      <Clock className="w-3.5 h-3.5" />
                      {node.last_seen_at ? `Останній heartbeat: ${formatRelativeTime(node.last_seen_at)}` : 'Heartbeat ще не було'}
                    </span>
                  </div>
                  {!isPrimary && (
                    <>
                      <div className="flex items-center justify-between text-gray-600 dark:text-slate-300">
                        <span className="flex items-center gap-1.5">
                          <HardDrive className="w-3.5 h-3.5" />
                          Відставання
                        </span>
                        <span>{node.replication_lag_bytes == null ? '—' : formatBytes(node.replication_lag_bytes)}</span>
                      </div>
                      <div className="flex items-center justify-between text-gray-600 dark:text-slate-300">
                        <span className="flex items-center gap-1.5">
                          <Database className="w-3.5 h-3.5" />
                          Розмір БД
                        </span>
                        <span>{node.db_size_bytes == null ? '—' : formatBytes(node.db_size_bytes)}</span>
                      </div>
                    </>
                  )}
                  {node.host && (
                    <div className="text-xs text-gray-400 truncate" title={node.host}>host: {node.host}{node.app_version ? ` · v${node.app_version}` : ''}</div>
                  )}
                </div>

                {!isPrimary && node.status !== 'archived' && (
                  <div className="mt-4 flex gap-2">
                    <Button
                      variant="secondary"
                      size="sm"
                      className="flex-1 text-danger-600"
                      disabled={actionBusyId === node.id}
                      onClick={() => setArchiveTarget(node)}
                    >
                      <Archive className="w-3.5 h-3.5 mr-1" />
                      Архівувати
                    </Button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* ── Модалка створення вузла / join-код ── */}
      <Modal isOpen={isCreateOpen} onClose={() => setIsCreateOpen(false)} title={created ? 'Вузол створено' : 'Додати вузол'}>
        {!created ? (
          <div className="space-y-4">
            <Input
              placeholder="Назва вузла (напр. Магазин на Лівому березі)"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              autoFocus
            />
            <Button className="w-full" onClick={() => void handleCreate()} disabled={isCreating || !newName.trim()}>
              {isCreating && <Loader2 className="w-4 h-4 mr-2 animate-spin" />}
              Створити вузол
            </Button>
          </div>
        ) : (
          <div className="space-y-4">
            <p className="text-sm text-gray-600 dark:text-slate-300">
              На новому комп'ютері (вузол) відкрийте екран «Приєднати як вузол мережі» і введіть цей код. Код дійсний до{' '}
              <b>{new Date(created.join_code_expires_at).toLocaleString()}</b>.
            </p>
            <div className="flex items-center gap-2 p-3 rounded-lg bg-slate-100 dark:bg-slate-700 font-mono text-lg tracking-widest">
              <span className="flex-1 text-center font-bold">{created.join_code}</span>
              <button onClick={() => void copyCode(created.join_code)} className="text-gray-500 hover:text-gray-800" title="Копіювати">
                <Copy className="w-4 h-4" />
              </button>
            </div>
            <p className="text-xs text-gray-400">
              Підказка для підключення (primary): <span className="font-mono">{created.primary_host_hint}</span>
            </p>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="secondary" onClick={() => { setCreated(null); setNewName(''); }}>Створити ще</Button>
              <Button onClick={() => setIsCreateOpen(false)}>Готово</Button>
            </div>
          </div>
        )}
      </Modal>

      {/* ── Підтвердження архівації ── */}
      <ConfirmDialog
        isOpen={archiveTarget !== null}
        title="Архівувати вузол?"
        message={`Вузол «${archiveTarget?.name ?? ''}» буде вимкнено з мережі. Реплікація зупиниться.`}
        confirmText="Архівувати"
        onClose={() => setArchiveTarget(null)}
        onConfirm={() => void handleArchive()}
        variant="danger"
        isLoading={actionBusyId === archiveTarget?.id}
      />
    </div>
  );
};

export default NetworkTopologyPage;
