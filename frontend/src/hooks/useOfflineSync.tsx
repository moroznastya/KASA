/**
 * Хук статусу синхронізації на базі outbox-push механізму (ЕТАП 4/5 Rust).
 *
 * Новий потік (замість старого getUnsyncedReceipts → receiptService.createReceipt):
 *   - `sync_status`  → { pending_count, failed_count, last_error, last_sync_at, health? }
 *     (читає outbox-чергу SQLite: status='pending' / status='failed')
 *   - `sync_health`  → { outbox_pending, outbox_failed, stale_failed_since,
 *     last_push_ok_at, last_pull_ok_at, last_push_fail_at, last_error, degraded }
 *     (діагностика циклу; degraded = failed>0 АБО stale pending ≥ 1 год)
 *   - `sync_now`     → { pushed, already_exists, failed, remaining, last_error }
 *     (ручний тригер push; Rust повторює батчі, поки є pending)
 *
 * Rust-команди недоступні (браузер або стара версія Tauri): invoke падає →
 * syncStatus()/syncNow() повертають null → available=false → SyncStatus
 * не рендериться. `health` у sync_status — опційний (проміжна Rust-збірка);
 * тоді хук робить окремий sync_health як fallback.
 */

import { useCallback, useEffect, useState } from 'react';
import {
  syncStatus,
  syncHealth,
  syncNow,
  type SyncStatusResult,
  type SyncNowResult,
  type SyncHealthResult,
} from '@/services/tauri/offline';

/** Період опитування sync_status (мс). */
const POLL_INTERVAL_MS = 15_000;

/**
 * Поріг «цикл мертвий» (мс): якщо degraded=true, але жодної спроби push
 * (ok/fail) не було довше цього часу — цикл синку зупинено (таск упав,
 * додаток закритий), а не «активно пробує і не виходить».
 * 30 хв — заздалегідь більше за типовий backoff-інтервал живої каси
 * і менше за stale-поріг Rust (3600с), тож хибних спрацювань немає.
 */
const STALL_THRESHOLD_MS = 30 * 60_000;

/** Безпечний парс ISO-часу з Rust (null/невалідний → null). */
const toDate = (iso: string | null | undefined): Date | null => {
  if (!iso) return null;
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? null : d;
};

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
  /** degraded з sync_health; null — health недоступний (стара Rust-збірка) */
  healthDegraded: boolean | null;
  /** outbox_failed з health (точніше за sync_status.failed_count) */
  healthFailed: number;
  /** stale_failed_since: час, з якого триває проблемний стан (failed/stale) */
  staleSince: Date | null;
  /** last_error з health-контексту (остання причина degraded) */
  healthLastError: string | null;
  /** Остання успішна спроба push (для розрізнення «живий цикл» vs «мертвий») */
  lastPushOkAt: Date | null;
  /** Остання невдала спроба push (для розрізнення «живий цикл» vs «мертвий») */
  lastPushFailAt: Date | null;
}

/** Стан health без degraded-полів (для зручного скидання). */
const HEALTH_IDLE = {
  healthDegraded: null,
  healthFailed: 0,
  staleSince: null,
  healthLastError: null,
  lastPushOkAt: null,
  lastPushFailAt: null,
};

/**
 * Хук для роботи з офлайн-синхронізацією (outbox-push).
 *
 * - Первинна перевірка доступності + статусу одразу після монтування.
 * - Poll sync_status кожні 15 секунд (+ sync_health, якщо статус без health).
 * - Оновлення після події 'kasa:offline-receipt-saved' (новий запис у черзі)
 *   та події 'online' (з'явилась мережа).
 * - Після ручного sync_now — повторний запит статусу (health включно).
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
    ...HEALTH_IDLE,
  });

  /** Оновити стан зі sync_status + sync_health (read-only; нічого не пушить). */
  const refreshStatus = useCallback(async () => {
    const status: SyncStatusResult | null = await syncStatus();
    if (!status) {
      // Команда недоступна (браузер / старий Rust) — ховаємо індикатор.
      setState((prev) => (prev.available ? { ...prev, available: false } : prev));
      return;
    }

    // health: переважно з того самого sync_status (Rust commands.rs додав поле);
    // fallback — окремий sync_health (проміжна збірка).
    let health: SyncHealthResult | null = status.health ?? null;
    if (!health) health = await syncHealth();

    setState((prev) => ({
      ...prev,
      available: true,
      pendingCount: status.pending_count,
      failedCount: status.failed_count,
      lastError: status.last_error,
      // last_sync_at з БД авторитетніший; ISO null (ще не було push) — не затираємо
      lastSyncAt: status.last_sync_at ? new Date(status.last_sync_at) : prev.lastSyncAt,
      // ── degraded / стагнація (QA ЕТАП 7 §1.3) ──
      healthDegraded: health ? health.degraded : null,
      healthFailed: health?.outbox_failed ?? 0,
      staleSince: toDate(health?.stale_failed_since),
      healthLastError: health?.last_error ?? null,
      lastPushOkAt: toDate(health?.last_push_ok_at),
      lastPushFailAt: toDate(health?.last_push_fail_at),
    }));
  }, []);

  /** Ручний тригер push: sync_now → оновити стан з результату + health. */
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

    // Після ручного тригера — свіжий health (degraded міг зникнути/з'явитись).
    await refreshStatus();
  }, [refreshStatus]);

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

