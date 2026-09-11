/**
 * Сервіс створення чеку з офлайн-fallback через Tauri SQLite чергу.
 *
 * ⚠️ Це ОКРЕМА Tauri-черга для роботи каси БЕЗ інтернету —
 *    НЕ плутати з backend-офлайн чергою ПРРО (вона працює через
 *    /api/v2/prro/* і має власну логіку).
 *
 * Логіка роботи:
 *   1. Якщо НЕ Tauri (браузер) → звичайний createReceipt (v2 /sale|/return).
 *   2. Якщо `check_online() == false` → `save_receipt_offline(receiptJson)`
 *      (чек зберігається в SQLite WAL чергу, далі синхронізується автоматично
 *      при поновленні з'єднання — див. useOfflineSync).
 *   3. Якщо POST впав через МЕРЕЖУ (немає відповіді від сервера) →
 *      `save_receipt_offline(receiptJson)`.
 *   4. Серверні помилки (4xx/5xx — валідація, брак залишків тощо) —
 *      проброс нагору (показуємо користувачу toast з деталями).
 */

import { AxiosError } from 'axios';
import { isTauri, checkOnlineStatus, saveReceiptOffline } from '@/hooks/useTauri';
import { receiptService } from '@/services/receiptService';
import { useStoreStore } from '@/store/storeStore';
import type { Receipt, ReceiptCreate } from '@/types/receipt';

export interface CreateReceiptResult {
  /** Чек: реальний (з сервера) АБО локальна копія (для друку в офлайн) */
  receipt: Receipt;
  /** true — чек збережено в Tauri SQLite чергу (ще не створено на сервері) */
  savedOffline: boolean;
  /** ID у SQLite-черзі (якщо savedOffline) */
  offlineId: number | null;
}

/**
 * Створити чек через API з fallback на Tauri офлайн-чергу.
 *
 * @param data           - Дані чеку (ReceiptCreate). Зберігаються «як є» в SQLite
 *                         для пізнішої повторної відправки через receiptService.createReceipt.
 * @param localReceiptFn - Опційна фабрика локальної копії Receipt (для друку чека
 *                         клієнту в офлайн-режимі — друк працює локально).
 */
export async function createReceiptWithOfflineFallback(
  data: ReceiptCreate,
  localReceiptFn?: () => Receipt,
): Promise<CreateReceiptResult> {
  // Браузерний режим (не Tauri) — офлайн-черга недоступна, тільки POST
  if (!isTauri()) {
    const receipt = await receiptService.createReceipt(data);
    return { receipt, savedOffline: false, offlineId: null };
  }

  // 1) check_online() == false → одразу в SQLite-чергу (каса працює без інтернету)
  const online = await checkOnlineStatus();
  if (!online) {
    return saveToQueue(data, localReceiptFn);
  }

  try {
    const receipt = await receiptService.createReceipt(data);
    return { receipt, savedOffline: false, offlineId: null };
  } catch (error) {
    // 2) Мережева помилка (немає відповіді від сервера) → офлайн-черга
    const axiosError = error as AxiosError;
    if (!axiosError.response) {
      return saveToQueue(data, localReceiptFn);
    }
    // 3) Серверна помилка (4xx/5xx) — проброс, покажемо користувачу
    throw error;
  }
}

async function saveToQueue(
  data: ReceiptCreate,
  localReceiptFn?: () => Receipt,
): Promise<CreateReceiptResult> {
  // Мультиточковість (Етап 5): store_id поточної точки зберігається в черзі —
  // при синхронізації чек потрапить у правильну точку продажу.
  const storeId = useStoreStore.getState().activeStoreId;
  const offlineId = await saveReceiptOffline(data, storeId);
  if (offlineId === null) {
    throw new Error('Не вдалося зберегти чек в офлайн-черзі Tauri');
  }
  const receipt = localReceiptFn ? localReceiptFn() : buildMinimalReceipt(data, offlineId);
  return { receipt, savedOffline: true, offlineId };
}

/**
 * Мінімальний локальний чек (якщо не передано фабрику) — щоб не ламати
 * подальший флоу (print dialog тощо), коли сервер недоступний.
 */
function buildMinimalReceipt(data: ReceiptCreate, offlineId: number): Receipt {
  const now = new Date().toISOString();
  const total = parseFloat(data.total_amount) || 0;
  const paid = parseFloat(data.paid_amount || data.total_amount) || total;
  return {
    id: `offline-${offlineId}`,
    receipt_number: `OFF-${String(offlineId).padStart(6, '0')}`,
    receipt_type: data.receipt_type,
    items: [],
    total_amount: data.total_amount,
    paid_amount: data.paid_amount,
    vat_amount: '0',
    payment_method: data.payment_method || null,
    payment_status: paid >= total ? 'paid' : 'partially_paid',
    cash_amount: String(data.cash_amount ?? 0),
    card_amount: String(data.card_amount ?? 0),
    change_amount: '0',
    cashier_id: data.cashier_id || '',
    created_by: data.cashier_id || '',
    created_at: now,
    original_receipt_number: data.receipt_number,
    is_fiscal: false,
    fiscal_status: 'none',
  };
}
