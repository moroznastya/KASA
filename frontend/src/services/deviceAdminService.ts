import api from './api';

/**
 * Панель власника/адміна: керування КАСАМИ МЕРЕЖІ (device-пристрої).
 *
 * Серверні endpoints (Rust, router_v1.rs — окремий роутер /api/v1/admin/*
 * БЕЗ store_middleware, тільки auth_middleware + require_admin у хендлерах):
 *   GET    /api/v1/admin/devices[?store_id=<uuid>]          → DeviceDto[]
 *   POST   /api/v1/admin/stores/:store_id/activation-code   → {code}
 *   POST   /api/v1/admin/devices/:device_id/block           → JSON (status)
 *   POST   /api/v1/admin/devices/:device_id/unblock         → JSON (status)
 *   DELETE /api/v1/admin/devices/:device_id                 → JSON (архівація,
 *                                                             status='deleted')
 *
 * X-Store-Id не потрібен (admin-роутер поза store_middleware); JWT
 * додається інтерцептором api.ts автоматично.
 */

export type DeviceStatus = 'active' | 'blocked' | 'deleted';

/** DeviceDto (контракт сервера, network.rs). */
export interface DeviceInfo {
  id: string;
  store_id: string;
  name: string;
  status: DeviceStatus;
  app_version: string | null;
  last_seen_at: string | null;
  activated_at: string | null;
  created_at: string;
  store_name: string;
}

export interface ActivationCodeResult {
  code: string;
}

/**
 * Дістати текст помилки з Axios-відповіді ({detail: string}) —
 * як в сусідніх сторінках (UsersPage). Повертає fallback, якщо detail немає.
 */
export function extractApiError(err: unknown, fallback: string): string {
  const axiosErr = err as { response?: { data?: { detail?: unknown } } };
  const detail = axiosErr.response?.data?.detail;
  if (typeof detail === 'string' && detail.trim()) return detail;
  return fallback;
}

export const deviceAdminService = {
  /** Список кас мережі; storeId опційний → ?store_id= (фільтр по точці). */
  async listDevices(storeId?: string): Promise<DeviceInfo[]> {
    const response = await api.get<DeviceInfo[]>('/admin/devices', {
      params: storeId ? { store_id: storeId } : undefined,
    });
    return response.data;
  },

  /** Згенерувати (або ПЕРЕгенерувати) код активації для точки. */
  async generateActivationCode(storeId: string): Promise<ActivationCodeResult> {
    const response = await api.post<ActivationCodeResult>(
      `/admin/stores/${storeId}/activation-code`
    );
    return response.data;
  },

  /** Заблокувати касу: синк миттєво відхиляється сервером. */
  async blockDevice(deviceId: string): Promise<void> {
    await api.post(`/admin/devices/${deviceId}/block`);
  },

  /** Розблокувати касу (status='active'). */
  async unblockDevice(deviceId: string): Promise<void> {
    await api.post(`/admin/devices/${deviceId}/unblock`);
  },

  /** Архівувати касу (status='deleted'; фізичного видалення немає). */
  async archiveDevice(deviceId: string): Promise<void> {
    await api.delete(`/admin/devices/${deviceId}`);
  },
};
