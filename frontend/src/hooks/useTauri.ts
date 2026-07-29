/**
 * Хук для взаємодії з Tauri Desktop API
 *
 * Надає зручний інтерфейс для:
 *   - Прямого друку ESC/POS (новий шлях — без Chrome, без PNG)
 *   - Другу чеків (plain text) та документів (HTML) — старі методи
 *   - Друку HTML-чеків на термопринтер (старий метод)
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
// Типи даних для прямого друку ESC/POS
// ─────────────────────────────────────────────────────────────────────────────

export interface ReceiptItemData {
  barcode?: string | null;
  name: string;
  quantity: number;
  price: number;
  total: number;
}

export interface ReceiptPrintData {
  shop_name: string;
  shop_address: string;
  tax_id: string;
  receipt_number: string;
  date: string;
  time: string;
  cashier: string;
  items: ReceiptItemData[];
  total: number;
  payment_method: string;
  paid: number;
  change: number;
  footer?: string | null;
}

export interface PrintResult {
  success: boolean;
  message: string;
}

// ─────────────────────────────────────────────────────────────────────────────
// НОВИЙ МЕТОД: Прямий друк ESC/POS
// ─────────────────────────────────────────────────────────────────────────────
//
// Фронтенд збирає дані чеку → відправляє JSON у Rust
// Rust генерує ESC/POS байти → пише на порт принтера
//
// @param data - дані чеку (товари, суми, магазин, тощо)
// @param printerName - назва принтера для lp (опціонально)
// @param devicePath - шлях до порту принтера (опціонально)
//
export async function printReceiptEscpos(
  data: ReceiptPrintData,
  printerName?: string,
  devicePath?: string,
): Promise<PrintResult> {
  try {
    const message = await invoke<string>('print_receipt_escpos', {
      data,
      printerName: printerName ?? null,
      devicePath: devicePath ?? null,
    });
    return { success: true, message };
  } catch (error) {
    return {
      success: false,
      message: error instanceof Error ? error.message : 'Невідома помилка друку ESC/POS',
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// СТАРІ МЕТОДИ (для сумісності)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Друк HTML-документа (цінники, етикетки, A4, чеки)
 */
export async function printDocument(
  html: string,
  printerName?: string,
): Promise<PrintResult> {
  try {
    const message = await invoke<string>('print_document', {
      html,
      printerName: printerName ?? null,
    });
    return { success: true, message };
  } catch (error) {
    return {
      success: false,
      message: error instanceof Error ? error.message : 'Невідома помилка друку',
    };
  }
}

/**
 * Друк чека простим текстом (для термопринтера)
 */
export async function printReceiptText(
  text: string,
  printerName?: string,
): Promise<PrintResult> {
  try {
    const message = await invoke<string>('print_receipt', {
      text,
      printerName: printerName ?? null,
    });
    return { success: true, message };
  } catch (error) {
    return {
      success: false,
      message: error instanceof Error ? error.message : 'Невідома помилка друку чека',
    };
  }
}

/**
 * Друк HTML-чека на термопринтер (старий метод: Chrome → PNG → ESC/POS)
 */
export async function printReceiptHtml(
  html: string,
  printerName?: string,
): Promise<PrintResult> {
  try {
    const message = await invoke<string>('print_receipt_html', {
      html,
      printerName: printerName ?? null,
    });
    return { success: true, message };
  } catch (error) {
    return {
      success: false,
      message: error instanceof Error ? error.message : 'Невідома помилка друку чека',
    };
  }
}

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

/**
 * Попередній перегляд перед друком
 */
export async function printPreview(html: string): Promise<PrintResult> {
  try {
    const message = await invoke<string>('print_preview', { html });
    return { success: true, message };
  } catch (error) {
    return {
      success: false,
      message: error instanceof Error ? error.message : 'Невідома помилка',
    };
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
  // Новий метод
  printReceiptEscpos: (data: ReceiptPrintData, printer?: string, devicePath?: string) => Promise<PrintResult>;
  // Старі методи
  print: (html: string, printer?: string) => Promise<PrintResult>;
  printReceiptText: (text: string, printer?: string) => Promise<PrintResult>;
  printReceiptHtml: (html: string, printer?: string) => Promise<PrintResult>;
  printPreview: (html: string) => Promise<PrintResult>;
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

  const print = useCallback(
    async (html: string, printer?: string): Promise<PrintResult> => {
      if (!state.isTauri) return { success: false, message: 'Tauri не доступний' };
      setState((prev) => ({ ...prev, printing: true }));
      try { return await printDocument(html, printer); }
      finally { setState((prev) => ({ ...prev, printing: false })); }
    },
    [state.isTauri],
  );

  const printReceiptTextAction = useCallback(
    async (text: string, printer?: string): Promise<PrintResult> => {
      if (!state.isTauri) return { success: false, message: 'Tauri не доступний' };
      setState((prev) => ({ ...prev, printing: true }));
      try { return await printReceiptText(text, printer); }
      finally { setState((prev) => ({ ...prev, printing: false })); }
    },
    [state.isTauri],
  );

  const printReceiptHtmlAction = useCallback(
    async (html: string, printer?: string): Promise<PrintResult> => {
      if (!state.isTauri) return { success: false, message: 'Tauri не доступний' };
      setState((prev) => ({ ...prev, printing: true }));
      try { return await printReceiptHtml(html, printer); }
      finally { setState((prev) => ({ ...prev, printing: false })); }
    },
    [state.isTauri],
  );

  // Новий метод
  const printReceiptEscposAction = useCallback(
    async (data: ReceiptPrintData, printer?: string, devicePath?: string): Promise<PrintResult> => {
      if (!state.isTauri) return { success: false, message: 'Tauri не доступний' };
      setState((prev) => ({ ...prev, printing: true }));
      try { return await printReceiptEscpos(data, printer, devicePath); }
      finally { setState((prev) => ({ ...prev, printing: false })); }
    },
    [state.isTauri],
  );

  const printPreviewAction = useCallback(
    async (html: string): Promise<PrintResult> => {
      if (!state.isTauri) return { success: false, message: 'Tauri не доступний' };
      return printPreview(html);
    },
    [state.isTauri],
  );

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
      } catch { continue; }
    }
    return synced;
  }, [state.isTauri]);

  return {
    ...state,
    printReceiptEscpos: printReceiptEscposAction,
    print,
    printReceiptText: printReceiptTextAction,
    printReceiptHtml: printReceiptHtmlAction,
    printPreview: printPreviewAction,
    refreshPrinters,
    refreshOnlineStatus,
    saveOffline,
    syncReceipts,
  };
}
