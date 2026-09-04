import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import {
  ArrowLeft,
  Store as StoreIcon,
  Building2,
  Phone,
  MapPin,
  Save,
  Archive,
  RotateCcw,
  Users,
  MonitorSmartphone,
  FileText,
  Tag,
  Plus,
  Pencil,
  Power,
  PowerOff,
  KeyRound,
  Hash,
  Loader2,
  RefreshCw,
  AlertTriangle,
} from 'lucide-react';
import toast from 'react-hot-toast';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Modal } from '@/components/ui/Modal';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { adminService } from '@/services/adminService';
import { adminPrroService } from '@/services/adminPrroService';
import { deviceAdminService, DeviceInfo, extractApiError } from '@/services/deviceAdminService';
import { Store, StoreWorker } from '@/types/store';
import { StorePrroSettings } from '@/types/adminPrro';
import { formatDateTime } from '@/utils/format';

// ── Ролі (Етап 1: адмінка owner/store_manager; каса admin/cashier) ─────────
const ROLE_LABELS: Record<string, string> = {
  owner: 'Власник',
  store_manager: 'Керуючий мережею',
  manager: 'Керуючий мережею',
  admin: 'Адміністратор',
  cashier: 'Касир',
};

const ROLE_COLORS: Record<string, string> = {
  owner: 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-400',
  store_manager: 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400',
  manager: 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400',
  admin: 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400',
  cashier: 'bg-gray-100 dark:bg-slate-700 text-gray-700 dark:text-gray-300',
};

const GLOBAL_ROLE_OPTIONS = [
  { value: 'store_manager', label: 'Керуючий мережею (адмінка)' },
  { value: 'admin', label: 'Адміністратор (каса)' },
  { value: 'cashier', label: 'Касир (каса)' },
];

const STORE_ROLE_OPTIONS = [
  { value: 'owner', label: 'Власник' },
  { value: 'store_manager', label: 'Керуючий мережею' },
  { value: 'admin', label: 'Адміністратор' },
  { value: 'cashier', label: 'Касир' },
];

/** online: last_seen_at < 5 хв (вимога ТЗ 5.3). */
function isDeviceOnline(lastSeenAt: string | null): boolean {
  if (!lastSeenAt) return false;
  const t = new Date(lastSeenAt).getTime();
  if (Number.isNaN(t)) return false;
  return Date.now() - t < 5 * 60 * 1000;
}

type TabKey = 'general' | 'workers' | 'devices' | 'prro' | 'prices';

/**
 * «Режим адміністратора» — картка торговельної точки (Етап 1, ТЗ 5.1–5.3).
 * Вкладки: загальне / працівники / каси / ПРРО (Етап 4) / ціни (Етапи 5-6).
 * Архівація точки — з confirm-модалкою про прив'язані каси; деактивація
 * працівника — без фізичного видалення (is_active=false).
 */
