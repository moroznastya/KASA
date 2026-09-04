import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  MonitorSmartphone,
  RefreshCw,
  Ban,
  Unlock,
  Archive,
  Copy,
  KeyRound,
  Loader2,
  AlertTriangle,
} from 'lucide-react';
import toast from 'react-hot-toast';
import { Button } from '@/components/ui/Button';
import { Select } from '@/components/ui/Select';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { storeService } from '@/services/storeService';
import { Store } from '@/types/store';
import { deviceAdminService, DeviceInfo, DeviceStatus, extractApiError } from '@/services/deviceAdminService';
import { formatDate, formatRelativeTime } from '@/utils/format';

/**
 * «Каси мережі» — панель власника/адміна для керування ВСІМА касами,
 * що синхронізуються як мережеві device-пристрої (Етап 3/4).
 *
 * На відміну від Settings → «Мережева каса» (активація ЦІЄЇ каси) цей екран
 * працює з серверною адмін-панеллю: GET /admin/devices, генерація кодів
 * активації для точок, блокування/розблокування/архівація пристроїв.
 */

const STATUS_META: Record<DeviceStatus, { label: string; badgeClass: string; iconClass: string }> = {
  active: {
    label: 'Активна',
    badgeClass: 'bg-success-50 dark:bg-success-900/20 text-success-600 dark:text-success-300',
    iconClass: 'bg-success-50 dark:bg-success-900/20 text-success-600 dark:text-success-300',
  },
  blocked: {
    label: 'Заблокована',
    badgeClass: 'bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300',
    iconClass: 'bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300',
  },
  deleted: {
    label: 'Архівована',
    badgeClass: 'bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400',
    iconClass: 'bg-gray-100 dark:bg-slate-700 text-gray-400 dark:text-gray-500',
  },
};

