/**
 * Індикатор режиму вузла (ADR-0007 §10.2, §11.3) — Фаза 3.1а/3.1б.
 *
 * Джерело істини — `GET /local/status` (`route_local.rs::local_status`), а НЕ
 * `navigator.onLine`: браузерна мережа нічого не каже про доступність primary,
 * а каса на standby мусить бачити різницю між двома різними станами:
 *
 *   • `lagging` — primary ДОСТУПНИЙ, черга > 0 → **НОРМА**: синк іде,
 *     тривожити користувача не можна (спокійний синій + лічильник);
 *   • `offline` — primary НЕДОСТУПНИЙ → деградація: черга росте
 *     (виразний червоний + лічильник + текст про офлайн).
 *
 * Полінг: 20 с + додатково при поверненні фокуса/вкладки (не частіше 5 с).
 * Тости — ЛИШЕ на переходах стану, ніколи на кожному полінгу.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import toast from 'react-hot-toast';
import { CheckCircle2, CloudOff, RefreshCw } from 'lucide-react';
import { getNodeStatus, type NodeEffective, type NodeStatus } from '@/services/nodeStatusService';

/** Період полінгу /local/status (мс). 20 с — у межах «15–30 с», без шуму в мережі. */
const POLL_INTERVAL_MS = 20_000;

/** Мінімальний інтервал між запитами при фокусі вікна (антидребезг). */
const FOCUS_MIN_GAP_MS = 5_000;

export interface NodeStatusState {
  /** Останній успішно прочитаний статус (null — вузол не standby / API недоступний). */
  status: NodeStatus | null;
  /** true, коли вузол підтверджено standby і /local/status відповідає. */
  available: boolean;
}

/**
 * Хук статусу вузла.
 *
 * - перший запит одразу після монтування;
 * - далі кожні 20 с;
 * - додатково при `focus` / поверненні вкладки з фону (з антидребезгом 5 с);
 * - сповіщення про ЗМІНУ стану (не про кожен полінг).
 *
 * `effective` поза standby вузлом не читається (за 404/400 → available=false),
 * тому на primary-касі індикатор не рендериться взагалі.
 */
// react-refresh: файл свідомо експортує хук + компонент (NodeStatusIndicator нижче)
// eslint-disable-next-line react-refresh/only-export-components
export function useNodeStatus(): NodeStatusState {
  const [status, setStatus] = useState<NodeStatus | null>(null);
  const [available, setAvailable] = useState<boolean>(false);

  /** Попередній `effective` — база для детекції ПЕРЕХОДІВ (тости лише тут). */
  const prevEffectiveRef = useRef<NodeEffective | null>(null);
  /** Час останнього запиту — антидребезг для focus/visibilitychange. */
  const lastFetchAtRef = useRef<number>(0);

  const refresh = useCallback(async () => {
    lastFetchAtRef.current = Date.now();
    const next = await getNodeStatus();

    // Не standby (404) або локальний API мовчить → індикатора немає.
    // Базу переходів скидаємо: після повернення не буде хибного «відновлено».
    if (!next || next.mode !== 'standby') {
      prevEffectiveRef.current = null;
      setAvailable(false);
      setStatus(null);
      return;
    }

    setAvailable(true);
    setStatus(next);

    const prev = prevEffectiveRef.current;
    prevEffectiveRef.current = next.effective;

    // Перше спостереження (prev === null) — це БАЗА, а не перехід: без тосту,
    // інакше кожен запуск застосунку в standby давав би toast про lagging.
    if (prev === null || prev === next.effective) return;

    if (next.effective === 'offline') {
      // Деградація: primary недоступний.
      toast.error(
        `Немає зв'язку з головним сервером. Документи йдуть у локальну чергу (${next.queue_pending})`,
        { duration: 6000 }
      );
      return;
    }

    if (prev === 'offline') {
      // Повернення зв'язку після офлайну (у т.ч. одразу в lagging — черга ще йде).
      toast.success('Звʼязок з головним сервером відновлено');
      return;
    }

    if (next.effective === 'lagging') {
      // НОРМА, не тривога: спокійне інформування без toast.error.
      toast(`Черга синхронізується: ${next.queue_pending} операцій очікують сервер`, {
        icon: '⏳',
      });
    }
  }, []);

  useEffect(() => {
    void refresh();
    const interval = setInterval(() => void refresh(), POLL_INTERVAL_MS);

    // Свіжий статус при поверненні до вікна (каса згорнута/інша вкладка).
    const maybeRefresh = () => {
      if (document.visibilityState === 'hidden') return;
      if (Date.now() - lastFetchAtRef.current < FOCUS_MIN_GAP_MS) return;
      void refresh();
    };

    window.addEventListener('focus', maybeRefresh);
    document.addEventListener('visibilitychange', maybeRefresh);
    return () => {
      clearInterval(interval);
      window.removeEventListener('focus', maybeRefresh);
      document.removeEventListener('visibilitychange', maybeRefresh);
    };
  }, [refresh]);

  return { status, available };
}

