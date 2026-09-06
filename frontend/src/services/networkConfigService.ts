import api from './api';
import {
  ExportNetworkConfigBody,
  ExportNetworkConfigResult,
  ImportNetworkConfigResult,
  NetworkConfigFile,
} from '@/types/networkConfig';

/**
 * Конфіг-файл мережі (Етап 3 — «Експорт конфігурації» для нової каси).
 *
 * Rust endpoints (admin_network_config.rs — /api/v1/admin/network-config/*,
 * require_owner: admin/store_manager → 403, окремий роутер БЕЗ store_middleware):
 *   POST /admin/network-config/export → { filename, content }
 *   POST /admin/network-config/import → { ok, network_id, store, server_url }
 *
 * Викликається тільки від імені role=owner (кнопки ховаються для інших —
 * див. StoresPage). include_db_password завжди false: пароль БД у файлі для
 * каси не потрібен (каса працює через API, не через прямий доступ до БД).
 */
/**
 * Розпарсити та провалідувати конфіг-файл мережі (schema_version=1).
 *
 * Використовується «Мережевою касою» (DeviceSyncPage): файл імпортується
 * ЛОКАЛЬНО (без HTTP) — лише JSON.parse + валідація ключів; серверна
 * активація йде через існуючий POST /devices/activate (code + server_url).
 *
 * @throws Error зі зрозумілим користувачу текстом.
 */
export function parseNetworkConfigFile(content: string): NetworkConfigFile {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    throw new Error('Файл не є валідним JSON');
  }
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Файл конфігурації порожній або пошкоджений');
  }
  const cfg = parsed as Partial<NetworkConfigFile>;
  if (cfg.schema_version !== 1) {
    throw new Error(
      `Непідтримувана schema_version — очікується 1 (у файлі: ${String(cfg.schema_version)})`
    );
  }
  if (typeof cfg.server_url !== 'string' || !cfg.server_url.trim()) {
    throw new Error('У файлі відсутня адреса сервера (server_url)');
  }
  const store = cfg.store as Partial<NetworkConfigFile['store']> | undefined;
  if (!store || typeof store !== 'object' || typeof store.name !== 'string' || !store.name.trim()) {
    throw new Error('У файлі відсутні дані торгової точки (store.name)');
  }
  if (typeof store.activation_code !== 'string' || !store.activation_code.trim()) {
    throw new Error('У файлі відсутній код активації (store.activation_code)');
  }
  return cfg as NetworkConfigFile;
}

export const networkConfigService = {
  async exportNetworkConfig(storeId: string, serverUrl?: string): Promise<ExportNetworkConfigResult> {
    const body: ExportNetworkConfigBody = {
      store_id: storeId,
      include_db_password: false,
    };
    if (serverUrl && serverUrl.trim()) body.server_url = serverUrl.trim();
    const response = await api.post<ExportNetworkConfigResult>('/admin/network-config/export', body);
    return response.data;
  },

  async importNetworkConfig(content: string): Promise<ImportNetworkConfigResult> {
    const response = await api.post<ImportNetworkConfigResult>('/admin/network-config/import', {
      content,
    });
    return response.data;
  },
};

/**
 * Зберегти текст як файл у браузері (Blob + <a download>).
 *
 * У Tauri окремого fs/dialog-плагіна немає (див. package.json) — той самий
 * download-фолбек: webview відкриває стандартний діалог збереження засобами
 * системи (Tauri 2 налаштовано на downloads).
 */
export function downloadTextFile(filename: string, content: string): void {
  const blob = new Blob([content], { type: 'application/json;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  try {
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
}
