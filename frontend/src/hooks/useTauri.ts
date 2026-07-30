/**
 * Хук для взаємодії з Tauri Desktop API
 *
 * Надає зручний інтерфейс для:
 *   - Print-as-Image (єдиний шлях друку: html2canvas → PNG → Rust)
 *   - Офлайн-режиму (кеш товарів, збереження чеків)
 *   - Системних налаштувань
 */

import { useCallback, useState } from 'react';

/**
 * Перевіряє, чи запущено в Tauri
 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Викликає Tauri команду з TypeScript безпекою
 */
async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error('Tauri не доступний — застосунок запущено в браузері');
  }

  // Динамічний імпорт Tauri API
  const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
  return tauriInvoke<T>(cmd, args);
}

// ─────────────────────────────────────────────────────────────────────────────
// Друк
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Отримати список доступних принтерів
 */
export async function getPrinters(): Promise<string[]> {
  try {
    return await invoke<string[]>('get_printers');
  } catch {
    return [];
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Офлайн-режим
// ─────────────────────────────────────────────────────────────────────────────

export async function cacheProducts(products: unknown[]): Promise<number> {
  try {
    const result = await invoke<string>('cache_products', {
      productsJson: JSON.stringify(products),
    });
    const match = result.match(/\d+/);
    return match ? parseInt(match[0], 10) : 0;
  } catch {
    return 0;
  }
}

export async function getCachedProducts(query?: string): Promise<unknown[]> {
  try {
    const data = await invoke<string>('get_cached_products', {
      query: query ?? null,
    });
    return JSON.parse(data);
  } catch {
    return [];
  }
}

export interface OfflineReceipt {
  id: number;
  data: string;
}

export async function saveReceiptOffline(receipt: unknown): Promise<number | null> {
  try {
    const result = await invoke<string>('save_receipt_offline', {
      receiptJson: JSON.stringify(receipt),
    });
    const match = result.match(/#(\d+)/);
    return match ? parseInt(match[1], 10) : null;
  } catch {
    return null;
  }
}

export async function getUnsyncedReceipts(): Promise<OfflineReceipt[]> {
  try {
    const data = await invoke<string>('get_unsynced_receipts');
    return JSON.parse(data);
  } catch {
    return [];
  }
}

export async function markReceiptSynced(receiptId: number): Promise<boolean> {
  try {
    await invoke('mark_receipt_synced', { receiptId });
    return true;
  } catch {
    return false;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Налаштування
// ─────────────────────────────────────────────────────────────────────────────

export async function saveLocalSetting(key: string, value: string): Promise<boolean> {
  try {
    await invoke('save_setting', { key, value });
    return true;
  } catch {
    return false;
  }
}

export async function getLocalSetting(key: string): Promise<string | null> {
  try {
    return await invoke<string | null>('get_setting', { key });
  } catch {
    return null;
  }
}

export async function checkOnlineStatus(): Promise<boolean> {
  try {
    return await invoke<boolean>('is_online');
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
        const response = await fetch('/api/v1/receipts', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: receipt.data,
        });
        if (response.ok) {
          await markReceiptSynced(receipt.id);
          synced++;
        }
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