/** Компактне «з DD.MM, HH:MM» (локальний час) для повідомлень стагнації. */
const formatSince = (d: Date | null): string | null =>
  d
    ? d.toLocaleString('uk-UA', {
        day: '2-digit',
        month: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      })
    : null;

/**
 * Компонент індикатора статусу синхронізації.
 *
 * Пріоритет станів (зверху вниз):
 *   1. syncing → синє «Синхронізація…»
 *   2. degraded (стагнація, QA ЕТАП 7):
 *        a. цикл МЕРТВИЙ (stalled: жодної спроби push > 30 хв) → ЧЕРВОНЕ
 *           «⛔ Стагнація: N не доставлено з <час>» (title=last_error)
 *        b. цикл ЖИВИЙ, але буксує (retrying: спроби тривають, failed>0
 *           або stale pending) → ПОМАРАНЧЕВЕ «⚠ N не доставлено з <час>»
 *   3. failedCount > 0 без health (стара Rust-збірка) → червоне «⚠ Потребує уваги»
 *   4. pendingCount > 0 → жовте «N очікують синхронізації»
 *   5. інакше + lastSyncAt → зелене «Синхронізовано HH:MM:SS»
 *
 * Розрізнення «активний backoff/мережа лежить» vs «цикл мертвий»:
 *   - backoff (мережа лежить < 1 год): pending>0, degraded=false → стан 4
 *     (жовтий лічильник — спроби тривають самі);
 *   - стагнація: degraded=true; якщо остання спроба (ok/fail) свіжа →
 *     цикл живий, але не доставляє (стан 2b); якщо спроб немає > 30 хв →
 *     цикл зупинено (стан 2a).
 * Команди недоступні (available=false) → не рендериться взагалі.
 */
export const SyncStatus: React.FC = () => {
  const {
    syncing,
    lastSyncAt,
    pendingCount,
    failedCount,
    lastError,
    sync,
    available,
    healthDegraded,
    healthFailed,
    staleSince,
    healthLastError,
    lastPushOkAt,
    lastPushFailAt,
  } = useOfflineSync();

  if (!available) return null;

  const time = lastSyncAt?.toLocaleTimeString('uk-UA', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
  const sinceLabel = formatSince(staleSince);

  // ── Стагнація (degraded) ─────────────────────────────────────────────
  const degraded = healthDegraded === true;
  let stalled = false;
  if (degraded) {
    const lastAttempt = Math.max(
      lastPushFailAt?.getTime() ?? 0,
      lastPushOkAt?.getTime() ?? 0,
    );
    // Жодної спроби push (ok/fail) за STALL_THRESHOLD — цикл зупинено.
    stalled = lastAttempt === 0 || Date.now() - lastAttempt > STALL_THRESHOLD_MS;
  }
  // N у повідомленні: failed-записи (не доставлені) або stale pending.
  const degradedCount = healthFailed > 0 ? healthFailed : pendingCount;
  const degradedError = healthLastError ?? lastError;

  return (
    <div className="flex items-center text-xs whitespace-nowrap">
      {syncing ? (
        <span className="text-blue-600 dark:text-blue-400">Синхронізація…</span>
      ) : degraded && stalled ? (
        <button
          type="button"
          onClick={() => void sync()}
          title={`${degradedError ?? 'Невідома причина'}. Остання успішна синхронізація: ${
            lastSyncAt ? lastSyncAt.toLocaleString('uk-UA') : 'не було'
          }. Натисніть, щоб спробувати синхронізувати зараз.`}
          className="text-red-600 hover:text-red-700 dark:text-red-400 underline font-medium"
        >
          ⛔ Стагнація: {degradedCount}{' '}
          {healthFailed > 0
            ? `не доставлено${sinceLabel ? ` з ${sinceLabel}` : ''}`
            : `очікують${sinceLabel ? ` з ${sinceLabel}` : ''}`}
        </button>
      ) : degraded ? (
        <button
          type="button"
          onClick={() => void sync()}
          title={`${degradedError ?? 'Синхронізація буксує'}. Натисніть, щоб повторити.`}
          className="text-orange-600 hover:text-orange-700 dark:text-orange-400 underline"
        >
          ⚠ {healthFailed > 0
            ? `${healthFailed} не доставлено${sinceLabel ? ` з ${sinceLabel}` : ''}`
            : `Синк буксує: ${pendingCount} очікують`}
        </button>
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