/**
 * Компактний індикатор у хедері. Три ВІЗУАЛЬНО РІЗНІ стани:
 *
 *   1. active  — зелене «Сервер доступний» (нейтрально, без тривоги, без чисел);
 *   2. lagging — синє «Синхронізація» + лічильник черги (звичайна інформація,
 *                `title` прямо каже «це норма»);
 *   3. offline — ЧЕРВОНЕ «Немає зв'язку з сервером» + лічильник (виразне
 *                попередження, інша іконка) + `title` з поясненням про чергу.
 *
 * Верстка: `inline-flex` + `whitespace-nowrap` + `shrink-0`; текстова частина
 * ховається нижче `lg` (лишаються іконка + лічильник), щоб не ламати хедер на
 * вузькому екрані каси. Лічильник показується лише коли є що показувати.
 */
export const NodeStatusIndicator: React.FC = () => {
  const { status, available } = useNodeStatus();

  if (!available || !status || status.mode !== 'standby') return null;

  const pending = status.queue_pending;

  // ── offline: деградація (виразне попередження) ───────────────────────────
  if (status.effective === 'offline') {
    return (
      <span
        role="status"
        aria-live="polite"
        title={`Немає зв'язку з головним сервером. Продажі працюють: документи пишуться в локальну чергу (${pending}). Синхронізація відбудеться автоматично, коли зв'язок повернеться.`}
        className="inline-flex items-center gap-1.5 shrink-0 whitespace-nowrap px-2.5 py-1 rounded-full border text-xs font-semibold bg-danger-50 dark:bg-danger-900/20 text-danger-700 dark:text-danger-300 border-danger-200 dark:border-danger-800"
      >
        <CloudOff className="w-3.5 h-3.5 shrink-0" />
        <span className="hidden lg:inline">Немає звʼязку з сервером</span>
        <span className="lg:hidden">Офлайн</span>
        <span className="rounded-full bg-white/70 dark:bg-slate-900/40 px-1.5 tabular-nums">
          {pending}
        </span>
      </span>
    );
  }

  // ── lagging: НОРМА — синк іде, черга не порожня ──────────────────────────
  if (status.effective === 'lagging') {
    return (
      <span
        role="status"
        aria-live="polite"
        title={`Черга синхронізується: ${pending} операцій очікують відправки на головний сервер. Це нормальний стан — дані дійдуть автоматично.`}
        className="inline-flex items-center gap-1.5 shrink-0 whitespace-nowrap px-2.5 py-1 rounded-full border text-xs font-semibold bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300 border-primary-200 dark:border-primary-800"
      >
        <RefreshCw className="w-3.5 h-3.5 shrink-0" />
        <span className="hidden lg:inline">Синхронізація черги</span>
        <span className="lg:hidden">Синк</span>
        <span className="rounded-full bg-white/70 dark:bg-slate-900/40 px-1.5 tabular-nums">
          {pending}
        </span>
      </span>
    );
  }

  // ── active: усе нормально (нейтрально-зелене, без чисел) ─────────────────
  return (
    <span
      role="status"
      aria-live="polite"
      title="Головний сервер доступний, черга порожня (вузол у режимі standby)"
      className="inline-flex items-center gap-1.5 shrink-0 whitespace-nowrap px-2.5 py-1 rounded-full border text-xs font-medium bg-success-50 dark:bg-success-900/20 text-success-700 dark:text-success-400 border-success-200 dark:border-success-800"
    >
      <CheckCircle2 className="w-3.5 h-3.5 shrink-0" />
      <span className="hidden lg:inline">Сервер доступний</span>
    </span>
  );
};
