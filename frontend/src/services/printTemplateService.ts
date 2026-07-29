import api from './api';
import type { PrintTemplate, PrintTemplateFormData } from '@/types/printTemplate';

/**
 * API-клієнт для роботи з шаблонами друку чеків (PrintTemplate).
 *
 * Всі методи звертаються до `/api/v1/print-templates`.
 */
export const printTemplateService = {
  /** Отримати всі активні шаблони */
  getAll: async (): Promise<PrintTemplate[]> => {
    const res = await api.get<PrintTemplate[]>('/print-templates');
    return res.data;
  },

  /** Отримати всі шаблони (включно з неактивними) */
  getAllIncludingInactive: async (): Promise<PrintTemplate[]> => {
    const res = await api.get<PrintTemplate[]>('/print-templates/all');
    return res.data;
  },

  /** Отримати шаблон за ID */
  getById: async (id: string): Promise<PrintTemplate> => {
    const res = await api.get<PrintTemplate>(`/print-templates/${id}`);
    return res.data;
  },

  /** Створити новий шаблон */
  create: async (data: PrintTemplateFormData): Promise<PrintTemplate> => {
    const res = await api.post<PrintTemplate>('/print-templates', data);
    return res.data;
  },

  /** Оновити шаблон */
  update: async (id: string, data: Partial<PrintTemplateFormData>): Promise<PrintTemplate> => {
    const res = await api.put<PrintTemplate>(`/print-templates/${id}`, data);
    return res.data;
  },

  /** Видалити шаблон */
  delete: async (id: string): Promise<void> => {
    await api.delete(`/print-templates/${id}`);
  },

  /** Встановити шаблон основним */
  setDefault: async (id: string): Promise<PrintTemplate> => {
    const res = await api.post<PrintTemplate>(`/print-templates/${id}/set-default`);
    return res.data;
  },

  /** Отримати шаблон за замовчуванням за типом */
  getDefault: async (type: string): Promise<PrintTemplate | null> => {
    try {
      const res = await api.get<PrintTemplate>('/print-templates/default', {
        params: { type },
      });
      return res.data;
    } catch {
      return null;
    }
  },

  /**
   * Рендер шаблону з підстановкою змінних.
   *
   * @param id — ID шаблону
   * @param data — словник змінних { "shop_name": "Калина", "items": "<tr>...", ... }
   * @returns готовий HTML для друку
   */
  render: async (id: string, data: Record<string, string>): Promise<string> => {
    const res = await api.post<{ html: string }>(`/print-templates/${id}/render`, { data });
    return res.data.html;
  },
};