const NetworkDevicesPage: React.FC = () => {
  const [stores, setStores] = useState<Store[]>([]);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Фільтр списку кас по торговій точці ('' = всі точки).
  const [filterStoreId, setFilterStoreId] = useState<string>('');

  // Блок генерації коду активації.
  const [codeStoreId, setCodeStoreId] = useState<string>('');
  const [isGenerating, setIsGenerating] = useState(false);
  const [generatedCode, setGeneratedCode] = useState<string | null>(null);

  // Деструктивні дії (confirm) + busy-стан кнопок дій.
  const [blockTarget, setBlockTarget] = useState<DeviceInfo | null>(null);
  const [archiveTarget, setArchiveTarget] = useState<DeviceInfo | null>(null);
  const [actionBusyId, setActionBusyId] = useState<string | null>(null);

  // ── Точки (для фільтра і генерації коду) ──
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const data = await storeService.list();
        if (!cancelled) setStores(data);
      } catch {
        if (!cancelled) toast.error('Не вдалося завантажити список торгових точок');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // ── Список кас: перезапит при зміні фільтра ──
  const loadDevices = useCallback(async () => {
    setIsLoading(true);
    setLoadError(null);
    try {
      const data = await deviceAdminService.listDevices(filterStoreId || undefined);
      setDevices(data);
    } catch (err) {
      setLoadError(extractApiError(err, 'Не вдалося завантажити список кас'));
    } finally {
      setIsLoading(false);
    }
  }, [filterStoreId]);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  // Сортування: активні перші, потім заблоковані; архівовані — в кінці.
  const sortedDevices = useMemo(() => {
    const order: Record<DeviceStatus, number> = { active: 0, blocked: 1, deleted: 2 };
    return [...devices].sort((a, b) => {
      const byStatus = order[a.status] - order[b.status];
      if (byStatus !== 0) return byStatus;
      return (b.last_seen_at || '').localeCompare(a.last_seen_at || '');
    });
  }, [devices]);

  const storeOptions = stores.map((s) => ({ value: s.id, label: s.name }));

  // ── Генерація коду активації ──
  const handleGenerateCode = async () => {
    if (!codeStoreId) {
      toast.error('Оберіть торгову точку');
      return;
    }
    setIsGenerating(true);
    setGeneratedCode(null);
    try {
      const result = await deviceAdminService.generateActivationCode(codeStoreId);
      setGeneratedCode(result.code);
      toast.success('Код активації згенеровано');
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося згенерувати код активації'));
    } finally {
      setIsGenerating(false);
    }
  };

  const handleCopyCode = async () => {
    if (!generatedCode) return;
    try {
      await navigator.clipboard.writeText(generatedCode);
      toast.success('Код скопійовано в буфер обміну');
    } catch {
      toast.error('Не вдалося скопіювати код');
    }
  };

  // ── Блокування (confirm) ──
  const handleBlock = async () => {
    if (!blockTarget) return;
    setActionBusyId(blockTarget.id);
    try {
      await deviceAdminService.blockDevice(blockTarget.id);
      toast.success(`Касу «${blockTarget.name}» заблоковано`);
      setBlockTarget(null);
      await loadDevices();
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося заблокувати касу'));
    } finally {
      setActionBusyId(null);
    }
  };

  // ── Розблокування (без confirm — зворотна дія) ──
  const handleUnblock = async (device: DeviceInfo) => {
    setActionBusyId(device.id);
    try {
      await deviceAdminService.unblockDevice(device.id);
      toast.success(`Касу «${device.name}» розблоковано`);
      await loadDevices();
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося розблокувати касу'));
    } finally {
      setActionBusyId(null);
    }
  };

  // ── Архівація (confirm, деструктивно) ──
  const handleArchive = async () => {
    if (!archiveTarget) return;
    setActionBusyId(archiveTarget.id);
    try {
      await deviceAdminService.archiveDevice(archiveTarget.id);
      toast.success(`Касу «${archiveTarget.name}» архівовано`);
      setArchiveTarget(null);
      await loadDevices();
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося архівувати касу'));
    } finally {
      setActionBusyId(null);
    }
  };

  const emptyText = filterStoreId
    ? 'Для обраної точки кас не знайдено'
    : 'Жодної каси не активовано';

  return (
    <div className="p-6 max-w-7xl mx-auto">
      {/* ── Заголовок ── */}
      <div className="flex items-start justify-between gap-4 mb-6">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Каси мережі</h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Керування касами, що синхронізуються як мережеві пристрої: коди активації,
            блокування, архівація
          </p>
        </div>
        <Button variant="secondary" onClick={() => void loadDevices()} title="Оновити список">
          <RefreshCw className="w-4 h-4 mr-2" />
          Оновити
        </Button>
      </div>

      {/* ── Код активації для точки ── */}
      <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-5 mb-6">
        <div className="flex items-center gap-2 mb-1">
          <KeyRound className="w-5 h-5 text-primary-600 dark:text-primary-400" />
          <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">
            Код активації для точки
          </h2>
        </div>
        <p className="text-sm text-gray-500 dark:text-gray-400">
          Код дійсний лише для активації <span className="font-medium">нової</span> каси
          (Налаштування → Мережева каса). Вже активовані каси від коду не залежать.
        </p>

        <div className="flex flex-wrap items-end gap-3 mt-4">
          <Select
            label="Торгова точка"
            containerClassName="w-full sm:w-80"
            placeholder="Оберіть точку"
            options={storeOptions}
            value={codeStoreId}
            onChange={(e) => {
              setCodeStoreId(String(e.target.value));
              setGeneratedCode(null);
            }}
          />
          <Button
            onClick={() => void handleGenerateCode()}
            isLoading={isGenerating}
            disabled={!codeStoreId}
          >
            {isGenerating ? null : <KeyRound className="w-4 h-4 mr-2" />}
            Згенерувати код
          </Button>
        </div>

        {generatedCode && (
          <div className="mt-4 rounded-lg border border-primary-200 dark:border-primary-800 bg-primary-50 dark:bg-primary-900/10 px-4 py-3">
            <div className="flex flex-wrap items-center gap-4">
              <code className="text-2xl sm:text-3xl font-mono font-bold tracking-[0.25em] text-primary-700 dark:text-primary-300">
                {generatedCode}
              </code>
              <Button variant="secondary" size="sm" onClick={() => void handleCopyCode()}>
                <Copy className="w-4 h-4 mr-1" />
                Копіювати
              </Button>
            </div>
            <p className="mt-2 text-xs text-amber-700 dark:text-amber-400 flex items-center gap-1.5">
              <AlertTriangle className="w-3.5 h-3.5 flex-shrink-0" />
              Новий код буде дійсним для наступних активацій. Активовані каси не постраждають.
            </p>
          </div>
        )}
      </div>

      {/* ── Фільтр списку по точці ── */}
      <div className="flex items-center justify-between gap-4 mb-4">
        <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">
          Пристрої <span className="text-sm font-normal text-gray-400">({devices.length})</span>
        </h2>
        <div className="w-full max-w-xs">
          <Select
            placeholder="Всі точки"
            options={[{ value: '', label: 'Всі точки' }, ...storeOptions]}
            value={filterStoreId}
            onChange={(e) => setFilterStoreId(String(e.target.value))}
          />
        </div>
      </div>

      {/* ── Список ── */}
      {isLoading ? (
        <div className="flex items-center justify-center py-20">
          <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
        </div>
      ) : loadError ? (
        <div className="rounded-lg border border-danger-200 dark:border-danger-800 bg-danger-50 dark:bg-danger-900/10 px-4 py-6 text-center">
          <AlertTriangle className="w-10 h-10 mx-auto text-danger-500 mb-3" />
          <p className="text-danger-600 dark:text-danger-300 mb-4">{loadError}</p>
          <Button onClick={() => void loadDevices()}>
            <RefreshCw className="w-4 h-4 mr-2" />
            Спробувати знову
          </Button>
        </div>
      ) : sortedDevices.length === 0 ? (
        <div className="text-center py-16 bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700">
          <MonitorSmartphone className="w-16 h-16 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
          <p className="text-gray-500 dark:text-gray-400 font-medium">{emptyText}</p>
          <p className="text-sm text-gray-400 dark:text-gray-500 mt-1 max-w-md mx-auto">
            Згенеруйте код активації для точки вище, потім на касі: Налаштування →
            Мережева каса → введіть код та адресу сервера.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {sortedDevices.map((device) => {
            // Захисний fallback: сервер може повернути невідомий статус
            // (напр. legacy 'pending') — показуємо сірим з сирим значенням.
            const meta =
              STATUS_META[device.status] ?? {
                label: device.status,
                badgeClass:
                  'bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400',
                iconClass: 'bg-gray-100 dark:bg-slate-700 text-gray-400 dark:text-gray-500',
              };
            const isDeleted = device.status === 'deleted';
            const busy = actionBusyId === device.id;
            return (
              <div
                key={device.id}
                className={`bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-4 flex items-start gap-4 transition-opacity ${
                  isDeleted ? 'opacity-60' : 'hover:shadow-md'
                }`}
              >
                <div
                  className={`w-11 h-11 rounded-lg flex items-center justify-center flex-shrink-0 ${meta.iconClass}`}
                >
                  <MonitorSmartphone className="w-5 h-5" />
                </div>

                <div className="flex-1 min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="font-medium text-gray-900 dark:text-gray-100 truncate">
                      {device.name}
                    </h3>
                    <span
                      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${meta.badgeClass}`}
                    >
                      {meta.label}
                    </span>
                  </div>

                  <dl className="mt-2 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-x-6 gap-y-1.5 text-sm">
                    <div>
                      <dt className="text-xs text-gray-400 dark:text-gray-500">Торгова точка</dt>
                      <dd className="text-gray-700 dark:text-gray-300 truncate">{device.store_name}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-gray-400 dark:text-gray-500">Версія застосунку</dt>
                      <dd className="text-gray-700 dark:text-gray-300">{device.app_version || '—'}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-gray-400 dark:text-gray-500">Активація</dt>
                      <dd className="text-gray-700 dark:text-gray-300">
                        {device.activated_at ? formatDate(device.activated_at) : '—'}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs text-gray-400 dark:text-gray-500">Остання активність</dt>
                      <dd className="text-gray-700 dark:text-gray-300">
                        {device.last_seen_at ? formatRelativeTime(device.last_seen_at) : 'ніколи'}
                      </dd>
                    </div>
                  </dl>
                </div>

                {!isDeleted && (
                  <div className="flex items-center gap-2 flex-shrink-0">
                    {device.status === 'active' && (
                      <Button
                        variant="danger"
                        size="sm"
                        onClick={() => setBlockTarget(device)}
                        disabled={busy}
                        title="Заблокувати касу — синк миттєво відхилятиметься"
                      >
                        {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Ban className="w-4 h-4" />}
                        Заблокувати
                      </Button>
                    )}
                    {device.status === 'blocked' && (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => void handleUnblock(device)}
                        disabled={busy}
                        title="Розблокувати касу"
                      >
                        {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Unlock className="w-4 h-4" />}
                        Розблокувати
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setArchiveTarget(device)}
                      disabled={busy}
                      title="Архівувати касу"
                    >
                      <Archive className="w-4 h-4" />
                    </Button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* ── Confirm: блокування ── */}
      <ConfirmDialog
        isOpen={blockTarget !== null}
        onClose={() => setBlockTarget(null)}
        onConfirm={() => void handleBlock()}
        title="Заблокувати касу?"
        message={`Каса «${blockTarget?.name || ''}» (${blockTarget?.store_name || ''}) перестане синхронізуватися: сервер миттєво відхилятиме її запити. Ви зможете розблокувати її пізніше.`}
        confirmText="Заблокувати"
        variant="danger"
        isLoading={actionBusyId !== null}
      />

      {/* ── Confirm: архівація ── */}
      <ConfirmDialog
        isOpen={archiveTarget !== null}
        onClose={() => setArchiveTarget(null)}
        onConfirm={() => void handleArchive()}
        title="Архівувати касу?"
        message={`Каса «${archiveTarget?.name || ''}» буде позначена як архівована і зникне зі списку активних. Пристрій більше не зможе синхронізуватися.`}
        confirmText="Архівувати"
        variant="danger"
        isLoading={actionBusyId !== null}
      />
    </div>
  );
};

export default NetworkDevicesPage;