const StoreDetailPage: React.FC = () => {
  const { storeId } = useParams<{ storeId: string }>();
  const navigate = useNavigate();

  const [store, setStore] = useState<Store | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Форма «Загальне».
  const [form, setForm] = useState({
    name: '',
    address: '',
    phone: '',
    legal_name: '',
    edrpou: '',
  });
  const [isSavingGeneral, setIsSavingGeneral] = useState(false);

  // Архівація / відновлення.
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [isArchiving, setIsArchiving] = useState(false);

  // Працівники.
  const [workers, setWorkers] = useState<StoreWorker[]>([]);
  const [workersLoading, setWorkersLoading] = useState(false);
  const [workerModalOpen, setWorkerModalOpen] = useState(false);
  const [workerForm, setWorkerForm] = useState({
    name: '',
    login: '',
    password: '',
    pin_code: '',
    role: 'store_manager',
    store_role: 'store_manager',
  });
  const [isSavingWorker, setIsSavingWorker] = useState(false);
  const [deactivateTarget, setDeactivateTarget] = useState<StoreWorker | null>(null);
  const [resetPwdTarget, setResetPwdTarget] = useState<StoreWorker | null>(null);
  const [resetPwd, setResetPwd] = useState('');
  const [resetPinTarget, setResetPinTarget] = useState<StoreWorker | null>(null);
  const [resetPin, setResetPin] = useState('');
  const [workerActionBusy, setWorkerActionBusy] = useState<string | null>(null);

  // Каси.
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [devicesLoading, setDevicesLoading] = useState(false);

  // ПРРО (per-store: редагована форма «один магазин — один ПРРО»).
  const [prro, setPrro] = useState<StorePrroSettings | null>(null);
  const [prroLoading, setPrroLoading] = useState(false);
  const [prroError, setPrroError] = useState<string | null>(null);
  const [prroSaving, setPrroSaving] = useState(false);
  const [prroForm, setPrroForm] = useState({
    prro_fn: '',
    prro_tn: '',
    prro_zn: '',
    mode: 'test' as 'test' | 'prod',
    url: '',
  });
  const [prroKeyPassword, setPrroKeyPassword] = useState('');
  const [prroKeyFile, setPrroKeyFile] = useState<File | null>(null);

  const [tab, setTab] = useState<TabKey>('general');


  // ── ПРРО: read-only стан (Етап 5) ────────────────
  useEffect(() => {
    if (tab !== 'prro' || !storeId) return;
    let cancelled = false;
    setPrroLoading(true);
    setPrroError(null);
    adminPrroService
      .getStorePrro(storeId)
      .then((data) => {
        if (!cancelled) {
          setPrro(data);
          setPrroForm({
            prro_fn: data.settings.prro_fn || '',
            prro_tn: data.settings.prro_tn || '',
            prro_zn: data.settings.prro_zn || '',
            mode: data.settings.mode === 'prod' ? 'prod' : 'test',
            url: data.settings.url || '',
          });
          setPrroKeyPassword('');
          setPrroKeyFile(null);
        }
      })
      .catch((err) => {
        if (!cancelled) setPrroError(extractApiError(err, 'Не вдалося завантажити стан ПРРО'));
      })
      .finally(() => {
        if (!cancelled) setPrroLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [tab, storeId]);

  // ── Завантаження точки ─────────────────────────
  const loadStore = useCallback(async () => {
    if (!storeId) return;
    setIsLoading(true);
    setLoadError(null);
    try {
      const data = await adminService.getStore(storeId);
      setStore(data);
      setForm({
        name: data.name,
        address: data.address || '',
        phone: data.phone || '',
        legal_name: data.legal_name || '',
        edrpou: data.edrpou || '',
      });
    } catch (err) {
      setLoadError(extractApiError(err, 'Не вдалося завантажити точку'));
    } finally {
      setIsLoading(false);
    }
  }, [storeId]);

  useEffect(() => {
    void loadStore();
  }, [loadStore]);

  // ── Працівники ──────────────────────────────────
  const loadWorkers = useCallback(async () => {
    if (!storeId) return;
    setWorkersLoading(true);
    try {
      const data = await adminService.listWorkers(storeId);
      setWorkers(data);
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося завантажити працівників'));
    } finally {
      setWorkersLoading(false);
    }
  }, [storeId]);

  useEffect(() => {
    if (tab === 'workers') void loadWorkers();
  }, [tab, loadWorkers]);

  // ── Каси ────────────────────────────────────────
  const loadDevices = useCallback(async () => {
    if (!storeId) return;
    setDevicesLoading(true);
    try {
      const data = await deviceAdminService.listDevices(storeId);
      setDevices(data);
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося завантажити каси'));
    } finally {
      setDevicesLoading(false);
    }
  }, [storeId]);

  useEffect(() => {
    if (tab === 'devices') void loadDevices();
  }, [tab, loadDevices]);

  // ── Збереження ПРРО точки (PUT /admin/stores/:id/prro-settings) ──
  const handleSavePrro = async () => {
    if (!storeId) return;
    const hasFormValue =
      prroForm.prro_fn.trim() || prroForm.prro_tn.trim() || prroForm.prro_zn.trim() ||
      prroForm.url.trim() || prroKeyPassword || prroKeyFile;
    if (!hasFormValue) {
      toast.error('Немає даних для збереження — заповніть хоча б одне поле');
      return;
    }
    setPrroSaving(true);
    try {
      const updated = await adminPrroService.updateStorePrro(storeId, {
        prro_fn: prroForm.prro_fn.trim() || undefined,
        prro_tn: prroForm.prro_tn.trim() || undefined,
        prro_zn: prroForm.prro_zn.trim() || undefined,
        mode: prroForm.mode,
        url: prroForm.url.trim() || undefined,
        key_password: prroKeyPassword || undefined,
        keyFile: prroKeyFile,
      });
      setPrro(updated);
      setPrroForm({
        prro_fn: updated.settings.prro_fn || '',
        prro_tn: updated.settings.prro_tn || '',
        prro_zn: updated.settings.prro_zn || '',
        mode: updated.settings.mode === 'prod' ? 'prod' : 'test',
        url: updated.settings.url || '',
      });
      setPrroKeyPassword('');
      setPrroKeyFile(null);
      toast.success('Налаштування ПРРО точки збережено');
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося зберегти налаштування ПРРО'));
    } finally {
      setPrroSaving(false);
    }
  };

  // ── Збереження «Загальне» (PUT /admin/stores/:id) ──
  const handleSaveGeneral = async () => {
    if (!store || !storeId) return;
    if (!form.name.trim()) {
      toast.error('Введіть назву точки');
      return;
    }
    setIsSavingGeneral(true);
    try {
      const updated = await adminService.updateStore(storeId, {
        name: form.name.trim(),
        address: form.address.trim() || null,
        phone: form.phone.trim() || null,
        legal_name: form.legal_name.trim() || null,
        edrpou: form.edrpou.trim() || null,
      });
      setStore(updated);
      toast.success('Дані точки збережено');
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося зберегти точку'));
    } finally {
      setIsSavingGeneral(false);
    }
  };

  // ── Архівація (DELETE → is_active=false) ─────────
  const handleArchive = async () => {
    if (!storeId) return;
    setIsArchiving(true);
    try {
      const result = await adminService.archiveStore(storeId);
      if (result.warning) {
        toast(result.warning, { duration: 5000 });
      }
      toast.success('Точку заархівовано');
      setArchiveOpen(false);
      await loadStore();
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося архівувати точку'));
    } finally {
      setIsArchiving(false);
    }
  };

  const handleRestore = async () => {
    if (!store || !storeId) return;
    setIsSavingGeneral(true);
    try {
      const updated = await adminService.updateStore(storeId, {
        name: store.name,
        is_active: true,
      });
      setStore(updated);
      toast.success('Точку відновлено');
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося відновити точку'));
    } finally {
      setIsSavingGeneral(false);
    }
  };

  // ── Працівники: створення ───────────────────────
  const openCreateWorker = () => {
    setWorkerForm({
      name: '',
      login: '',
      password: '',
      pin_code: '',
      role: 'store_manager',
      store_role: 'store_manager',
    });
    setWorkerModalOpen(true);
  };

  const handleCreateWorker = async () => {
    if (!storeId) return;
    if (!workerForm.name.trim()) {
      toast.error("Введіть ім'я працівника");
      return;
    }
    if (!workerForm.password) {
      toast.error('Введіть пароль');
      return;
    }
    setIsSavingWorker(true);
    try {
      await adminService.createWorker(storeId, {
        name: workerForm.name.trim(),
        login: workerForm.login.trim() || undefined,
        password: workerForm.password,
        pin_code: workerForm.pin_code.trim() || undefined,
        role: workerForm.role as 'store_manager' | 'admin' | 'cashier',
        store_role: workerForm.store_role,
      });
      toast.success('Працівника створено і прив\'язано до точки');
      setWorkerModalOpen(false);
      await loadWorkers();
      await loadStore();
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося створити працівника'));
    } finally {
      setIsSavingWorker(false);
    }
  };

  // ── Працівники: деактивація/активація ───────────
  const handleToggleActive = async (worker: StoreWorker) => {
    setWorkerActionBusy(worker.id);
    try {
      if (worker.is_active) {
        await adminService.deactivateUser(worker.id);
        toast.success(`Працівника «${worker.name}» деактивовано (запис збережено)`);
      } else {
        await adminService.activateUser(worker.id);
        toast.success(`Працівника «${worker.name}» активовано`);
      }
      setDeactivateTarget(null);
      await loadWorkers();
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося змінити статус працівника'));
    } finally {
      setWorkerActionBusy(null);
    }
  };

  // ── Працівники: скидання пароля / PIN ───────────
  const handleResetPassword = async () => {
    if (!resetPwdTarget) return;
    if (resetPwd.length < 4) {
      toast.error('Пароль має містити щонайменше 4 символи');
      return;
    }
    setWorkerActionBusy(resetPwdTarget.id);
    try {
      await adminService.resetPassword(resetPwdTarget.id, resetPwd);
      toast.success(`Пароль працівника «${resetPwdTarget.name}» оновлено`);
      setResetPwdTarget(null);
      setResetPwd('');
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося скинути пароль'));
    } finally {
      setWorkerActionBusy(null);
    }
  };

  const handleResetPin = async () => {
    if (!resetPinTarget) return;
    if (resetPin.length < 4 || resetPin.length > 10) {
      toast.error('PIN має містити від 4 до 10 символів');
      return;
    }
    setWorkerActionBusy(resetPinTarget.id);
    try {
      await adminService.resetPin(resetPinTarget.id, resetPin);
      toast.success(`PIN працівника «${resetPinTarget.name}» оновлено`);
      setResetPinTarget(null);
      setResetPin('');
    } catch (err) {
      toast.error(extractApiError(err, 'Не вдалося скинути PIN'));
    } finally {
      setWorkerActionBusy(null);
    }
  };

  const sortedDevices = useMemo(() => {
    const order: Record<string, number> = { active: 0, blocked: 1, deleted: 2 };
    return [...devices].sort((a, b) => {
      const byStatus = (order[a.status] ?? 3) - (order[b.status] ?? 3);
      if (byStatus !== 0) return byStatus;
      return (b.last_seen_at || '').localeCompare(a.last_seen_at || '');
    });
  }, [devices]);

  // ── Рендер ───────────────────────────────────────
  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <Spinner size="lg" />
      </div>
    );
  }

  if (loadError || !store) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <div className="text-center">
          <AlertTriangle className="w-10 h-10 text-danger-500 mx-auto mb-3" />
          <p className="text-red-500 font-medium">Помилка завантаження точки</p>
          <p className="text-sm text-gray-500 mt-1">{loadError}</p>
          <Button variant="secondary" className="mt-4" onClick={() => navigate('/settings/stores')}>
            До списку точок
          </Button>
        </div>
      </div>
    );
  }

  const tabs: { key: TabKey; label: string; icon: React.ReactNode }[] = [
    { key: 'general', label: 'Загальне', icon: <StoreIcon className="w-4 h-4" /> },
    { key: 'workers', label: 'Працівники', icon: <Users className="w-4 h-4" /> },
    { key: 'devices', label: 'Каси', icon: <MonitorSmartphone className="w-4 h-4" /> },
    { key: 'prro', label: 'ПРРО', icon: <FileText className="w-4 h-4" /> },
    { key: 'prices', label: 'Ціни', icon: <Tag className="w-4 h-4" /> },
  ];

  return (
    <div className="max-w-6xl mx-auto px-4 py-6 space-y-6">
      {/* Заголовок */}
      <div className="flex items-center justify-between flex-wrap gap-3">
        <div className="flex items-center gap-4 min-w-0">
          <button
            onClick={() => navigate('/settings/stores')}
            className="p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
            title="Назад до списку точок"
          >
            <ArrowLeft className="w-5 h-5 text-gray-500" />
          </button>
          <div className="min-w-0">
            <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100 truncate flex items-center gap-3">
              {store.name}
              {store.is_active ? (
                <Badge variant="success" size="sm">Активна</Badge>
              ) : (
                <Badge variant="default" size="sm">Архів</Badge>
              )}
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Режим адміністратора · картка торговельної точки
            </p>
          </div>
        </div>
        <Button variant="secondary" onClick={() => void loadStore()} title="Оновити">
          <RefreshCw className="w-4 h-4 mr-2" />
          Оновити
        </Button>
      </div>

      {/* Вкладки */}
      <div className="flex gap-1 border-b border-gray-200 dark:border-slate-700 overflow-x-auto">
        {tabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors whitespace-nowrap ${
              tab === t.key
                ? 'border-primary-500 text-primary-600 dark:text-primary-400'
                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
            }`}
          >
            {t.icon}
            {t.label}
            {t.key === 'workers' && workers.length > 0 && tab === 'workers' && (
              <span className="text-xs text-gray-400">({workers.length})</span>
            )}
            {t.key === 'devices' && store.devices_count != null && store.devices_count > 0 && (
              <span className="text-xs text-gray-400">({store.devices_count})</span>
            )}
          </button>
        ))}
      </div>

      {/* ── Вкладка: Загальне ─────────────────────── */}
      {tab === 'general' && (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <div className="lg:col-span-2 bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-6 space-y-4">
            <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">
              Основні дані
            </h2>
            <Input
              label="Назва точки"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Магазин на Хрещатику"
              disabled={!store.is_active}
            />
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <Input
                label="Адреса"
                value={form.address}
                onChange={(e) => setForm({ ...form, address: e.target.value })}
                placeholder="м. Київ, вул. Хрещатик, 1"
                icon={<MapPin className="w-4 h-4" />}
                disabled={!store.is_active}
              />
              <Input
                label="Телефон"
                value={form.phone}
                onChange={(e) => setForm({ ...form, phone: e.target.value })}
                placeholder="+380 00 000 00 00"
                icon={<Phone className="w-4 h-4" />}
                disabled={!store.is_active}
              />
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <Input
                label="Юрособа / ФОП"
                value={form.legal_name}
                onChange={(e) => setForm({ ...form, legal_name: e.target.value })}
                placeholder="ФОП Іванов І.І. / ТОВ «Мережа»"
                icon={<Building2 className="w-4 h-4" />}
                disabled={!store.is_active}
              />
              <Input
                label="ЄДРПОУ / ІПН"
                value={form.edrpou}
                onChange={(e) => setForm({ ...form, edrpou: e.target.value })}
                placeholder="12345678"
                icon={<Hash className="w-4 h-4" />}
                disabled={!store.is_active}
              />
            </div>
            <p className="text-xs text-gray-400 dark:text-gray-500">
              Юрособа та ЄДРПОУ використовуються для фіскалізації (ПРРО) — реалізація на Етапі 4.
            </p>
            <div className="flex items-center justify-end gap-3 pt-2">
              <Button onClick={() => void handleSaveGeneral()} isLoading={isSavingGeneral} disabled={!store.is_active}>
                <Save className="w-4 h-4 mr-2" />
                Зберегти
              </Button>
            </div>
          </div>

          {/* Статус та дії */}
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-6 space-y-4 h-fit">
            <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">Статус точки</h2>
            <div className="flex items-center justify-between">
              <span className="text-sm text-gray-500 dark:text-gray-400">Стан</span>
              {store.is_active ? (
                <Badge variant="success">Активна</Badge>
              ) : (
                <Badge variant="default">Архівована</Badge>
              )}
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm text-gray-500 dark:text-gray-400">Прив'язано кас</span>
              <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                {store.devices_count ?? 0}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm text-gray-500 dark:text-gray-400">Працівників</span>
              <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                {store.workers_count ?? 0}
              </span>
            </div>
            <div className="pt-3 border-t border-gray-200 dark:border-slate-700 space-y-2">
              {store.is_active ? (
                <Button
                  variant="danger"
                  className="w-full"
                  onClick={() => setArchiveOpen(true)}
                >
                  <Archive className="w-4 h-4 mr-2" />
                  Архівувати точку
                </Button>
              ) : (
                <Button
                  variant="secondary"
                  className="w-full"
                  onClick={() => void handleRestore()}
                  isLoading={isSavingGeneral}
                >
                  <RotateCcw className="w-4 h-4 mr-2" />
                  Відновити точку
                </Button>
              )}
              <p className="text-xs text-gray-400 dark:text-gray-500 leading-relaxed">
                Архівація — м'яке видалення (статус «Архів»). Фізичного видалення
                точки та її даних не передбачено — це зберігає історію продажів.
              </p>
            </div>
          </div>
        </div>
      )}

      {/* ── Вкладка: Працівники ───────────────────── */}
      {tab === 'workers' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Працівники, прив'язані до цієї точки (фільтр по точці)
            </p>
            <Button onClick={openCreateWorker}>
              <Plus className="w-4 h-4 mr-2" />
              Новий працівник
            </Button>
          </div>

          {workersLoading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
            </div>
          ) : workers.length === 0 ? (
            <div className="text-center py-16 bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700">
              <Users className="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
              <p className="text-gray-500 dark:text-gray-400">
                До цієї точки ще не прив'язано працівників
              </p>
            </div>
          ) : (
            <div className="space-y-3">
              {workers.map((w) => (
                <div
                  key={w.id}
                  className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-4 flex items-center gap-4"
                >
                  <div className="w-10 h-10 rounded-full bg-primary-50 dark:bg-primary-900/20 text-primary-600 dark:text-primary-400 flex items-center justify-center font-semibold flex-shrink-0">
                    {w.name.charAt(0).toUpperCase()}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <h3 className="font-medium text-gray-900 dark:text-gray-100 truncate">
                        {w.name}
                      </h3>
                      {w.is_active ? (
                        <Badge variant="success" size="sm">Активний</Badge>
                      ) : (
                        <Badge variant="danger" size="sm">Деактивований</Badge>
                      )}
                    </div>
                    <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 flex-wrap mt-0.5">
                      <span>{w.login}</span>
                      <span>·</span>
                      <span className={`inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium ${ROLE_COLORS[w.role] || ROLE_COLORS.cashier}`}>
                        {ROLE_LABELS[w.role] || w.role}
                      </span>
                      <span>·</span>
                      <span className="text-xs text-gray-400">
                        на точці: {ROLE_LABELS[w.store_role] || w.store_role}
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => { setResetPwdTarget(w); setResetPwd(''); }}
                      title="Скинути пароль"
                      disabled={workerActionBusy === w.id}
                    >
                      <KeyRound className="w-4 h-4" />
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => { setResetPinTarget(w); setResetPin(''); }}
                      title="Скинути PIN"
                      disabled={workerActionBusy === w.id}
                    >
                      <Hash className="w-4 h-4" />
                    </Button>
                    {w.is_active ? (
                      <Button
                        variant="danger"
                        size="sm"
                        onClick={() => setDeactivateTarget(w)}
                        title="Деактивувати (без видалення)"
                        disabled={workerActionBusy === w.id}
                      >
                        <PowerOff className="w-4 h-4 mr-1" />
                        Деактивувати
                      </Button>
                    ) : (
                      <Button
                        variant="success"
                        size="sm"
                        onClick={() => void handleToggleActive(w)}
                        title="Активувати"
                        disabled={workerActionBusy === w.id}
                      >
                        <Power className="w-4 h-4 mr-1" />
                        Активувати
                      </Button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── Вкладка: Каси ─────────────────────────── */}
      {tab === 'devices' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Каси точки зі статусом синхронізації (online — останній сеанс &lt; 5 хв)
            </p>
            <Button variant="secondary" onClick={() => navigate('/network/devices')}>
              <MonitorSmartphone className="w-4 h-4 mr-2" />
              Керування касами мережі
            </Button>
          </div>

          {devicesLoading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
            </div>
          ) : sortedDevices.length === 0 ? (
            <div className="text-center py-16 bg-white dark:bg-slate-800 rounded-xl border border-gray-200 dark:border-slate-700">
              <MonitorSmartphone className="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
              <p className="text-gray-500 dark:text-gray-400">
                Для цієї точки ще не активовано жодної каси
              </p>
              <Button
                variant="secondary"
                className="mt-4"
                onClick={() => navigate('/network/devices')}
              >
                Активувати касу
              </Button>
            </div>
          ) : (
            <div className="space-y-3">
              {sortedDevices.map((d) => {
                const online = isDeviceOnline(d.last_seen_at);
                return (
                  <div
                    key={d.id}
                    className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-4 flex items-center gap-4"
                  >
                    <div
                      className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${
                        d.status === 'deleted'
                          ? 'bg-gray-300 dark:bg-gray-600'
                          : online
                            ? 'bg-success-500'
                            : 'bg-danger-500'
                      }`}
                    />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <h3 className="font-medium text-gray-900 dark:text-gray-100">{d.name}</h3>
                        {d.status === 'deleted' ? (
                          <Badge variant="default" size="sm">Архівована</Badge>
                        ) : d.status === 'blocked' ? (
                          <Badge variant="danger" size="sm">Заблокована</Badge>
                        ) : online ? (
                          <Badge variant="success" size="sm">online</Badge>
                        ) : (
                          <Badge variant="default" size="sm">offline</Badge>
                        )}
                      </div>
                      <div className="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400 mt-1 flex-wrap">
                        <span>
                          Останній сеанс:{' '}
                          {d.last_seen_at ? formatDateTime(d.last_seen_at) : '—'}
                        </span>
                        <span>·</span>
                        <span>Версія застосунку: {d.app_version || '—'}</span>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* ── Вкладка: ПРРО (per-store, «один магазин — один ПРРО») ── */}
      {tab === 'prro' && (
        <div className="space-y-4">
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-6">
            {prroLoading ? (
              <div className="flex items-center justify-center py-10">
                <Spinner size="lg" />
              </div>
            ) : prroError ? (
              <div className="flex items-start gap-3 p-4 rounded-xl bg-danger-50 dark:bg-danger-900/20 text-danger-600 dark:text-danger-300 text-sm">
                <AlertTriangle className="w-5 h-5 shrink-0 mt-0.5" />
                <div>
                  <p className="font-medium">Не вдалося завантажити стан ПРРО</p>
                  <p className="mt-1 text-xs opacity-90">{prroError}</p>
                </div>
              </div>
            ) : prro ? (
              <div className="space-y-5">
                <div className="flex items-start justify-between gap-4 flex-wrap">
                  <div className="flex items-center gap-3">
                    <FileText className="w-6 h-6 text-primary-600" />
                    <div>
                      <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                        ПРРО точки — окремий реєстр
                      </h3>
                      <p className="text-sm text-gray-500 dark:text-gray-400">
                        Конфіг, зміни та офлайн-черга цієї точки ізольовані від інших магазинів
                      </p>
                    </div>
                  </div>
                  {prro.configured ? (
                    <Badge variant="success">Готовий</Badge>
                  ) : (
                    <Badge variant="danger">Не налаштований</Badge>
                  )}
                </div>

                {/* Реквізити ПРРО — редагована форма */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <Input
                    label="Фіскальний номер ФН (prro_fn)"
                    value={prroForm.prro_fn}
                    onChange={(e) => setPrroForm({ ...prroForm, prro_fn: e.target.value })}
                    placeholder="5–15 цифр"
                    disabled={!prro.editable}
                  />
                  <Input
                    label="Податковий номер (prro_tn)"
                    value={prroForm.prro_tn}
                    onChange={(e) => setPrroForm({ ...prroForm, prro_tn: e.target.value })}
                    placeholder="5–20 символів"
                    disabled={!prro.editable}
                  />
                  <Input
                    label="Заводський номер (prro_zn)"
                    value={prroForm.prro_zn}
                    onChange={(e) => setPrroForm({ ...prroForm, prro_zn: e.target.value })}
                    placeholder="3–30 символів"
                    disabled={!prro.editable}
                  />
                  <div className="grid grid-cols-2 gap-3">
                    <Select
                      label="Режим"
                      value={prroForm.mode}
                      onChange={(e) =>
                        setPrroForm({ ...prroForm, mode: e.target.value === 'prod' ? 'prod' : 'test' })
                      }
                      options={[
                        { value: 'test', label: 'Тест' },
                        { value: 'prod', label: 'Продакшн' },
                      ]}
                      disabled={!prro.editable}
                    />
                    <div className="flex items-end pb-0.5 text-sm text-gray-400">
                      {prro.settings.mode === 'prod' ? 'prod' : 'test'}
                    </div>
                  </div>
                  <Input
                    label="URL фіскального сервера"
                    value={prroForm.url}
                    onChange={(e) => setPrroForm({ ...prroForm, url: e.target.value })}
                    placeholder="cabinet.tax.gov.ua:9443"
                    disabled={!prro.editable}
                  />
                </div>

                {/* Ключ ЕЦП точки: завантаження (файл) + пароль; секрети НЕ повертаються */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div className="p-4 rounded-lg border border-gray-200 dark:border-slate-600">
                    <p className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide">
                      Ключ ЕЦП (файл)
                    </p>
                    <div className="mt-2 flex items-center gap-2 flex-wrap">
                      {prro.key.file_configured ? (
                        <Badge variant="success">Завантажено</Badge>
                      ) : (
                        <Badge variant="danger">Не завантажено</Badge>
                      )}
                      {prro.key.file_name && (
                        <span className="text-sm font-mono text-gray-800 dark:text-gray-200">
                          {prro.key.file_name}
                        </span>
                      )}
                    </div>
                    <p className="mt-1 text-xs text-gray-400">
                      джерело: {prro.key.source === 'env' ? 'env' : prro.key.source === 'keystore' ? 'keystore' : '—'}
                    </p>
                    <input
                      type="file"
                      className="mt-3 block w-full text-sm text-gray-600 dark:text-gray-300 file:mr-3 file:rounded-lg file:border-0 file:bg-primary-50 file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-primary-700 hover:file:bg-primary-100 dark:file:bg-primary-900/30 dark:file:text-primary-300"
                      onChange={(e) => setPrroKeyFile(e.target.files?.[0] || null)}
                      accept=".jks,.dat,.key,.pfx,.p12"
                      disabled={!prro.editable}
                    />
                    {prroKeyFile && (
                      <p className="mt-1 text-xs text-primary-600">Обрано: {prroKeyFile.name}</p>
                    )}
                  </div>
                  <div className="p-4 rounded-lg border border-gray-200 dark:border-slate-600">
                    <p className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide">
                      Пароль ключа КЕП
                    </p>
                    <Input
                      type="password"
                      value={prroKeyPassword}
                      onChange={(e) => setPrroKeyPassword(e.target.value)}
                      placeholder={prro.key.password_configured ? 'Змінити пароль (опційно)' : 'Введіть пароль ключа'}
                      disabled={!prro.editable}
                      className="mt-2"
                    />
                    <p className="mt-1 text-xs text-gray-400">
                      {prro.key.password_configured
                        ? 'Пароль збережено (Fernet, per-store). У відповідях API пароль ніколи не передається.'
                        : 'Не збережено — укажіть пароль, якщо завантажуєте ключ.'}
                    </p>
                  </div>
                </div>

                {/* Стан: сертифікат/остання зміна (публічні атрибути) */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div className="p-4 rounded-lg border border-gray-200 dark:border-slate-600">
                    <p className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide">Сертифікат КЕП</p>
                    <p className="mt-1 text-sm text-gray-800 dark:text-gray-200 font-mono">
                      {prro.key.signer_serial || '—'}
                    </p>
                    <p className="mt-1 text-xs text-gray-400">{prro.key.signer_name || ''}</p>
                  </div>
                  {prro.last_shift && (
                    <div className="p-4 rounded-lg bg-gray-50 dark:bg-slate-700/50 border border-gray-100 dark:border-slate-600 flex flex-wrap items-center gap-x-6 gap-y-1 text-sm">
                      <span className="text-gray-500 dark:text-gray-400">Остання зміна:</span>
                      <span className="font-medium text-gray-900 dark:text-gray-100">№ {prro.last_shift.shift_number}</span>
                      <Badge variant={prro.last_shift.status === 'open' ? 'success' : 'default'}>
                        {prro.last_shift.status === 'open' ? 'Відкрита' : 'Закрита'}
                      </Badge>
                      <span className="text-gray-500 dark:text-gray-400">чеків: {prro.last_shift.receipt_count}</span>
                      {prro.last_shift.zreport_number && (
                        <span className="text-gray-500 dark:text-gray-400">Z-звіт: {prro.last_shift.zreport_number}</span>
                      )}
                    </div>
                  )}
                </div>

                {/* Збереження */}
                <div className="flex items-center justify-end gap-3 pt-2 border-t border-gray-200 dark:border-slate-700">
                  <p className="text-xs text-gray-400 mr-auto">
                    {prro.settings_updated_at
                      ? `Оновлено: ${formatDateTime(prro.settings_updated_at)}`
                      : 'Налаштувань ще не було'}
                  </p>
                  <Button
                    onClick={() => void handleSavePrro()}
                    isLoading={prroSaving}
                    disabled={!prro.editable || prroLoading}
                  >
                    <Save className="w-4 h-4 mr-2" />
                    Зберегти ПРРО
                  </Button>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      )}

      {/* ── Вкладка: Ціни (заглушка Етапи 5-6) ────── */}
      {tab === 'prices' && (
        <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-8 text-center">
          <Tag className="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            Ціни точки — Етапи 5-6
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400 max-w-lg mx-auto leading-relaxed">
            Перевизначення цін товарів по конкретній точці (store_product_prices)
            буде реалізовано на Етапах 5-6.
          </p>
        </div>
      )}

      {/* ── Confirm: архівація точки ──────────────── */}
      <ConfirmDialog
        isOpen={archiveOpen}
        onClose={() => setArchiveOpen(false)}
        onConfirm={() => void handleArchive()}
        title="Архівувати точку?"
        message={
          (store.devices_count ?? 0) > 0
            ? `Прив'язано ${store.devices_count} кас — після архівації вони перестануть синхронізуватись. Точку можна відновити пізніше, дані зберігаються.`
            : 'Точка перейде у статус «Архів». Дані зберігаються, точку можна відновити пізніше.'
        }
        confirmText="Архівувати"
        variant={store.devices_count ? 'warning' : 'danger'}
        isLoading={isArchiving}
      />

      {/* ── Модалка: новий працівник ──────────────── */}
      <Modal
        isOpen={workerModalOpen}
        onClose={() => setWorkerModalOpen(false)}
        title={`Новий працівник — ${store.name}`}
        size="md"
      >
        <div className="space-y-4">
          <Input
            label="ПІБ"
            value={workerForm.name}
            onChange={(e) => setWorkerForm({ ...workerForm, name: e.target.value })}
            placeholder="Повне ім'я"
          />
          <Input
            label="Логін"
            value={workerForm.login}
            onChange={(e) => setWorkerForm({ ...workerForm, login: e.target.value })}
            placeholder="Авто-генерація з імені, якщо порожньо"
          />
          <Input
            label="Пароль"
            type="password"
            value={workerForm.password}
            onChange={(e) => setWorkerForm({ ...workerForm, password: e.target.value })}
            placeholder="Мінімум 4 символи"
          />
          <Input
            label="PIN-код (необов'язково)"
            type="password"
            value={workerForm.pin_code}
            onChange={(e) => setWorkerForm({ ...workerForm, pin_code: e.target.value })}
            placeholder="4-10 символів"
          />
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <Select
              label="Роль у системі"
              value={workerForm.role}
              onChange={(e) => {
                const role = String(e.target.value);
                setWorkerForm((prev) => ({ ...prev, role, store_role: role }));
              }}
              options={GLOBAL_ROLE_OPTIONS}
            />
            <Select
              label="Роль на цій точці"
              value={workerForm.store_role}
              onChange={(e) => setWorkerForm({ ...workerForm, store_role: String(e.target.value) })}
              options={STORE_ROLE_OPTIONS}
            />
          </div>
          <p className="text-xs text-gray-400 dark:text-gray-500">
            Ролі адмінки: власник мережі / керуючий мережею. Ролі каси: адміністратор
            точки / касир. Деактивація не видаляє запис — працівника можна активувати.
          </p>
        </div>
        <div className="flex items-center justify-end gap-3 pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => setWorkerModalOpen(false)}>
            Скасувати
          </Button>
          <Button onClick={() => void handleCreateWorker()} isLoading={isSavingWorker}>
            <Plus className="w-4 h-4 mr-2" />
            Створити
          </Button>
        </div>
      </Modal>

      {/* ── Confirm: деактивація працівника ───────── */}
      <ConfirmDialog
        isOpen={!!deactivateTarget}
        onClose={() => setDeactivateTarget(null)}
        onConfirm={() => deactivateTarget && void handleToggleActive(deactivateTarget)}
        title="Деактивувати працівника?"
        message={
          deactivateTarget
            ? `Працівник «${deactivateTarget.name}» втратить доступ до системи. Запис НЕ видаляється — його можна активувати пізніше.`
            : ''
        }
        confirmText="Деактивувати"
        variant="warning"
        isLoading={workerActionBusy === deactivateTarget?.id}
      />

      {/* ── Модалка: скинути пароль ───────────────── */}
      <Modal
        isOpen={!!resetPwdTarget}
        onClose={() => setResetPwdTarget(null)}
        title={`Скинути пароль — ${resetPwdTarget?.name || ''}`}
        size="sm"
      >
        <Input
          label="Новий пароль"
          type="password"
          value={resetPwd}
          onChange={(e) => setResetPwd(e.target.value)}
          placeholder="Мінімум 4 символи"
        />
        <div className="flex items-center justify-end gap-3 pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => setResetPwdTarget(null)}>
            Скасувати
          </Button>
          <Button onClick={() => void handleResetPassword()} isLoading={workerActionBusy === resetPwdTarget?.id}>
            <KeyRound className="w-4 h-4 mr-2" />
            Зберегти
          </Button>
        </div>
      </Modal>

      {/* ── Модалка: скинути PIN ──────────────────── */}
      <Modal
        isOpen={!!resetPinTarget}
        onClose={() => setResetPinTarget(null)}
        title={`Скинути PIN — ${resetPinTarget?.name || ''}`}
        size="sm"
      >
        <Input
          label="Новий PIN-код"
          type="password"
          value={resetPin}
          onChange={(e) => setResetPin(e.target.value)}
          placeholder="4-10 символів"
        />
        <div className="flex items-center justify-end gap-3 pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => setResetPinTarget(null)}>
            Скасувати
          </Button>
          <Button onClick={() => void handleResetPin()} isLoading={workerActionBusy === resetPinTarget?.id}>
            <Hash className="w-4 h-4 mr-2" />
            Зберегти
          </Button>
        </div>
      </Modal>
    </div>
  );
};

export default StoreDetailPage;
