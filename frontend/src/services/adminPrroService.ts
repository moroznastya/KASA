import api from './api';
import { StorePrroSettings } from '@/types/adminPrro';

/**
 * «ПРРО централізовано» (Етап 5 адмін-панелі, ТЗ 5.7) — READ-ONLY.
 *
 * Серверний endpoint (Rust, router_v1.rs — /api/v1/admin/stores/:id/prro-settings
 * поза store_middleware; require_admin(owner|store_manager|admin)):
 *   GET /admin/stores/:store_id/prro-settings
 *
 * Обмеження (аномалія задокументована в admin_prro.rs): модель зберігає ОДИН
 * глобальний ПРРО-реєстр на сервер (prro_settings/prro_shifts БЕЗ store_id),
 * КЕП — файл ключа поза БД. Централізований per-store PUT не підтримується —
 * метод навмисно відсутній (відповідь editable:false + reason).
 */

export const adminPrroService = {
  /** Read-only стан спільного ПРРО-реєстру в контексті картки точки. */
  async getStorePrro(storeId: string): Promise<StorePrroSettings> {
    const response = await api.get<StorePrroSettings>(
      `/admin/stores/${storeId}/prro-settings`,
    );
    return response.data;
  },
};
