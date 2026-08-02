/**
 * Хук для взаємодії з Tauri Desktop API
 *
 * ⚠️ Це ТОНКА ОБГОРТКА над правильним шаром Tauri-команд:
 *   - src/services/tauri/print.ts   (друк: print_image, print_html, get_printers …)
 *   - src/services/tauri/offline.ts (офлайн: cache_products, save_receipt_offline,
 *                                    get_cached_products(search), set_setting,
 *                                    check_online …)
 *
 * Надає зручний інтерфейс для:
 *   - Print-as-Image (єдиний шлях друку: html2canvas → PNG → Rust)
 *   - Офлайн-режиму (кеш товарів, збереження чеків)
 *   - Системних налаштувань
 */

import { useCallback, useState } from 'react';

// ─── Правильний шар Tauri-команд (snake_case структури) ────────────────────
import * as tauriOffline from '@/services/tauri/offline';
import * as tauriPrint from '@/services/tauri/print';
import { receiptService } from '@/services/receiptService';

/**
 * Перевіряє, чи запущено в Tauri
 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// ─────────────────────────────────────────────────────────────────────────────
// Друк
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Отримати список доступних принтерів.
 *
 * ✅ Делегує в services/tauri/print.ts → invoke('get_printers')
 */
export async function getPrinters(): Promise<string[]> {
  return tauriPrint.getPrinters();
}

// ─────────────────────────────────────────────────────────────────────────────
// Офлайн-режим
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Кешувати товари локально.
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('cache_products', { productsJson })
 */
export async function cacheProducts(products: unknown[]): Promise<number> {
  try {
    return await tauriOffline.cacheProducts(products);
  } catch {
    return 0;
  }
}

/**
 * Отримати кешовані товари.
 *
 * ⚠️ ВИПРАВЛЕНО: раніше передавав `query` — правильний параметр команди
 *    get_cached_products — `search` (і `limit`).
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('get_cached_products', { search, limit })
 */
export async function getCachedProducts(query?: string, limit?: number): Promise<unknown[]> {
  try {
    return await tauriOffline.getCachedProducts(query, limit);
  } catch {
    return [];
  }
}

export interface OfflineReceipt {
  id: number;
  data: string;
}

/**
 * Зберегти чек локально.
 *
 * ⚠️ ВИПРАВЛЕНО: Rust повертає `i64` (id чеку) НАПРЯМУ — не потрібно
 *    парсити `#(\d+)` з рядка. Раніше повертало `null` завжди.
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('save_receipt_offline', { receiptJson })
 */
export async function saveReceiptOffline(receipt: unknown): Promise<number | null> {
  try {
    return await tauriOffline.saveReceiptOffline(receipt);
  } catch {
    return null;
  }
}

/**
 * Отримати несинхронізовані чеки.
 *
 * ⚠️ ВИПРАВЛЕНО: раніше `invoke<string>` — команда повертає МАСИВ
 *    `[{ id: number, data: string }]`, тому JSON.parse перетворював
 *    масив у рядок і чеки ніколи не знаходились (завжди []).
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('get_unsynced_receipts')
 */
export async function getUnsyncedReceipts(): Promise<OfflineReceipt[]> {
  try {
    return await tauriOffline.getUnsyncedReceipts();
  } catch {
    return [];
  }
}

/**
 * Отримати кількість несинхронізованих чеків (для індикатора SyncStatus).
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('get_unsynced_count')
 */
export async function getUnsyncedCount(): Promise<number> {
  try {
    return await tauriOffline.getUnsyncedCount();
  } catch {
    return 0;
  }
}

/**
 * Позначити чек як синхронізований.
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('mark_receipt_synced', { receiptId })
 */
export async function markReceiptSynced(receiptId: number): Promise<boolean> {
  try {
    await tauriOffline.markReceiptSynced(receiptId);
    return true;
  } catch {
    return false;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Налаштування
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Зберегти локальне налаштування.
 *
 * ⚠️ ВИПРАВЛЕНО: команда називається `set_setting` (не `save_setting`).
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('set_setting', { key, value })
 */
export async function saveLocalSetting(key: string, value: string): Promise<boolean> {
  try {
    await tauriOffline.setSetting(key, value);
    return true;
  } catch {
    return false;
  }
}

/**
 * Отримати локальне налаштування.
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('get_setting', { key })
 */
export async function getLocalSetting(key: string): Promise<string | null> {
  try {
    return await tauriOffline.getSetting(key);
  } catch {
    return null;
  }
}

/**
 * Перевірити доступність інтернету.
 *
 * ⚠️ ВИПРАВЛЕНО: команда називається `check_online` (не `is_online`).
 *    Раніше `invoke('is_online')` завжди кидав помилку і статус падав
 *    на `navigator.onLine`.
 *
 * ✅ Делегує в services/tauri/offline.ts → invoke('check_online')
 */
export async function checkOnlineStatus(): Promise<boolean> {
  try {
    return await tauriOffline.checkOnline();
  } catch {
    return navigator.onLine;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// React Hook
// ─────────────────────────────────────────────────────────────────────────────

interface TauriState {
  isTauri: boolean;
  isOnline: boolean;
  printers: string[];
  printing: boolean;
  loading: boolean;
}

interface TauriActions {
  refreshPrinters: () => Promise<void>;
  refreshOnlineStatus: () => Promise<void>;
  saveOffline: (receipt: unknown) => Promise<number | null>;
  syncReceipts: () => Promise<number>;
}

export function useTauri(): TauriState & TauriActions {
  const [state, setState] = useState<TauriState>({
    isTauri: isTauri(),
    isOnline: navigator.onLine,
    printers: [],
    printing: false,
    loading: false,
  });

  const refreshPrinters = useCallback(async () => {
    if (!state.isTauri) return;
    const printers = await getPrinters();
    setState((prev) => ({ ...prev, printers }));
  }, [state.isTauri]);

  const refreshOnlineStatus = useCallback(async () => {
    const isOnline = await checkOnlineStatus();
    setState((prev) => ({ ...prev, isOnline }));
  }, []);

  const saveOffline = useCallback(
    async (receipt: unknown): Promise<number | null> => {
      if (!state.isTauri) return null;
      return saveReceiptOffline(receipt);
    },
    [state.isTauri],
  );

  const syncReceipts = useCallback(async (): Promise<number> => {
    if (!state.isTauri) return 0;
    const receipts = await getUnsyncedReceipts();
    let synced = 0;
    for (const receipt of receipts) {
      try {
        // ⚠️ Відправляємо через receiptService.createReceipt, а не напряму fetch:
        //   1) axios-інтерцептор додає Bearer-токен (раніше fetch ходив без auth)
        //   2) маршрутизація v1/v2: звичайні чеки → v2 POST /receipts/sale|return,
        //      боргові / з original_receipt_id → v1 POST /receipts.
        const data = JSON.parse(receipt.data) as Parameters<typeof receiptService.createReceipt>[0];
        await receiptService.createReceipt(data);
        await markReceiptSynced(receipt.id);
        synced++;
      } catch {
        continue;
      }
    }
    return synced;
  }, [state.isTauri]);

  return {
    ...state,
    refreshPrinters,
    refreshOnlineStatus,
    saveOffline,
    syncReceipts,
  };
}
