import { create } from 'zustand';
import { prroService } from '@/services/prroService';
import {
  PrroSettings,
  PrroSettingsSaveRequest,
  PrroStatus,
  PrroShift,
  PrroQueueItem,
  FiscalizeResult,
} from '@/types/prro';
import toast from 'react-hot-toast';

/** Інтервал авто-оновлення статусу ПРРО (мс) */
const STATUS_POLL_INTERVAL = 30_000;

interface PrroStore {
  settings: PrroSettings | null;
  status: PrroStatus | null;
  shifts: PrroShift[];
  queue: PrroQueueItem[];
  loading: boolean;
  error: string | null;

  // Специфічні стани дій
  savingSettings: boolean;
  testingConnection: boolean;
  openingShift: boolean;
  closingShift: boolean;
  syncing: boolean;
  fiscalizing: boolean;

  loadSettings: () => Promise<void>;
  loadStatus: () => Promise<void>;
  loadShifts: () => Promise<void>;
  loadQueue: () => Promise<void>;
  loadAll: () => Promise<void>;
  openShift: () => Promise<boolean>;
  closeShift: () => Promise<boolean>;
  testConnection: () => Promise<{ ok: boolean; message: string }>;
  saveSettings: (data: PrroSettingsSaveRequest) => Promise<boolean>;
  fiscalize: (receiptId: string) => Promise<FiscalizeResult | null>;
  sync: () => Promise<boolean>;
  clearError: () => void;
}

export const usePrroStore = create<PrroStore>((set, get) => ({
  settings: null,
  status: null,
  shifts: [],
  queue: [],
  loading: false,
  error: null,
  savingSettings: false,
  testingConnection: false,
  openingShift: false,
  closingShift: false,
  syncing: false,
  fiscalizing: false,

  loadSettings: async () => {
    set({ loading: true, error: null });
    try {
      const settings = await prroService.getSettings();
      set({ settings });
    } catch (error: any) {
      set({ error: error?.message || 'Помилка завантаження налаштувань ПРРО' });
    } finally {
      set({ loading: false });
    }
  },

  loadStatus: async () => {
    try {
      const status = await prroService.getStatus();
      set({ status });
    } catch {
      // Мовчки ігноруємо — статус оновлюється за інтервалом
    }
  },

  loadShifts: async () => {
    try {
      const response = await prroService.listShifts(1, 20);
      set({ shifts: response.items });
    } catch (error: any) {
      set({ error: error?.message || 'Помилка завантаження змін ПРРО' });
    }
  },

  loadQueue: async () => {
    try {
      const response = await prroService.getQueue(1, 20);
      set({ queue: response.items });
    } catch (error: any) {
      set({ error: error?.message || 'Помилка завантаження черги ПРРО' });
    }
  },

  loadAll: async () => {
    set({ loading: true, error: null });
    try {
      const [settings, status, shiftsRes, queueRes] = await Promise.allSettled([
        prroService.getSettings(),
        prroService.getStatus(),
        prroService.listShifts(1, 20),
        prroService.getQueue(1, 20),
      ]);
      set({
        settings: settings.status === 'fulfilled' ? settings.value : null,
        status: status.status === 'fulfilled' ? status.value : null,
        shifts: shiftsRes.status === 'fulfilled' ? shiftsRes.value.items : [],
        queue: queueRes.status === 'fulfilled' ? queueRes.value.items : [],
      });
    } catch {
      // Помилки обробляються індивідуально
    } finally {
      set({ loading: false });
    }
  },

  openShift: async () => {
    set({ openingShift: true, error: null });
    try {
      const shift = await prroService.openShift();
      toast.success(`Зміну №${shift.shift_number} відкрито`);
      await get().loadStatus();
      await get().loadShifts();
      return true;
    } catch (error: any) {
      const msg = error?.response?.data?.detail || error?.message || 'Помилка відкриття зміни';
      set({ error: msg });
      toast.error(msg);
      return false;
    } finally {
      set({ openingShift: false });
    }
  },

  closeShift: async () => {
    set({ closingShift: true, error: null });
    try {
      const shift = await prroService.closeShift();
      toast.success(`Зміну №${shift.shift_number} закрито (Z-звіт)`);
      await get().loadStatus();
      await get().loadShifts();
      return true;
    } catch (error: any) {
      const msg = error?.response?.data?.detail || error?.message || 'Помилка закриття зміни';
      set({ error: msg });
      toast.error(msg);
      return false;
    } finally {
      set({ closingShift: false });
    }
  },

  testConnection: async () => {
    set({ testingConnection: true, error: null });
    try {
      const result = await prroService.testConnection();
      const ok = result.ok === true || result.success === true;
      const message = result.message || result.detail || (ok ? 'З’єднання успішне' : 'З’єднання не вдалося');
      return { ok, message };
    } catch (error: any) {
      const message = error?.message || 'Помилка перевірки з’єднання';
      return { ok: false, message };
    } finally {
      set({ testingConnection: false });
    }
  },

  saveSettings: async (data: PrroSettingsSaveRequest) => {
    set({ savingSettings: true, error: null });
    try {
      const settings = await prroService.saveSettings(data);
      set({ settings });
      toast.success('Налаштування ПРРО збережено');
      return true;
    } catch (error: any) {
      const msg = error?.response?.data?.detail || error?.message || 'Помилка збереження налаштувань';
      set({ error: msg });
      toast.error(msg);
      return false;
    } finally {
      set({ savingSettings: false });
    }
  },

  fiscalize: async (receiptId: string) => {
    set({ fiscalizing: true, error: null });
    try {
      const result = await prroService.fiscalizeReceipt(receiptId);
      if (result.fiscal_status === 'sent') {
        toast.success(`Чек фіскалізовано №${result.fiscal_number || ''}`);
      } else if (result.fiscal_status === 'failed') {
        toast.error(`Помилка фіскалізації: ${result.error || 'невідома'}`);
      } else {
        toast.success('Чек додано до черги фіскалізації');
      }
      return result;
    } catch (error: any) {
      const msg = error?.response?.data?.detail || error?.message || 'Помилка фіскалізації';
      set({ error: msg });
      toast.error(msg);
      return null;
    } finally {
      set({ fiscalizing: false });
    }
  },

  sync: async () => {
    set({ syncing: true, error: null });
    try {
      const result = await prroService.syncQueue();
      const synced = Number(result.synced || 0);
      toast.success(`Синхронізовано: ${synced} документів`);
      await get().loadQueue();
      await get().loadStatus();
      return true;
    } catch (error: any) {
      const msg = error?.response?.data?.detail || error?.message || 'Помилка синхронізації';
      set({ error: msg });
      toast.error(msg);
      return false;
    } finally {
      set({ syncing: false });
    }
  },

  clearError: () => set({ error: null }),
}));

/**
 * Автоматичне оновлення статусу ПРРО (кожні 30 секунд).
 * Викликати один раз при монтуванні застосунку (або в PrroPage).
 */
export function startPrroStatusPolling(): () => void {
  // Одразу оновлюємо
  usePrroStore.getState().loadStatus();

  const timer = setInterval(() => {
    usePrroStore.getState().loadStatus();
  }, STATUS_POLL_INTERVAL);

  return () => clearInterval(timer);
}
