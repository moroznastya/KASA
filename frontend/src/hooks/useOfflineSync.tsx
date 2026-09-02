/**
 * Хук статусу синхронізації на базі outbox-push механізму (ЕТАП 4/5 Rust).
 *
 * Новий потік (замість старого getUnsyncedReceipts → receiptService.createReceipt):
 *   - `sync_status` → { pending_count, failed_count, last_error, last_sync_at }
 *     (читає outbox-чергу SQLite: status='pending' / status='failed')
 *   - `sync_now`    → { pushed, already_exists, failed, remaining, last_error }
 *     (ручний тригер push; Rust повторює батчі, поки є pending)
 *
 * Rust-команди недоступні (браузер або стара версія Tauri): invoke падає →
 * syncStatus()/syncNow() повертають null → available=false → SyncStatus
 * не рендериться.
 */

import { useCallback, useEffect, useState } from 'react';
import {
  syncStatus,
  syncNow,
  type SyncStatusResult,
  type SyncNowResult,
} from '@/services/tauri/offline';

/** Період опитування sync_status (мс). */
const POLL_INTERVAL_MS = 15_000;

export interface OfflineSyncState {
  /** sync_now у процесі виконання */
  syncing: boolean;
  /** Час останнього успішного push (з sync_status або після sync_now) */
  lastSyncAt: Date | null;
  /** Записи в outbox зі статусом 'pending' */
  pendingCount: number;
  /** Записи зі статусом 'failed' — потребують уваги */
  failedCount: number;
  /** Остання помилка push (для title/аномалій) */
  lastError: string | null;
  /** Команди sync_status/sync_now доступні (Tauri з Rust-бібліотекою) */
  available: boolean;
}

/**
 * Хук для роботи з офлайн-синхронізацією (outbox-push).
 *
 * - Первинна перевірка доступності + статусу одразу після монтування.
 * - Poll sync_status кожні 15 секунд.
 * - Оновлення після події 'kasa:offline-receipt-saved' (новий запис у черзі)
 *   та події 'online' (з'явилась мережа).
 */
// react-refresh: файл свідомо експортує хук + компонент (SyncStatus нижче)
// eslint-disable-next-line react-refresh/only-export-components
export function useOfflineSync() {
  const [state, setState] = useState<OfflineSyncState>({
    syncing: false,
    lastSyncAt: null,
    pendingCount: 0,
    failedCount: 0,
    lastError: null,
    available: false,
  });

  /** Оновити стан зі sync_status (read-only; нічого не пушить). */
  const refreshStatus = useCallback(async () => {
    const status: SyncStatusResult | null = await syncStatus();
    if (!status) {
      // Команда недоступна (браузер / старий Rust) — ховаємо індикатор.
      setState((prev) => (prev.available ? { ...prev, available: false } : prev));
      return;
    }
    setState((prev) => ({
      ...prev,
      available: true,
      pendingCount: status.pending_count,
      failedCount: status.failed_count,
      lastError: status.last_error,
      // last_sync_at з БД авторитетніший; ISO null (ще не було push) — не затираємо
      lastSyncAt: status.last_sync_at ? new Date(status.last_sync_at) : prev.lastSyncAt,
    }));
  }, []);

  /** Ручний тригер push: sync_now → оновити стан з результату. */
  const sync = useCallback(async () => {
    setState((prev) => ({ ...prev, syncing: true }));

    const result: SyncNowResult | null = await syncNow();
    if (!result) {
      setState((prev) => ({ ...prev, syncing: false, available: false }));
      return;
    }

    const progressed = result.pushed + result.already_exists > 0;
    const needsAttention = result.failed > 0 || result.remaining > 0;
    const lastError = result.failed > 0
      ? result.last_error ?? `${result.failed} записів не вдалося синхронізувати`
      : result.remaining > 0
        ? result.last_error ?? 'Частину записів не вдалося відправити на сервер'
        : null;

    setState((prev) => ({
      ...prev,
      syncing: false,
      available: true,
      pendingCount: result.remaining,
      failedCount: result.failed,
      lastError: needsAttention ? lastError : null,
      lastSyncAt: progressed ? new Date() : prev.lastSyncAt,
    }));
  }, []);

  // Первинна перевірка: визначає available + перший статус.
  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  // Poll 15с + події — тільки коли команди доступні (available=true).
  useEffect(() => {
    if (!state.available) return;

    const interval = setInterval(() => void refreshStatus(), POLL_INTERVAL_MS);

    const handleReceiptSaved = () => void refreshStatus();
    const handleOnline = () => void refreshStatus();

    window.addEventListener('kasa:offline-receipt-saved', handleReceiptSaved);
    window.addEventListener('online', handleOnline);

    return () => {
      clearInterval(interval);
      window.removeEventListener('kasa:offline-receipt-saved', handleReceiptSaved);
      window.removeEventListener('online', handleOnline);
    };
  }, [state.available, refreshStatus]);

  return {
    ...state,
    sync,
  };
}

/**
 * Компонент індикатора статусу синхронізації.
 *
 * 4 стани (пріоритет зверху вниз):
 *   1. syncing            → синє  «Синхронізація…»
 *   2. failedCount > 0    → червоне «⚠ Потребує уваги» (title=lastError; клік → sync)
 *   3. pendingCount > 0   → жовте «N очікують синхронізації» (клік → sync)
 *   4. інакше + lastSyncAt → зелене «Синхронізовано HH:MM:SS»
 * Команди недоступні (available=false) → не рендериться взагалі.
 */
export const SyncStatus: React.FC = () => {
  const { syncing, lastSyncAt, pendingCount, failedCount, lastError, sync, available } =
    useOfflineSync();

  if (!available) return null;

  const time = lastSyncAt?.toLocaleTimeString('uk-UA', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });

  return (
    <div className="flex items-center text-xs whitespace-nowrap">
      {syncing ? (
        <span className="text-blue-600 dark:text-blue-400">Синхронізація…</span>
      ) : failedCount > 0 ? (
        <button
          type="button"
          onClick={() => void sync()}
          title={lastError ?? 'Помилка синхронізації — натисніть, щоб повторити'}
          className="text-red-600 hover:text-red-700 dark:text-red-400 underline"
        >
          ⚠ Потребує уваги
        </button>
      ) : pendingCount > 0 ? (
        <button
          type="button"
          onClick={() => void sync()}
          title="Натисніть, щоб синхронізувати зараз"
          className="text-yellow-600 hover:text-yellow-700 dark:text-yellow-400 underline"
        >
          {pendingCount} {pendingCount === 1 ? 'очікує' : 'очікують'} синхронізації
        </button>
      ) : lastSyncAt ? (
        <span
          className="text-green-600 dark:text-green-400"
          title={lastSyncAt.toLocaleString('uk-UA')}
        >
          Синхронізовано {time}
        </span>
      ) : null}
    </div>
  );
};
