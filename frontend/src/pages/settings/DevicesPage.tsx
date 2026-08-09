import React, { useState, useEffect, useCallback } from 'react';
import {
  Plug,
  Plus,
  Trash2,
  RefreshCw,
  Scale,
  Wifi,
  Printer,
  ScanLine,
  CheckCircle2,
  XCircle,
  Loader2,
} from 'lucide-react';
import { isTauri } from '@/hooks/useTauri';
import { useDevicesStore } from '@/store/devicesStore';
import {
  DeviceConfig,
  DeviceStatus,
  DeviceType,
  PrinterInfo,
} from '@/services/tauri/devices';
import { Select, SelectOption } from '@/components/ui/Select';
import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { toast } from 'react-hot-toast';

// ─── Схеми динамічної форми (JSON Schema-based) ─────────────────────────────

interface DeviceFieldSchema {
  key: string;
  label: string;
  type: 'text' | 'number' | 'select' | 'checkbox';
  options?: SelectOption[];
  placeholder?: string;
}

interface DeviceTypeSchema {
  type: DeviceType;
  label: string;
  description: string;
  fields: DeviceFieldSchema[];
}

const BAUD_RATE_OPTIONS: SelectOption[] = [9600, 19200, 38400, 57600, 115200].map((v) => ({
  value: v,
  label: String(v),
}));

const DEVICE_SCHEMAS: Record<DeviceType, DeviceTypeSchema> = {
  scale: {
    type: 'scale',
    label: 'Касові ваги (COM)',
    description: 'Підключення через COM-порт (RS-232)',
    fields: [
      { key: 'name', label: 'Назва', type: 'text', placeholder: 'Наприклад: Ваги CAS LP-15' },
      { key: 'port', label: 'COM-порт', type: 'select' },
      {
        key: 'baudRate',
        label: 'Baud Rate',
        type: 'select',
        options: BAUD_RATE_OPTIONS,
      },
      { key: 'enabled', label: 'Автопідключення при старті', type: 'checkbox' },
    ],
  },
  terminal: {
    type: 'terminal',
    label: 'Термінал ПриватБанку (WiFi)',
    description: 'Платіжний термінал через мережу TCP/IP',
    fields: [
      { key: 'name', label: 'Назва', type: 'text', placeholder: 'Наприклад: Термінал каси 1' },
      { key: 'ip', label: 'IP-адреса', type: 'text', placeholder: '192.168.1.50' },
      { key: 'tcpPort', label: 'Порт', type: 'number', placeholder: '9100' },
      { key: 'enabled', label: 'Автопідключення при старті', type: 'checkbox' },
    ],
  },
  printer: {
    type: 'printer',
    label: 'Принтер чеків (CUPS)',
    description: 'CUPS-принтер для друку чеків',
    fields: [
      { key: 'name', label: 'Назва', type: 'text', placeholder: 'Наприклад: Принтер каси 1' },
      { key: 'printerName', label: 'Принтер', type: 'select' },
      { key: 'enabled', label: 'Автопідключення при старті', type: 'checkbox' },
    ],
  },
};

const DEVICE_TYPE_LABELS: Record<DeviceType, string> = {
  scale: 'Касові ваги',
  terminal: 'Термінал ПриватБанку',
  printer: 'Принтер чеків',
};

// ─── Хелпери статусу ─────────────────────────────────────────────────────────

interface StatusVisual {
  dot: string;
  text: string;
  textColor: string;
}

function getStatusVisual(status?: DeviceStatus): StatusVisual {
  if (!status || status.status === 'disconnected') {
    return {
      dot: 'bg-gray-400',
      text: 'Відключено',
      textColor: 'text-gray-500 dark:text-gray-400',
    };
  }
  if (status.status === 'connected') {
    return {
      dot: 'bg-green-500',
      text: 'Підключено',
      textColor: 'text-green-600 dark:text-green-400',
    };
  }
  return {
    dot: 'bg-red-500',
    text: 'Помилка',
    textColor: 'text-red-600 dark:text-red-400',
  };
}

/** Бейдж статусу CUPS-принтера: idle=🟢 Готовий, printing=🔵 Друкує,
 *  disabled=🔴 Вимкнено, error=🟠 Помилка */
