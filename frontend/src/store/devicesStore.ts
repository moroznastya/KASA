import { create } from 'zustand';
import {
  devicesApi,
  devicesEvents,
  DeviceConfig,
  DeviceStatus,
  DeviceType,
  DetectedDevices,
  PrinterInfo,
} from '@/services/tauri/devices';
import toast from 'react-hot-toast';

interface DevicesStore {
  /** Збережені конфігурації пристроїв */
  devices: DeviceConfig[];
  /** Статуси по id пристрою */
  statuses: Record<string, DeviceStatus>;
  /** Поточна вага по deviceId (оновлюється через listen "weight-updated") */
  weights: Record<string, number>;
  /** Доступні COM-порти системи */
  availablePorts: string[];
  /** Виявлені пристрої (автовиявлення: принтери, COM-порти, USB) */
  detected: DetectedDevices | null;
  detectedLoading: boolean;
  /** CUPS-принтери системи (для Select у формі printer) */
  systemPrinters: PrinterInfo[];
  loading: boolean;
  error: string | null;

  // Дії
  loadDevices: () => Promise<void>;
  loadAvailablePorts: () => Promise<string[]>;
  loadDetected: () => Promise<void>;
  loadSystemPrinters: () => Promise<PrinterInfo[]>;
  saveDevice: (config: DeviceConfig) => Promise<DeviceConfig | null>;
  deleteDevice: (id: string) => Promise<boolean>;
  connectDevice: (id: string) => Promise<DeviceStatus | null>;
  disconnectDevice: (id: string) => Promise<DeviceStatus | null>;
  testConnection: (type: DeviceType, config: object) => Promise<boolean>;
  /** Швидке додавання виявленого принтера до "Мої пристрої" */
  quickAddPrinter: (printer: PrinterInfo) => Promise<DeviceConfig | null>;
  /** Швидке додавання ваг за COM-портом до "Мої пристрої" */
  quickAddScale: (port: string) => Promise<DeviceConfig | null>;
  /** Підписатися на події пристроїв. Повертає cleanup-функцію для useEffect */
  initListeners: () => () => void;
  clearError: () => void;
}

