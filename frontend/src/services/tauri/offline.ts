/**
 * Сервіс для роботи з офлайн-режимом через Tauri v2 Desktop API.
 *
 * Надає функції для:
 *   - Кешування товарів локально
 *   - Збереження чеків при відсутності інтернету
 *   - Синхронізації при поновленні з'єднання
 *   - Роботи з локальними налаштуваннями
 */

import { invoke } from '@tauri-apps/api/core';

// ─── Типи ───────────────────────────────────────────────────────────────────

export interface OfflineStats {
  products_cached: number;
  unsynced_receipts: number;
  db_size_bytes: number;
  db_path: string;
}

// ─── Офлайн-режим ───────────────────────────────────────────────────────────

/**
 * Перевірити чи доступний офлайн-режим.
 */
export async function isOfflineAvailable(): Promise<boolean> {
  return invoke<boolean>('is_offline_available');
}

/**
 * Перевірити доступність інтернету через Tauri.
 *
 * ⚠️ Правильна назва команди: `check_online` (не `is_online`).
 */
export async function checkOnline(): Promise<boolean> {
  return invoke<boolean>('check_online');
}

/**
 * Отримати кількість несинхронізованих чеків.
 */
export async function getUnsyncedCount(): Promise<number> {
  return invoke<number>('get_unsynced_count');
}

// ─── Синхронізація (outbox-push, ЕТАП 4/5 Rust) ─────────────────────────────────

/**
 * Статус outbox-черги (команда `sync_status`).
 *
 * Контракт (torgashka-infrastructure/src/offline/commands.rs):
 *   { pending_count, failed_count, last_error, last_sync_at }
 */
export interface SyncStatusResult {
  /** Записи в outbox зі статусом 'pending' */
  pending_count: number;
  /** Записи зі статусом 'failed' — потребують уваги */
  failed_count: number;
  /** Остання помилка push (null, якщо все чисто) */
  last_error: string | null;
  /** ISO-час останнього успішного push (MAX(pushed_at) по done), null — ще не було */
  last_sync_at: string | null;
}

/**
 * Результат ручного push (команда `sync_now`).
 *
 * Контракт (torgashka-infrastructure/src/offline/commands.rs):
 *   { pushed, already_exists, failed, remaining, last_error }
 * Команда повторює батчі, поки є pending (або поки є мережевий прогрес).
 */
export interface SyncNowResult {
  /** Відправлено успішно (created) */
  pushed: number;
  /** Вже існували на сервері (ідемпотентний повтор) */
  already_exists: number;
  /** Не вдалося відправити (5xx/429/валідація) — outbox status='failed' */
  failed: number;
  /** Залишилось у черзі (pending після спроби) */
  remaining: number;
  /** Остання помилка push (null — все чисто) */
  last_error: string | null;
}

/**
 * Отримати статус outbox-черги.
 *
 * Каса без Rust/Tauri (або стара версія без команди): invoke падає →
 * повертає null (UI показує 'unavailable').
 */
export async function syncStatus(): Promise<SyncStatusResult | null> {
  try {
    return await invoke<SyncStatusResult>('sync_status');
  } catch {
    return null;
  }
}

/**
 * Ручний тригер push: повторює батчі, поки є pending.
 *
 * Каса без Rust/Tauri: invoke падає → повертає null.
 */
export async function syncNow(): Promise<SyncNowResult | null> {
  try {
    return await invoke<SyncNowResult>('sync_now');
  } catch {
    return null;
  }
}

// ─── Товари (кеш) ───────────────────────────────────────────────────────────

/**
 * Кешувати товари локально (для поточної точки продажу).
 *
 * @param products - Масив товарів для кешування
 * @param storeId  - UUID точки продажу (мультиточковість, Етап 5)
 */
export async function cacheProducts(products: unknown[], storeId?: string | null): Promise<number> {
  return invoke<number>('cache_products', {
    productsJson: JSON.stringify(products),
    storeId: storeId ?? null,
  });
}

/**
 * Записати помилку фронтенду у /tmp/kasa-frontend.log (діагностика).
 * Використовується пасткою window.onerror / unhandledrejection у main.tsx.
 */
