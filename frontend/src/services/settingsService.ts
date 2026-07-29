import api from './api';

interface SystemSetting {
  id: string;
  module: string;
  key: string;
  value: string | null;
  value_type: string;
  label: string;
  description: string | null;
  options: string | null;
  is_active: boolean;
}

interface SettingsModuleResponse {
  modules: Record<string, SystemSetting[]>;
}

export const settingsService = {
  /** Отримати всі налаштування */
  async getAll(): Promise<SettingsModuleResponse> {
    const response = await api.get<SettingsModuleResponse>('/settings');
    return response.data;
  },

  /** Отримати налаштування модуля (general, pos, ...) */
  async getByModule(module: string): Promise<SystemSetting[]> {
    const response = await api.get<SystemSetting[]>(`/settings/${module}`);
    return response.data;
  },

  /** Отримати одне налаштування за ключем */
  async getValue(key: string): Promise<string | null> {
    try {
      const all = await this.getAll();
      for (const moduleSettings of Object.values(all.modules)) {
        const found = moduleSettings.find(s => s.key === key);
        if (found) return found.value;
      }
      return null;
    } catch {
      return null;
    }
  },

  /** Оновити одне налаштування */
  async update(key: string, value: string): Promise<SystemSetting> {
    const response = await api.put<SystemSetting>(`/settings/${key}`, { value });
    return response.data;
  },

  /** Масове оновлення налаштувань */
  async batchUpdate(settings: Record<string, string>): Promise<SettingsModuleResponse> {
    const response = await api.put<SettingsModuleResponse>('/settings', { settings });
    return response.data;
  },
};
