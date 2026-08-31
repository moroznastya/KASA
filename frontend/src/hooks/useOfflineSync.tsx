/**
 * Хук для автоматичної синхронізації офлайн-даних
 *
 * При поновленні інтернет-з'єднання автоматично:
 *   1. Перевіряє наявність несинхронізованих чеків (get_unsynced_count)
 *   2. Відправляє їх на сервер (get_unsynced_receipts → receiptService.createReceipt:
 *      звичайні чеки → v2 /receipts/sale|return, боргові → v1 /receipts)
 *   3. Позначає як синхронізовані (mark_receipt_synced)
 */

import { useEffect, useCallback, useState } from 'react';
import {useTauri} from './useTauri';

interface SyncState {
  syncing: boolean;
  lastSync: Date | null;
  pendingCount: number;
  syncedCount: number;
  error: string | null;
}

/**
 * Хук для роботи з офлайн-синхронізацією
 */
// react-refresh: файл свідомо експортує хук + компонент (SyncStatus нижче)
// eslint-disable-next-line react-refresh/only-export-components
export function useOfflineSync() {
  const {isTauri: inTauri, syncReceipts} = useTauri();
  const [state, setState] = useState<SyncState>({
    syncing: false,
    lastSync: null,
    pendingCount: 0,
    syncedCount: 0,
    error: null,
  });

  // ── Лічильник pending-чеків (get_unsynced_count) ────────────────────
  // Одразу при завантаженні показуємо скільки чеків очікують у SQLite-черзі.
  // Також оновлюємося, коли PosPage зберігає новий чек офлайн
  // (подія 'kasa:offline-receipt-saved').
  useEffect(() => {
    if (!inTauri) return;
    let cancelled = false;

    const refreshPendingCount = async () => {
      try {
        const { getUnsyncedCount } = await import('./useTauri');
        const count = await getUnsyncedCount();
        if (!cancelled) {
          setState((prev) => ({ ...prev, pendingCount: count }));
        }
      } catch {
        // Ігноруємо — наступне оновлення виправить
      }
    };

    refreshPendingCount();

    const handleOfflineReceiptSaved = () => refreshPendingCount();
    window.addEventListener('kasa:offline-receipt-saved', handleOfflineReceiptSaved);

    return () => {
      cancelled = true;
      window.removeEventListener('kasa:offline-receipt-saved', handleOfflineReceiptSaved);
    };
  }, [inTauri]);

  // Запустити синхронізацію вручну
  const sync = useCallback(async () => {
    if (!inTauri) return;

    setState((prev) => ({ ...prev, syncing: true, error: null }));

    try {
      const { getUnsyncedReceipts } = await import('./useTauri');
      const pending = await getUnsyncedReceipts();

      setState((prev) => ({ ...prev, pendingCount: pending.length }));

      if (pending.length === 0) {
        setState((prev) => ({
          ...prev,
          syncing: false,
          lastSync: new Date(),
          syncedCount: 0,
        }));
        return;
      }

      const synced = await syncReceipts();

      setState((prev) => ({
        ...prev,
        syncing: false,
        lastSync: new Date(),
        syncedCount: synced,
        pendingCount: pending.length - synced,
        error: synced < pending.length ? `Не вдалося синхронізувати ${pending.length - synced} чеків` : null,
      }));
    } catch (error) {
      setState((prev) => ({
        ...prev,
        syncing: false,
        error: error instanceof Error ? error.message : 'Помилка синхронізації',
      }));
    }
  }, [inTauri, syncReceipts]);

  // Автоматична синхронізація при поновленні з'єднання
  useEffect(() => {
    if (!inTauri) return;

    const handleOnline = () => {
      sync();
    };

    window.addEventListener('online', handleOnline);

    // Синхронізація при завантаженні
    const initialSync = setTimeout(() => sync(), 3000);

    return () => {
      window.removeEventListener('online', handleOnline);
      clearTimeout(initialSync);
    };
  }, [inTauri, sync]);

  return {
    ...state,
    sync,
    isTauri: inTauri,
  };
}

/**
 * Компонент індикатора статусу синхронізації
 */
export const SyncStatus: React.FC = () => {
  const { syncing, lastSync, pendingCount, error, sync, isTauri } = useOfflineSync();

  if (!isTauri) return null;

  return (
    <div className="flex items-center gap-2 text-xs">
      {syncing && (
        <span className="text-blue-600 dark:text-blue-400">
          Синхронізація...
        </span>
      )}

      {!syncing && pendingCount > 0 && (
        <button
          onClick={sync}
          className="text-yellow-600 hover:text-yellow-700 dark:text-yellow-400 underline"
          title="Натисніть, щоб синхронізувати зараз"
        >
          {pendingCount} чеків очікують синхронізації
        </button>
      )}

      {!syncing && pendingCount === 0 && lastSync && (
        <span className="text-green-600 dark:text-green-400">
          Синхронізовано: {lastSync.toLocaleTimeString('uk-UA')}
        </span>
      )}

      {error && (
        <span className="text-red-600 dark:text-red-400" title={error}>
          ⚠ Помилка
        </span>
      )}
    </div>
  );
};
