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
 * Отримати кількість несинхронізованих чеків.
 */
export async function getUnsyncedCount(): Promise<number> {
  return invoke<number>('get_unsynced_count');
}

// ─── Товари (кеш) ───────────────────────────────────────────────────────────

/**
 * Кешувати товари локально.
 *
 * @param products - Масив товарів для кешування
 */
export async function cacheProducts(products: unknown[]): Promise<number> {
  return invoke<number>('cache_products', {
    productsJson: JSON.stringify(products),
  });
}

/**
 * Отримати кешовані товари.
 *
 * @param search - Пошуковий запит (опціонально)
 * @param limit - Максимальна кількість (опціонально, за замовчуванням 100)
 */
export async function getCachedProducts(search?: string, limit?: number): Promise<unknown[]> {
  const result = await invoke<string>('get_cached_products', {
    search: search ?? null,
    limit: limit ?? 100,
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
 * Зберегти чек локально.
 *
 * @param receipt - Дані чека
 * @returns ID збереженого чека
 */
export async function saveReceiptOffline(receipt: unknown): Promise<number> {
  return invoke<number>('save_receipt_offline', {
    receiptJson: JSON.stringify(receipt),
  });
}

/**
 * Отримати несинхронізовані чеки.
 */
export async function getUnsyncedReceipts(): Promise<Array<{ id: number; data: string }>> {
  return invoke<Array<{ id: number; data: string }>>('get_unsynced_receipts');
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

// ─── Статистика ─────────────────────────────────────────────────────────────

/**
 * Отримати статистику офлайн-бази.
 */
export async function getOfflineStats(): Promise<OfflineStats> {
  return invoke<OfflineStats>('get_offline_stats');
}