export const useDevicesStore = create<DevicesStore>((set, get) => ({
  devices: [],
  statuses: {},
  weights: {},
  availablePorts: [],
  detected: null,
  detectedLoading: false,
  systemPrinters: [],
  loading: false,
  error: null,

  /** Завантажити конфігурації + статуси пристроїв */
  loadDevices: async () => {
    set({ loading: true, error: null });
    try {
      const [devices, statusList] = await Promise.all([
        devicesApi.getDevices(),
        devicesApi.getDevicesStatus(),
      ]);
      const statuses: Record<string, DeviceStatus> = {};
      statusList.forEach((s) => {
        statuses[s.id] = s;
      });
      set({ devices, statuses });
    } catch (error: any) {
      set({ error: error?.message || 'Помилка завантаження пристроїв' });
    } finally {
      set({ loading: false });
    }
  },

  /** Оновити список доступних COM-портів */
  loadAvailablePorts: async () => {
    try {
      const ports = await devicesApi.getAvailablePorts();
      set({ availablePorts: ports });
      return ports;
    } catch (error: any) {
      set({ error: error?.message || 'Помилка отримання списку COM-портів' });
      return [];
    }
  },

  /** Автовиявлення всіх підключених пристроїв (get_detected_devices) */
  loadDetected: async () => {
    set({ detectedLoading: true, error: null });
    try {
      const detected = await devicesApi.getDetectedDevices();
      set({ detected });
    } catch (error: any) {
      set({ error: error?.message || 'Помилка виявлення пристроїв' });
    } finally {
      set({ detectedLoading: false });
    }
  },

  /** Отримати CUPS-принтери системи (для Select у формі printer) */
  loadSystemPrinters: async () => {
    try {
      const printers = await devicesApi.getSystemPrinters();
      set({ systemPrinters: printers });
      return printers;
    } catch (error: any) {
      set({ error: error?.message || 'Помилка отримання списку принтерів' });
      return [];
    }
  },

  /** Зберегти конфігурацію пристрою (створити або оновити) */
  saveDevice: async (config) => {
    try {
      const saved = await devicesApi.saveDeviceConfig(config);
      set((state) => {
        const exists = state.devices.some((d) => d.id === saved.id);
        return {
          devices: exists
            ? state.devices.map((d) => (d.id === saved.id ? saved : d))
            : [...state.devices, saved],
        };
      });
      toast.success('Пристрій збережено');
      return saved;
    } catch (error: any) {
      const msg = error?.message || 'Помилка збереження пристрою';
      set({ error: msg });
      toast.error(msg);
      return null;
    }
  },

  /** Видалити пристрій */
  deleteDevice: async (id) => {
    try {
      await devicesApi.deleteDevice(id);
      set((state) => {
        const statuses = { ...state.statuses };
        const weights = { ...state.weights };
        delete statuses[id];
        delete weights[id];
        return {
          devices: state.devices.filter((d) => d.id !== id),
          statuses,
          weights,
        };
      });
      toast.success('Пристрій видалено');
      return true;
    } catch (error: any) {
      const msg = error?.message || 'Помилка видалення пристрою';
      set({ error: msg });
      toast.error(msg);
      return false;
    }
  },

  /** Підключитися до пристрою */
  connectDevice: async (id) => {
    try {
      const status = await devicesApi.connectDevice(id);
      set((state) => ({ statuses: { ...state.statuses, [id]: status } }));
      return status;
    } catch (error: any) {
      const msg = error?.message || 'Помилка підключення пристрою';
      set({ error: msg });
      toast.error(msg);
      return null;
    }
  },

  /** Відключитися від пристрою */
  disconnectDevice: async (id) => {
    try {
      const status = await devicesApi.disconnectDevice(id);
      set((state) => ({ statuses: { ...state.statuses, [id]: status } }));
      return status;
    } catch (error: any) {
      const msg = error?.message || 'Помилка відключення пристрою';
      set({ error: msg });
      toast.error(msg);
      return null;
    }
  },

  /** Перевірити з'єднання (test_connection) — повертає boolean */
  testConnection: async (type, config) => {
    try {
      return await devicesApi.testConnection(type, config);
    } catch (error: any) {
      set({ error: error?.message || 'Помилка перевірки з’єднання' });
      return false;
    }
  },

  /**
   * Швидке додавання виявленого CUPS-принтера до "Мої пристрої".
   * Створює DeviceConfig { id:'', name, deviceType:'printer', enabled:true,
   * config:{ printerName } } та зберігає через saveDevice().
   */
  quickAddPrinter: async (printer) => {
    const config: DeviceConfig = {
      id: '',
      name: printer.name,
      deviceType: 'printer',
      enabled: true,
      config: { printerName: printer.name },
    };
    return get().saveDevice(config);
  },

  /**
   * Швидке додавання ваг за виявленим COM-портом до "Мої пристрої".
   * Створює DeviceConfig { id:'', name: `Ваги (${port})`, deviceType:'scale',
   * enabled:false, config:{ port, baudRate:9600 } } та зберігає через saveDevice().
   */
  quickAddScale: async (port) => {
    const config: DeviceConfig = {
      id: '',
      name: `Ваги (${port})`,
      deviceType: 'scale',
      enabled: false,
      config: { port, baudRate: 9600, protocol: 'vta2' },
    };
    return get().saveDevice(config);
  },

  /**
   * Підписатися на події пристроїв:
   *   - "device-status-changed" → оновлення statuses
   *   - "weight-updated"        → оновлення weights
   * Повертає cleanup-функцію (скасування підписок).
   * Викликати один раз при монтуванні сторінки.
   */
  initListeners: () => {
    const unlisteners: Promise<() => void>[] = [
      devicesEvents.onDeviceStatusChanged((status) => {
        set((state) => ({
          statuses: { ...state.statuses, [status.id]: status },
        }));
      }),
      devicesEvents.onWeightUpdated((payload) => {
        set((state) => ({
          weights: { ...state.weights, [payload.deviceId]: payload.value },
        }));
      }),
    ];

    // Cleanup: дочекатися розв'язання промісів listen і викликати unlisten
    return () => {
      unlisteners.forEach((p) => {
        p.then((unlisten) => unlisten()).catch(() => {
          /* підписка вже неактивна */
        });
      });
    };
  },

  clearError: () => set({ error: null }),
}));

/** Допоміжний селектор: отримати статус пристрою за id */
export const selectDeviceStatus = (state: DevicesStore, id: string): DeviceStatus | undefined =>
  state.statuses[id];

/** Допоміжний селектор: отримати поточну вагу пристрою за id */
export const selectDeviceWeight = (state: DevicesStore, id: string): number | undefined =>
  state.weights[id];
