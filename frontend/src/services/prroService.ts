import api from './api';
import {
  PrroSettings,
  PrroSettingsSaveRequest,
  PrroStatus,
  PrroShift,
  PrroShiftsResponse,
  PrroQueueItem,
  PrroQueueResponse,
  FiscalizeResult,
  PrroTestConnectionResult,
  ReceiptFiscalInfo,
} from '@/types/prro';

/**
 * API-клієнт для роботи з ПРРО (програмний РРО).
 *
 * Всі запити йдуть на `/api/v2/prro/*` — для цього використовуємо
 * per-request `baseURL: '/api/v2'` поверх спільного axios-інстансу
 * (зберігаються auth-інтерцептори: токен та refresh).
 */

const V2 = { baseURL: '/api/v2' } as const;

/** Помилка відповіді бекенду (detail може бути строкою або масивом) */
function extractErrorMessage(error: unknown): string {
  const err = error as { response?: { data?: { detail?: unknown } } };
  const detail = err?.response?.data?.detail;
  if (typeof detail === 'string') return detail;
  if (Array.isArray(detail)) return detail.map((d: any) => d.msg || String(d)).join('; ');
  if (detail && typeof detail === 'object') return JSON.stringify(detail);
  return 'Помилка запиту до ПРРО';
}

export const prroService = {
  /** Отримати налаштування ПРРО */
  async getSettings(): Promise<PrroSettings> {
    const response = await api.get<PrroSettings>('/prro/settings', V2);
    return response.data;
  },

  /**
   * Зберегти налаштування ПРРО.
   *
   * Підтримує передачу файлу ключа (base64) або лише імені файлу (якщо
   * ключ вже збережено на сервері, напр. certs/prro-test/).
   */
  async saveSettings(data: PrroSettingsSaveRequest): Promise<PrroSettings> {
    const formData = new FormData();

    if (data.prro_fn !== undefined && data.prro_fn !== null) formData.append('prro_fn', data.prro_fn);
    if (data.prro_tn !== undefined && data.prro_tn !== null) formData.append('prro_tn', data.prro_tn);
    if (data.prro_zn !== undefined && data.prro_zn !== null) formData.append('prro_zn', data.prro_zn);
    if (data.mode) formData.append('mode', data.mode);
    if (data.key_password !== undefined && data.key_password !== null) formData.append('key_password', data.key_password);
    if (data.key_file_name) formData.append('key_file_name', data.key_file_name);
    if (data.key_file_base64) {
      // Конвертуємо base64 у Blob/File для multipart-завантаження
      const byteCharacters = atob(data.key_file_base64);
      const byteNumbers = new Array(byteCharacters.length);
      for (let i = 0; i < byteCharacters.length; i++) {
        byteNumbers[i] = byteCharacters.charCodeAt(i);
      }
      const byteArray = new Uint8Array(byteNumbers);
      const blob = new Blob([byteArray]);
      formData.append('key_file', blob, data.key_file_name || 'key.dat');
    }
    if (data.auto_fiscalize !== undefined && data.auto_fiscalize !== null) {
      formData.append('auto_fiscalize', data.auto_fiscalize ? 'true' : 'false');
    }

    const response = await api.put<PrroSettings>('/prro/settings', formData, V2);
    return response.data;
  },

  /** Перевірити зв'язок з фіскальним сервером (ping) */
  async testConnection(): Promise<PrroTestConnectionResult> {
    try {
      const response = await api.post<PrroTestConnectionResult>('/prro/test-connection', null, V2);
      return response.data;
    } catch (error) {
      // Повертаємо зрозумілу помилку, не кидаючи виняток
      return { ok: false, success: false, message: extractErrorMessage(error) };
    }
  },

  /** Отримати статус ПРРО */
  async getStatus(): Promise<PrroStatus> {
    const response = await api.get<PrroStatus>('/prro/status', V2);
    return response.data;
  },

  /** Відкрити зміну ПРРО */
  async openShift(comment?: string): Promise<PrroShift> {
    const response = await api.post<PrroShift>(
      '/prro/shift/open',
      comment ? { comment } : undefined,
      V2
    );
    return response.data;
  },

  /** Закрити зміну ПРРО (Z-звіт) */
  async closeShift(comment?: string): Promise<PrroShift> {
    const response = await api.post<PrroShift>(
      '/prro/shift/close',
      comment ? { comment } : undefined,
      V2
    );
    return response.data;
  },

  /** Список змін ПРРО з пагінацією */
  async listShifts(page = 1, size = 20): Promise<PrroShiftsResponse> {
    const response = await api.get<PrroShiftsResponse>('/prro/shifts', {
      ...V2,
      params: { page, size },
    });
    return response.data;
  },

  /** Фіскалізувати чек (ручна фіскалізація) */
  async fiscalizeReceipt(receiptId: string, manual = true): Promise<FiscalizeResult> {
    const response = await api.post<FiscalizeResult>(
      `/prro/receipts/${receiptId}/fiscalize`,
      { manual },
      V2
    );
    return response.data;
  },

  /** Синхронізувати офлайн-чергу ПРРО */
  async syncQueue(limit = 100): Promise<{ synced: number; failed: number; [key: string]: unknown }> {
    const response = await api.post(
      '/prro/sync',
      null,
      { ...V2, params: { limit } }
    );
    return response.data;
  },

  /** Журнал офлайн-черги ПРРО */
  async getQueue(page = 1, size = 20, status?: string): Promise<PrroQueueResponse> {
    const response = await api.get<PrroQueueResponse>('/prro/queue', {
      ...V2,
      params: { page, size, ...(status ? { status } : {}) },
    });
    return response.data;
  },

  /**
   * Отримати фіскальні реквізити чеку (v2 receipts).
   * Використовується після створення чеку в POS, щоб показати статус
   * фіскалізації та fiscal_check_url для QR-коду.
   */
  async getReceiptFiscalInfo(receiptId: string): Promise<ReceiptFiscalInfo> {
    const response = await api.get<ReceiptFiscalInfo>(`/receipts/${receiptId}`, V2);
    return response.data;
  },
};
