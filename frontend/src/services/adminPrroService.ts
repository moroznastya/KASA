import api from './api';
import {
  PrroSettingsUpdateInput,
  StorePrroSettings,
} from '@/types/adminPrro';

/**
 * «Один магазин — один ПРРО» (адмін-панель) — per-store налаштування.
 *
 * Серверні endpoint (Rust, router_v1.rs — /api/v1/admin/stores/:id/prro-settings
 * поза store_middleware; RBAC owner|admin):
 *   GET /admin/stores/:store_id/prro-settings — конфіг ТОЧКИ (ключ замасковано);
 *   PUT /admin/stores/:store_id/prro-settings — збереження конфігу точки +
 *       опційне завантаження ключа КЕП (multipart, key_file/поля).
 *
 * Модель per-store (закриття аномалії Етапа 5): prro_settings має
 * (store_id, key_name); PUT точки Б не затирає конфіг точки А. Ключ/пароль
 * КЕП ніколи не повертаються в plaintext.
 */

export const adminPrroService = {
  /** Конфіг/стан ПРРО конкретної точки. */
  async getStorePrro(storeId: string): Promise<StorePrroSettings> {
    const response = await api.get<StorePrroSettings>(
      `/admin/stores/${storeId}/prro-settings`,
    );
    return response.data;
  },

  /**
   * Зберігає налаштування ПРРО точки + опційно завантажує ключ КЕП.
   * Повертає оновлений стан (як GET).
   */
  async updateStorePrro(
    storeId: string,
    input: PrroSettingsUpdateInput,
  ): Promise<StorePrroSettings> {
    const form = new FormData();
    if (input.prro_fn !== undefined) form.append('prro_fn', input.prro_fn);
    if (input.prro_tn !== undefined) form.append('prro_tn', input.prro_tn);
    if (input.prro_zn !== undefined) form.append('prro_zn', input.prro_zn);
    if (input.mode !== undefined) form.append('mode', input.mode);
    if (input.url !== undefined) form.append('url', input.url);
    if (input.key_password) form.append('key_password', input.key_password);
    if (input.keyFile) form.append('key_file', input.keyFile);
    const response = await api.put<StorePrroSettings>(
      `/admin/stores/${storeId}/prro-settings`,
      form,
    );
    return response.data;
  },
};