export async function logFrontendError(message: string): Promise<void> {
  try {
    await invoke('log_frontend_error', { message });
  } catch {
    // Лог — некритичний: якщо команда недоступна, мовчки ігноруємо
  }
}

/**
 * Отримати кешовані товари поточної точки.
 *
 * @param search  - Пошуковий запит (опціонально)
 * @param limit   - Максимальна кількість (опціонально, за замовчуванням 100)
 * @param storeId - UUID точки продажу (фільтр кешу, мультиточковість)
 */
export async function getCachedProducts(
  search?: string,
  limit?: number,
  storeId?: string | null,
): Promise<unknown[]> {
  const result = await invoke<string>('get_cached_products', {
    search: search ?? null,
    limit: limit ?? 100,
    storeId: storeId ?? null,
  });
  return JSON.parse(result);
}

/**
 * Очистити кеш товарів.
 */
export async function clearProductCache(): Promise<number> {
  return invoke<number>('clear_product_cache');
}

// ─── Чеки (офлайн) ──────────────────────────────────────────────────────────

/**
 * Зберегти чек локально (з точкою продажу).
 *
 * @param receipt - Дані чека
 * @param storeId - UUID точки продажу — зберігається в черзі, щоб при
 *                  синхронізації чек потрапив у правильну точку (Етап 5)
 * @returns ID збереженого чека
 */
export async function saveReceiptOffline(receipt: unknown, storeId?: string | null): Promise<number> {
  return invoke<number>('save_receipt_offline', {
    receiptJson: JSON.stringify(receipt),
    storeId: storeId ?? null,
  });
}

/**
 * Отримати несинхронізовані чеки (з store_id для коректної синхронізації).
 */
export interface UnsyncedReceipt {
  id: number;
  data: string;
  store_id: string | null;
}
export async function getUnsyncedReceipts(): Promise<UnsyncedReceipt[]> {
  return invoke<UnsyncedReceipt[]>('get_unsynced_receipts');
}

/**
 * Позначити чек як синхронізований.
 *
 * @param receiptId - ID чека
 */
export async function markReceiptSynced(receiptId: number): Promise<void> {
  return invoke<void>('mark_receipt_synced', { receiptId });
}

// ─── Налаштування ───────────────────────────────────────────────────────────

/**
 * Отримати локальне налаштування.
 *
 * @param key - Ключ налаштування
 */
export async function getSetting(key: string): Promise<string | null> {
  return invoke<string | null>('get_setting', { key });
}

/**
 * Зберегти локальне налаштування.
 *
 * @param key - Ключ налаштування
 * @param value - Значення
 */
export async function setSetting(key: string, value: string): Promise<void> {
  return invoke<void>('set_setting', { key, value });
}

/**
 * Зберегти облікові дані синхронізації (`server_url` + `api_token`).
 * Викликається після успішного логіна / оновлення токена (authStore).
 * Rust читає ці ключі з SQLite settings перед кожним push (sync_now).
 *
 * @returns true — збережено; false — не Tauri/помилка invoke
 */
export async function persistSyncCredentials(apiToken: string, serverUrl: string): Promise<boolean> {
  try {
    await invoke<void>('set_setting', { key: 'api_token', value: apiToken });
    await invoke<void>('set_setting', { key: 'server_url', value: serverUrl });
    return true;
  } catch {
    return false;
  }
}

/**
 * Зберегти активну торгову точку (`store_id`).
 * Викликається при зміні точки / автовиборі після логіна (storeStore).
 * Rust використовує store_id як контекст для POST /sync/push.
 *
 * @returns true — збережено; false — не Tauri/помилка invoke
 */
export async function persistSyncStore(storeId: string): Promise<boolean> {
  try {
    await invoke<void>('set_setting', { key: 'store_id', value: storeId });
    return true;
  } catch {
    return false;
  }
}


// ─── Статистика ─────────────────────────────────────────────────────────────

/**
 * Отримати статистику офлайн-бази (кількість товарів — поточної точки).
 */
export async function getOfflineStats(storeId?: string | null): Promise<OfflineStats> {
  return invoke<OfflineStats>('get_offline_stats', { storeId: storeId ?? null });
}