function getPrinterStatusBadge(status: PrinterInfo['status']): {
  dot: string;
  label: string;
  badgeClass: string;
} {
  switch (status) {
    case 'idle':
      return {
        dot: 'bg-green-500',
        label: 'Готовий',
        badgeClass: 'bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-400 border-green-200 dark:border-green-800',
      };
    case 'printing':
      return {
        dot: 'bg-blue-500',
        label: 'Друкує',
        badgeClass: 'bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-400 border-blue-200 dark:border-blue-800',
      };
    case 'disabled':
      return {
        dot: 'bg-red-500',
        label: 'Вимкнено',
        badgeClass: 'bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-400 border-red-200 dark:border-red-800',
      };
    case 'error':
      return {
        dot: 'bg-orange-500',
        label: 'Помилка',
        badgeClass: 'bg-orange-50 dark:bg-orange-900/20 text-orange-700 dark:text-orange-400 border-orange-200 dark:border-orange-800',
      };
  }
}

/** Типізований доступ до поля draft:
 *  'name'/'enabled'/'deviceType' зберігаються в TOP-LEVEL draft (updateDraft),
 *  решта (port, baudRate, ip, tcpPort, printerName) — у config.
 *  ⚠️ НЕВІДПОВІДНІСТЬ «пишуть в одне місце, читають з іншого» раніше
 *  ламала поле name (value завжди '') та checkbox enabled (checked завжди false). */
type DeviceConfigValue = string | number | boolean | undefined;
const getConfigValue = (cfg: DeviceConfig, key: string): DeviceConfigValue => {
  if (key === 'name' || key === 'enabled' || key === 'deviceType') {
    return (cfg as unknown as Record<string, DeviceConfigValue>)[key];
  }
  return (cfg.config as Record<string, DeviceConfigValue>)[key];
};

// ─── Сторінка ────────────────────────────────────────────────────────────────

