import axios, { AxiosError } from 'axios';

/**
 * Активація каси як мережевого пристрою (device-режим синхронізації, Етап 3).
 *
 * Сервер: POST {serverUrl}/api/v1/devices/activate — ПУБЛІЧНИЙ (без JWT).
 *   Body: { code: string, device_fingerprint: string }
 *   Відповідь: { device_token, device_id, store_id, store_name }
 *   Код активації — до 9 символів A-Z0-9 (сервер апперкейсить сам).
 *
 * Після успіху фронт зберігає server_url + device_token у SQLite settings
 * (persistSyncDevice) — Rust сам (пере)запускає фонові синки.
 */

/** Результат активації (контракт сервера). */
export interface DeviceActivationResult {
  device_token: string;
  device_id: string;
  store_id: string;
  store_name: string;
}

// ── Ключі localStorage: fingerprint інсталяції + локальний стан UI ──────────
const FINGERPRINT_KEY = 'torgashka_device_fingerprint';
const ACTIVE_FLAG_KEY = 'torgashka_device_active';
const STORE_NAME_KEY = 'torgashka_device_store_name';
const STORE_ID_KEY = 'torgashka_device_store_id';

/**
 * Нормалізувати server_url для Rust/сервера:
 *   trim → без трейлінг-слешів → без суфікса `/api/v1`.
 * Rust (read_sync_auth/sync_push) САМ додає `/api/v1/...` до base_url —
 * у settings має лежати корінь (http://host:port), інакше буде подвійний шлях.
 */
export function normalizeServerUrl(raw: string): string {
  let url = raw.trim();
  url = url.replace(/\/+$/, '');
  url = url.replace(/\/api\/v1$/i, '');
  return url;
}

/**
 * Стабільний fingerprint інсталяції: генерується ОДИН раз і перевикористо-
 * вується (сервер ідентифікує фізичну касу). crypto.randomUUID з fallback.
 */
export function getDeviceFingerprint(): string {
  try {
    let fp = localStorage.getItem(FINGERPRINT_KEY);
    if (!fp) {
      const uuid =
        typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
          ? crypto.randomUUID()
          : `fp-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
      fp = `torg-${uuid}`;
      localStorage.setItem(FINGERPRINT_KEY, fp);
    }
    return fp;
  } catch {
    // localStorage недоступний (приватний режим) — разовий ідентифікатор.
    return `torg-fp-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
  }
}

/**
 * Викликати серверну активацію: POST {serverUrl}/api/v1/devices/activate.
 * Публічний ендпоінт — окремий axios-запит БЕЗ JWT/X-Store-Id interceptor-ів.
 *
 * @throws Error зі зрозумілим текстом (текст сервера для 4xx, мережа — своя).
 */
export async function activateDevice(code: string, serverUrl: string): Promise<DeviceActivationResult> {
  const base = normalizeServerUrl(serverUrl);
  if (!base) {
    throw new Error('Вкажіть адресу сервера');
  }
  const url = `${base}/api/v1/devices/activate`;
  try {
    const { data } = await axios.post<DeviceActivationResult>(
      url,
      { code: code.trim(), device_fingerprint: getDeviceFingerprint() },
      { timeout: 15000 }
    );
    return data;
  } catch (err) {
    const axiosErr = err as AxiosError<unknown>;
    if (axiosErr.response) {
      // HTTP 4xx/5xx — текст помилки з сервера (успішний формат не гарантований:
      // {detail} / {error} / {message} / plain text).
      const body = axiosErr.response.data;
      let serverMsg: string | null = null;
      if (body && typeof body === 'object') {
        const rec = body as Record<string, unknown>;
        serverMsg =
          (typeof rec.detail === 'string' && rec.detail) ||
          (typeof rec.error === 'string' && rec.error) ||
          (typeof rec.message === 'string' && rec.message) ||
          null;
      } else if (typeof body === 'string' && body.trim()) {
        serverMsg = body.trim();
      }
      if (serverMsg) {
        throw new Error(serverMsg);
      }
      throw new Error(`Сервер відповів з помилкою (HTTP ${axiosErr.response.status})`);
    }
    if (axiosErr.code === 'ECONNABORTED') {
      throw new Error('Сервер не відповів за 15 секунд. Перевірте адресу та мережу.');
    }
    if (axiosErr.code === 'ERR_NETWORK') {
      throw new Error('Сервер недоступний. Перевірте адресу, мережу та те, що сервер запущено.');
    }
    throw new Error(axiosErr.message || 'Мережева помилка при зверненні до сервера');
  }
}

// ── Локальний стан (метадані для UI) ────────────────────────────────────────
// Первинне джерело активності — SQLite settings (getSetting('device_token'));
// localStorage зберігає лише store_name/id для бейджа «Активовано» і слугує
// fallback у браузері (без Tauri invoke недоступний).

/** Запам'ятати успішну активацію (метадані для бейджа). */
export function rememberDeviceActivation(result: DeviceActivationResult, serverUrl: string): void {
  try {
    localStorage.setItem(ACTIVE_FLAG_KEY, '1');
    localStorage.setItem(STORE_NAME_KEY, result.store_name);
    localStorage.setItem(STORE_ID_KEY, result.store_id);
    if (serverUrl) localStorage.setItem('torgashka_device_server_url', normalizeServerUrl(serverUrl));
  } catch {
    /* ignore */
  }
}

/** Скинути локальний стан активації. */
export function forgetDeviceActivation(): void {
  try {
    localStorage.removeItem(ACTIVE_FLAG_KEY);
    localStorage.removeItem(STORE_NAME_KEY);
    localStorage.removeItem(STORE_ID_KEY);
  } catch {
    /* ignore */
  }
}

/** Метадані активного device-режиму з localStorage (можуть бути застарілими). */
export function readLocalDeviceState(): { active: boolean; storeName: string | null; storeId: string | null } {
  try {
    return {
      active: localStorage.getItem(ACTIVE_FLAG_KEY) === '1',
      storeName: localStorage.getItem(STORE_NAME_KEY),
      storeId: localStorage.getItem(STORE_ID_KEY),
    };
  } catch {
    return { active: false, storeName: null, storeId: null };
  }
}