const DevicesPage: React.FC = () => {
  const isDesktop = isTauri();

  const devices = useDevicesStore((s) => s.devices);
  const statuses = useDevicesStore((s) => s.statuses);
  const weights = useDevicesStore((s) => s.weights);
  const availablePorts = useDevicesStore((s) => s.availablePorts);
  const detected = useDevicesStore((s) => s.detected);
  const detectedLoading = useDevicesStore((s) => s.detectedLoading);
  const systemPrinters = useDevicesStore((s) => s.systemPrinters);
  const loading = useDevicesStore((s) => s.loading);
  const error = useDevicesStore((s) => s.error);
  const loadDevices = useDevicesStore((s) => s.loadDevices);
  const loadAvailablePorts = useDevicesStore((s) => s.loadAvailablePorts);
  const loadDetected = useDevicesStore((s) => s.loadDetected);
  const loadSystemPrinters = useDevicesStore((s) => s.loadSystemPrinters);
  const saveDevice = useDevicesStore((s) => s.saveDevice);
  const deleteDevice = useDevicesStore((s) => s.deleteDevice);
  const connectDevice = useDevicesStore((s) => s.connectDevice);
  const disconnectDevice = useDevicesStore((s) => s.disconnectDevice);
  const testConnection = useDevicesStore((s) => s.testConnection);
  const quickAddPrinter = useDevicesStore((s) => s.quickAddPrinter);
  const initListeners = useDevicesStore((s) => s.initListeners);

  // Локальний стан UI
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<DeviceConfig | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [addType, setAddType] = useState<DeviceType>('scale');
  const [showTypePicker, setShowTypePicker] = useState(false);
  const [portsLoading, setPortsLoading] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<boolean | null>(null);

  // ── Монтування: пристрої + виявлення + підписки ──
  useEffect(() => {
    loadDevices();
    loadDetected();
    loadSystemPrinters();
    loadAvailablePorts().catch(() => {});
    const cleanup = initListeners();
    return cleanup;
  }, [loadDevices, loadDetected, loadSystemPrinters, loadAvailablePorts, initListeners]);

  const refreshPorts = useCallback(async () => {
    setPortsLoading(true);
    try {
      await loadAvailablePorts();
    } finally {
      setPortsLoading(false);
    }
  }, [loadAvailablePorts]);

  /** Оновити виявлені пристрої (кнопка у заголовку) */
  const handleRefreshDetected = useCallback(async () => {
    await Promise.all([loadDetected(), loadSystemPrinters()]);
  }, [loadDetected, loadSystemPrinters]);

  // ── Вибір пристрою в sidebar ──
  const handleSelect = useCallback(
    (id: string) => {
      const device = devices.find((d) => d.id === id);
      if (!device) return;
      setSelectedId(id);
      setDraft({
        ...device,
        config: { ...device.config },
      });
      setTestResult(null);
    },
    [devices]
  );

  // ── Видалення ──
  const handleDelete = useCallback(
    async (id: string) => {
      if (!window.confirm('Видалити пристрій?')) return;
      const ok = await deleteDevice(id);
      if (ok) {
        if (selectedId === id) {
          setSelectedId(null);
          setDraft(null);
        }
      }
    },
    [deleteDevice, selectedId]
  );

  // ── Швидке додавання виявлених ──
  const handleQuickAddPrinter = useCallback(
    async (printer: PrinterInfo) => {
      const saved = await quickAddPrinter(printer);
      if (saved) {
        setSelectedId(saved.id);
        setDraft({ ...saved, config: { ...saved.config } });
        toast.success(`Принтер «${printer.name}» додано`);
      }
    },
    [quickAddPrinter]
  );

  // ── Додавання: відкриття модалки з вибором типу ──
  const openAddModal = useCallback(() => {
    setShowTypePicker(true);
    setAddType('scale');
    setDraft(null);
    setTestResult(null);
    setAddOpen(true);
  }, []);

  const pickType = useCallback((type: DeviceType) => {
    setAddType(type);
    setShowTypePicker(false);
    if (type === 'scale') {
      setDraft({
        id: '',
        name: '',
        deviceType: type,
        enabled: false,
        config: { port: '', baudRate: 9600 },
      });
    } else if (type === 'terminal') {
      setDraft({
        id: '',
        name: '',
        deviceType: type,
        enabled: false,
        config: { ip: '', tcpPort: 9100 },
      });
    } else {
      setDraft({
        id: '',
        name: '',
        deviceType: type,
        enabled: true,
        config: { printerName: '' },
      });
    }
  }, []);

  // ── Оновлення поля draft ──
  const updateDraft = useCallback((field: string, value: string | number | boolean) => {
    setDraft((prev) => {
      if (!prev) return prev;
      if (field === 'name' || field === 'enabled' || field === 'deviceType') {
        return { ...prev, [field]: value };
      }
      return { ...prev, config: { ...prev.config, [field]: value } };
    });
  }, []);

  // ── Збереження ──
  const handleSave = useCallback(async () => {
    if (!draft) return;
    const saved = await saveDevice(draft);
    if (saved) {
      setSelectedId(saved.id);
      setDraft({ ...saved, config: { ...saved.config } });
      setAddOpen(false);
    }
  }, [draft, saveDevice]);

  // ── Підключити / Відключити ──
  const handleToggleConnect = useCallback(async () => {
    if (!draft) return;
    const current = statuses[draft.id];
    if (current?.status === 'connected') {
      await disconnectDevice(draft.id);
    } else {
      await connectDevice(draft.id);
    }
  }, [draft, statuses, connectDevice, disconnectDevice]);

  // ── Test Connection (термінал) ──
  const handleTest = useCallback(async () => {
    if (!draft) return;
    setTesting(true);
    setTestResult(null);
    try {
      const ok = await testConnection(draft.deviceType, draft.config);
      setTestResult(ok);
      if (ok) {
        toast.success('З’єднання успішне');
      } else {
        toast.error('Не вдалося встановити з’єднання');
      }
    } finally {
      setTesting(false);
    }
  }, [draft, testConnection]);

  // ── Похідні дані ──
  const detectedPrinters = detected?.printers || [];
  const detectedScanners = detected?.scanners || [];

  /** Опції принтерів для Select у формі (мітка: name + (за замовчуванням)) */
  const printerFormPrinters =
    systemPrinters.length > 0 ? systemPrinters : detectedPrinters;
  const printerFormOptions: SelectOption[] = printerFormPrinters.map((p) => ({
    value: p.name,
    label: p.isDefault ? `${p.name} (за замовчуванням)` : p.name,
  }));

  /** Чи вже додано принтер у "Мої пристрої" */
  const isPrinterAdded = useCallback(
    (name: string) =>
      devices.some(
        (d) => d.deviceType === 'printer' && d.config.printerName === name
      ),
    [devices]
  );

  const selectedDevice = selectedId ? devices.find((d) => d.id === selectedId) : undefined;
  const schema = draft ? DEVICE_SCHEMAS[draft.deviceType] : undefined;
  const currentStatus = selectedId ? statuses[selectedId] : undefined;
  const statusVisual = getStatusVisual(currentStatus);
  const currentWeight = selectedId ? weights[selectedId] : undefined;

  // ── Заглушка для браузера ──
  if (!isDesktop) {
    return (
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6">
        <div className="flex items-center gap-3 mb-6">
          <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
            <Plug className="w-5 h-5" />
          </div>
          <div>
            <h1 className="text-xl font-bold text-gray-900 dark:text-white">Підключені пристрої</h1>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Конфігурація та моніторинг POS-обладнання
            </p>
          </div>
        </div>
        <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-10 text-center">
          <Plug className="w-12 h-12 mx-auto text-gray-300 dark:text-slate-600 mb-4" />
          <p className="text-lg font-medium text-gray-900 dark:text-white">
            Доступно лише в десктоп-версії Torgashka
          </p>
          <p className="mt-2 text-sm text-gray-500 dark:text-gray-400 max-w-md mx-auto">
            Цей розділ працює через Tauri Desktop API. Відкрийте застосунок Torgashka на
            комп'ютері, щоб налаштувати касові ваги, платіжні термінали та принтери чеків.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6">
      {/* ── Заголовок ── */}
      <div className="flex flex-wrap items-center justify-between gap-3 mb-6">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
            <Plug className="w-5 h-5" />
          </div>
          <div>
            <h1 className="text-xl font-bold text-gray-900 dark:text-white">Підключені пристрої</h1>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Конфігурація та моніторинг POS-обладнання
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <Button
            type="button"
            variant="secondary"
            onClick={handleRefreshDetected}
            disabled={detectedLoading}
            className="flex items-center gap-2"
          >
            {detectedLoading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <RefreshCw className="w-4 h-4" />
            )}
            Оновити виявлені
          </Button>
          <Button onClick={openAddModal} className="flex items-center gap-2">
            <Plus className="w-4 h-4" />
            Додати пристрій
          </Button>
        </div>
      </div>

      {error && (
        <div className="mb-4 px-4 py-3 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 text-sm text-red-700 dark:text-red-400">
          {error}
        </div>
      )}

      {/* ══ СЕКЦІЯ A: ВИЯВЛЕНІ ПРИСТРОЇ ══ */}
      <section className="mb-8">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">
          Виявлені пристрої
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* ── Принтери (CUPS) ── */}
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
            <div className="px-4 py-3 border-b border-gray-200 dark:border-slate-700 flex items-center gap-2">
              <Printer className="w-4 h-4 text-primary-600 dark:text-primary-400" />
              <span className="text-sm font-semibold text-gray-900 dark:text-white">
                Принтери (CUPS)
              </span>
              <span className="ml-auto text-xs text-gray-400 dark:text-slate-500">
                {detectedPrinters.length}
              </span>
            </div>
            <div className="divide-y divide-gray-100 dark:divide-slate-700/50 max-h-64 overflow-y-auto">
              {detectedPrinters.length === 0 && (
                <div className="px-4 py-6 text-sm text-gray-400 dark:text-slate-500 text-center">
                  Принтери не знайдені
                </div>
              )}
              {detectedPrinters.map((printer) => {
                const badge = getPrinterStatusBadge(printer.status);
                const added = isPrinterAdded(printer.name);
                return (
                  <div key={printer.name} className="px-4 py-2.5 flex items-start gap-2">
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium text-gray-900 dark:text-white truncate flex items-center gap-2">
                        {printer.name}
                        {printer.isDefault && (
                          <span className="flex-shrink-0 text-[10px] px-1.5 py-0.5 rounded-full bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400 border border-gray-200 dark:border-slate-600">
                            за замовчуванням
                          </span>
                        )}
                      </p>
                      <span
                        className={`inline-flex items-center gap-1.5 mt-1 text-[11px] px-2 py-0.5 rounded-full border ${badge.badgeClass}`}
                      >
                        <span className={`w-1.5 h-1.5 rounded-full ${badge.dot}`} />
                        {badge.label}
                      </span>
                    </div>
                    {!added ? (
                      <button
                        type="button"
                        onClick={() => handleQuickAddPrinter(printer)}
                        title="Додати до моїх пристроїв"
                        className="flex-shrink-0 flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-lg border border-gray-300 dark:border-slate-600 text-gray-600 dark:text-gray-300 hover:bg-primary-50 dark:hover:bg-primary-900/20 hover:border-primary-500 dark:hover:border-primary-500 transition-colors"
                      >
                        <Plus className="w-3.5 h-3.5" /> Додати
                      </button>
                    ) : (
                      <span className="flex-shrink-0 text-xs text-green-600 dark:text-green-400 mt-1">
                        Додано
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          {/* ── Сканери ── */}
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
            <div className="px-4 py-3 border-b border-gray-200 dark:border-slate-700 flex items-center gap-2">
              <ScanLine className="w-4 h-4 text-primary-600 dark:text-primary-400" />
              <span className="text-sm font-semibold text-gray-900 dark:text-white">
                Сканери
              </span>
              <span className="ml-auto text-xs text-gray-400 dark:text-slate-500">
                {detectedScanners.length}
              </span>
            </div>
            <div className="divide-y divide-gray-100 dark:divide-slate-700/50 max-h-64 overflow-y-auto">
              {detectedScanners.length === 0 ? (
                <div className="px-4 py-6 flex items-center justify-center gap-2 text-sm text-gray-400 dark:text-slate-500">
                  <ScanLine className="w-4 h-4" />
                  Сканери не знайдені
                </div>
              ) : (
                detectedScanners.map((scanner) => (
                  <div key={scanner.device} className="px-4 py-2.5">
                    <p className="text-sm font-medium text-gray-900 dark:text-white truncate">
                      {scanner.name}
                    </p>
                    <p className="text-xs text-gray-500 dark:text-gray-400 truncate font-mono">
                      {scanner.device}
                    </p>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </section>

      {/* ══ СЕКЦІЯ B: МОЇ ПРИСТРОЇ ══ */}
      <section>
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">
          Мої пристрої
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-[18rem_1fr] gap-6">
          {/* ── Ліва колонка: список пристроїв ── */}
          <aside className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden h-fit">
            <div className="px-4 py-3 border-b border-gray-200 dark:border-slate-700">
              <span className="text-sm font-semibold text-gray-900 dark:text-white">
                Пристрої ({devices.length})
              </span>
            </div>

            <div className="divide-y divide-gray-100 dark:divide-slate-700/50">
              {loading && devices.length === 0 && (
                <div className="px-4 py-8 text-center text-gray-400 dark:text-slate-500">
                  <Loader2 className="w-6 h-6 mx-auto animate-spin mb-2" />
                  Завантаження…
                </div>
              )}
              {!loading && devices.length === 0 && (
                <div className="px-4 py-8 text-center text-sm text-gray-400 dark:text-slate-500">
                  Немає пристроїв.
                  <br />
                  Додайте пристрій зі списку виявлених
                  <br />
                  або натисніть «+ Додати пристрій»
                </div>
              )}
              {devices.map((device) => {
                const st = getStatusVisual(statuses[device.id]);
                const isSelected = selectedId === device.id;
                return (
                  <div
                    key={device.id}
                    role="button"
                    tabIndex={0}
                    onClick={() => handleSelect(device.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') handleSelect(device.id);
                    }}
                    className={`
                      w-full text-left px-4 py-3 flex items-start gap-3 cursor-pointer transition-colors
                      ${isSelected
                        ? 'bg-primary-50 dark:bg-primary-900/20 border-l-2 border-primary-500'
                        : 'border-l-2 border-transparent hover:bg-gray-50 dark:hover:bg-slate-700/50'}
                    `}
                  >
                    <span className="mt-1.5 flex-shrink-0">
                      <span className={`block w-2.5 h-2.5 rounded-full ${st.dot}`} title={st.text} />
                    </span>
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium text-gray-900 dark:text-white truncate">
                        {device.name || DEVICE_TYPE_LABELS[device.deviceType]}
                      </p>
                      <p className={`text-xs ${st.textColor}`}>
                        {DEVICE_TYPE_LABELS[device.deviceType]} · {st.text}
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDelete(device.id);
                      }}
                      title="Видалити пристрій"
                      className="p-1.5 rounded-lg text-gray-300 hover:text-red-500 dark:text-slate-600 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors flex-shrink-0"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                );
              })}
            </div>
          </aside>

          {/* ── Права панель: форма / плейсхолдер ── */}
          <section className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-6">
            {!draft || !schema ? (
              <div className="h-full min-h-[24rem] flex flex-col items-center justify-center text-center">
                <Plug className="w-12 h-12 text-gray-300 dark:text-slate-600 mb-4" />
                <p className="text-lg font-medium text-gray-900 dark:text-white">
                  Оберіть пристрій або додайте новий
                </p>
                <p className="mt-2 text-sm text-gray-500 dark:text-gray-400 max-w-sm">
                  Додайте пристрій зі списку виявлених або натисніть «+ Додати пристрій»,
                  щоб налаштувати касові ваги, термінал або принтер чеків.
                </p>
              </div>
            ) : (
              <div className="space-y-5">
                {/* Заголовок форми */}
                <div className="flex items-center gap-3 pb-4 border-b border-gray-200 dark:border-slate-700">
                  <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
                    {draft.deviceType === 'scale' ? (
                      <Scale className="w-5 h-5" />
                    ) : draft.deviceType === 'terminal' ? (
                      <Wifi className="w-5 h-5" />
                    ) : (
                      <Printer className="w-5 h-5" />
                    )}
                  </div>
                  <div className="flex-1">
                    <h2 className="text-lg font-semibold text-gray-900 dark:text-white">
                      {draft.name || schema.label}
                    </h2>
                    <p className={`text-sm ${statusVisual.textColor} flex items-center gap-1.5`}>
                      <span className={`inline-block w-2 h-2 rounded-full ${statusVisual.dot}`} />
                      {statusVisual.text}
                      {currentStatus?.error && (
                        <span className="text-xs text-red-500 truncate" title={currentStatus.error}>
                          — {currentStatus.error}
                        </span>
                      )}
                    </p>
                  </div>
                </div>

                {/* Поточна вага (scale, через listen) */}
                {draft.deviceType === 'scale' && currentWeight !== undefined && (
                  <div className="flex items-center gap-3 px-4 py-3 rounded-xl bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800">
                    <Scale className="w-5 h-5 text-green-600 dark:text-green-400" />
                    <div>
                      <p className="text-xs text-green-700 dark:text-green-400">Поточна вага</p>
                      <p className="text-lg font-bold text-green-800 dark:text-green-300">
                        {currentWeight.toFixed(2)} кг
                      </p>
                    </div>
                  </div>
                )}

                {/* Динамічна форма за DEVICE_SCHEMAS */}
                <div className="space-y-4">
                  {schema.fields.map((field) => (
                    <div key={field.key}>
                      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                        {field.label}
                      </label>
                      {field.type === 'checkbox' ? (
                        <label className="flex items-center gap-2 cursor-pointer select-none">
                          <input
                            type="checkbox"
                            checked={Boolean(getConfigValue(draft, field.key))}
                            onChange={(e) => updateDraft(field.key, e.target.checked)}
                            className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500 dark:border-slate-600 dark:bg-slate-700"
                          />
                          <span className="text-sm text-gray-600 dark:text-gray-400">
                            {field.label}
                          </span>
                        </label>
                      ) : field.key === 'port' ? (
                        <div className="flex gap-2">
                          <div className="flex-1">
                            <Select
                              options={[
                                { value: '', label: 'Оберіть порт…' },
                                ...availablePorts.map((p) => ({ value: p, label: p })),
                              ]}
                              value={String(getConfigValue(draft, 'port') || '')}
                              onChange={(e) => updateDraft('port', e.target.value)}
                              placeholder="Оберіть порт…"
                            />
                          </div>
                          <button
                            type="button"
                            onClick={refreshPorts}
                            disabled={portsLoading}
                            title="Оновити список портів"
                            className="flex-shrink-0 px-3 rounded-lg border border-gray-300 dark:border-slate-600 text-gray-500 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-slate-700 transition-colors"
                          >
                            {portsLoading ? (
                              <Loader2 className="w-4 h-4 animate-spin" />
                            ) : (
                              <RefreshCw className="w-4 h-4" />
                            )}
                          </button>
                        </div>
                      ) : field.key === 'printerName' ? (
                        <Select
                          options={printerFormOptions}
                          value={String(getConfigValue(draft, 'printerName') || '')}
                          onChange={(e) => updateDraft('printerName', e.target.value)}
                          placeholder="Оберіть принтер…"
                        />
                      ) : field.type === 'select' ? (
                        <Select
                          options={field.options || []}
                          value={String(getConfigValue(draft, field.key) ?? '')}
                          onChange={(e) => {
                            const raw = e.target.value;
                            const num = Number(raw);
                            updateDraft(field.key, Number.isNaN(num) || raw === '' ? raw : num);
                          }}
                        />
                      ) : (
                        <Input
                          type={field.type === 'number' ? 'number' : 'text'}
                          value={String(getConfigValue(draft, field.key) ?? '')}
                          onChange={(e) => {
                            const raw = e.target.value;
                            updateDraft(
                              field.key,
                              field.type === 'number' ? (raw === '' ? '' : Number(raw)) : raw
                            );
                          }}
                          placeholder={field.placeholder}
                          className="w-full"
                        />
                      )}
                    </div>
                  ))}
                </div>

                {/* Test Connection (тільки terminal) */}
                {draft.deviceType === 'terminal' && (
                  <div className="flex items-center gap-3">
                    <Button
                      type="button"
                      variant="secondary"
                      onClick={handleTest}
                      disabled={testing}
                      className="flex items-center gap-2"
                    >
                      {testing ? (
                        <Loader2 className="w-4 h-4 animate-spin" />
                      ) : (
                        <RefreshCw className="w-4 h-4" />
                      )}
                      Test Connection
                    </Button>
                    {testResult === true && (
                      <span className="flex items-center gap-1.5 text-sm text-green-600 dark:text-green-400">
                        <CheckCircle2 className="w-4 h-4" /> З’єднання успішне
                      </span>
                    )}
                    {testResult === false && (
                      <span className="flex items-center gap-1.5 text-sm text-red-600 dark:text-red-400">
                        <XCircle className="w-4 h-4" /> Помилка з’єднання
                      </span>
                    )}
                  </div>
                )}

                {/* Кнопки дій */}
                <div className="flex items-center gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
                  <Button onClick={handleSave} className="flex items-center gap-2">
                    Зберегти
                  </Button>
                  {draft.id && (
                    <Button
                      type="button"
                      variant={currentStatus?.status === 'connected' ? 'secondary' : 'primary'}
                      onClick={handleToggleConnect}
                      className="flex items-center gap-2"
                    >
                      {currentStatus?.status === 'connected' ? (
                        <>
                          <XCircle className="w-4 h-4" /> Відключити
                        </>
                      ) : (
                        <>
                          <Plug className="w-4 h-4" /> Підключити
                        </>
                      )}
                    </Button>
                  )}
                </div>
              </div>
            )}
          </section>
        </div>
      </section>

      {/* ── Модалка додавання пристрою ── */}
      <Modal
        isOpen={addOpen}
        onClose={() => setAddOpen(false)}
        title={showTypePicker ? 'Додати пристрій' : `Новий пристрій: ${DEVICE_SCHEMAS[addType].label}`}
        size="lg"
      >
        {showTypePicker ? (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 p-1">
            <button
              type="button"
              onClick={() => pickType('scale')}
              className="p-5 rounded-xl border-2 border-gray-200 dark:border-slate-700 hover:border-primary-500 dark:hover:border-primary-500 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-all text-left"
            >
              <Scale className="w-8 h-8 text-primary-600 dark:text-primary-400 mb-3" />
              <p className="font-semibold text-gray-900 dark:text-white">Касові ваги (COM)</p>
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                Підключення через COM-порт (RS-232) для автоматичного зчитування ваги
              </p>
            </button>
            <button
              type="button"
              onClick={() => pickType('terminal')}
              className="p-5 rounded-xl border-2 border-gray-200 dark:border-slate-700 hover:border-primary-500 dark:hover:border-primary-500 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-all text-left"
            >
              <Wifi className="w-8 h-8 text-primary-600 dark:text-primary-400 mb-3" />
              <p className="font-semibold text-gray-900 dark:text-white">Термінал ПриватБанку (WiFi)</p>
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                Платіжний термінал через мережу TCP/IP для прийому оплати
              </p>
            </button>
            <button
              type="button"
              onClick={() => pickType('printer')}
              className="p-5 rounded-xl border-2 border-gray-200 dark:border-slate-700 hover:border-primary-500 dark:hover:border-primary-500 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-all text-left sm:col-span-2"
            >
              <Printer className="w-8 h-8 text-primary-600 dark:text-primary-400 mb-3" />
              <p className="font-semibold text-gray-900 dark:text-white">Принтер чеків (CUPS)</p>
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                CUPS-принтер для друку чеків (оберіть зі списку системних принтерів)
              </p>
            </button>
          </div>
        ) : (
          draft && (
            <div className="space-y-4">
              {DEVICE_SCHEMAS[draft.deviceType].fields.map((field) => (
                <div key={field.key}>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                    {field.label}
                  </label>
                  {field.type === 'checkbox' ? (
                    <label className="flex items-center gap-2 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={Boolean(getConfigValue(draft, field.key))}
                        onChange={(e) => updateDraft(field.key, e.target.checked)}
                        className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500 dark:border-slate-600 dark:bg-slate-700"
                      />
                      <span className="text-sm text-gray-600 dark:text-gray-400">{field.label}</span>
                    </label>
                  ) : field.key === 'port' ? (
                    <div className="flex gap-2">
                      <div className="flex-1">
                        <Select
                          options={[
                            { value: '', label: 'Оберіть порт…' },
                            ...availablePorts.map((p) => ({ value: p, label: p })),
                          ]}
                          value={String(getConfigValue(draft, 'port') || '')}
                          onChange={(e) => updateDraft('port', e.target.value)}
                          placeholder="Оберіть порт…"
                        />
                      </div>
                      <button
                        type="button"
                        onClick={refreshPorts}
                        disabled={portsLoading}
                        title="Оновити список портів"
                        className="flex-shrink-0 px-3 rounded-lg border border-gray-300 dark:border-slate-600 text-gray-500 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-slate-700 transition-colors"
                      >
                        {portsLoading ? (
                          <Loader2 className="w-4 h-4 animate-spin" />
                        ) : (
                          <RefreshCw className="w-4 h-4" />
                        )}
                      </button>
                    </div>
                  ) : field.key === 'printerName' ? (
                    <Select
                      options={printerFormOptions}
                      value={String(getConfigValue(draft, 'printerName') || '')}
                      onChange={(e) => updateDraft('printerName', e.target.value)}
                      placeholder="Оберіть принтер…"
                    />
                  ) : field.type === 'select' ? (
                    <Select
                      options={field.options || []}
                      value={String(getConfigValue(draft, field.key) ?? '')}
                      onChange={(e) => {
                        const raw = e.target.value;
                        const num = Number(raw);
                        updateDraft(field.key, Number.isNaN(num) || raw === '' ? raw : num);
                      }}
                    />
                  ) : (
                    <Input
                      type={field.type === 'number' ? 'number' : 'text'}
                      value={String(getConfigValue(draft, field.key) ?? '')}
                      onChange={(e) => {
                        const raw = e.target.value;
                        updateDraft(
                          field.key,
                          field.type === 'number' ? (raw === '' ? '' : Number(raw)) : raw
                        );
                      }}
                      placeholder={field.placeholder}
                      className="w-full"
                    />
                  )}
                </div>
              ))}
              <div className="flex items-center justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
                <Button variant="secondary" onClick={() => setAddOpen(false)}>
                  Скасувати
                </Button>
                <Button onClick={handleSave} className="flex items-center gap-2">
                  Зберегти
                </Button>
              </div>
            </div>
          )
        )}
      </Modal>
    </div>
  );
};

export default DevicesPage;
