import React, { useEffect, useState } from 'react';
import { Download } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { useUpdater } from '@/hooks/useUpdater';
import { isTauri } from '@/hooks/useTauri';

/**
 * Ненав'язливий банер «Доступне оновлення» (десктоп-обгортка Tauri).
 *
 * Принципи:
 *  • Працює ЛИШЕ в Tauri: у web-режимі компонент одразу повертає null і не
 *    робить жодних мережевих запитів.
 *  • Перевірка одноразова на сесію: module-level (in-memory) прапорці гасять і
 *    повторні монтування, і StrictMode double-mount у dev.
 *  • БЕЗ автовстановлення: install() викликається ВИКЛЮЧНО по кліку «Оновити зараз».
 *  • «Пізніше» — in-memory (до наступного запуску застосунку). Свідомо НЕ localStorage.
 *  • Помилки мережі/404 тихо логуються сервісом (services/tauri/updater) →
 *    банер просто не рендериться, жодних toast про помилку тут немає.
 */

const CHECK_DELAY_MS = 5000;

/** Сесійні (in-memory) прапорці — навмисно НЕ localStorage. */
let pendingTimer: ReturnType<typeof setTimeout> | null = null; // активний таймер перевірки
let checkDone = false; // перевірку вже запущено в цій сесії
let dismissedThisSession = false; // «Пізніше» — до наступного запуску застосунку

export const UpdateBanner: React.FC = () => {
  const { installing, available, checkForUpdates, install } = useUpdater();
  const [dismissed, setDismissed] = useState(dismissedThisSession);

  useEffect(() => {
    if (!isTauri()) return; // web-режим: жодних запитів
    if (checkDone || pendingTimer) return; // уже перевірено / вже заплановано
    pendingTimer = setTimeout(() => {
      pendingTimer = null;
      checkDone = true;
      void checkForUpdates();
    }, CHECK_DELAY_MS);
    return () => {
      if (pendingTimer) {
        clearTimeout(pendingTimer);
        pendingTimer = null;
      }
    };
  }, [checkForUpdates]);

  if (!isTauri()) return null;
  if (dismissed || !available) return null;

  const handleInstall = async () => {
    // Успіх: installAndRelaunch() сам перезапускає застосунок. false —
    // оновлення не встановлено: банер лишаємо як є, нічого не ламаємо.
    await install();
  };

  const handleLater = () => {
    dismissedThisSession = true; // до наступного запуску застосунку
    setDismissed(true);
  };

  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed bottom-4 left-1/2 -translate-x-1/2 z-[9990] w-[min(92vw,640px)]
                 flex flex-wrap items-center gap-3 rounded-xl border border-primary-200
                 dark:border-slate-700 bg-white dark:bg-slate-800 shadow-lg px-4 py-3"
    >
      <Download className="w-5 h-5 text-primary-600 shrink-0" />
      <p className="flex-1 min-w-[12rem] text-sm text-gray-700 dark:text-gray-200">
        Доступна версія <b>{available.version}</b> (у вас {available.currentVersion})
      </p>
      <div className="flex items-center gap-2 ml-auto">
        <Button
          variant="primary"
          size="sm"
          disabled={installing}
          onClick={() => void handleInstall()}
          // Спінер усередині primary-кнопки має бути білим: у Spinner базовий
          // text-primary-600 збігається з фоном, тому перекриваємо через
          // descendant-варіант (вища специфічність за звичайну utility).
          className={installing ? '[&_svg]:text-white' : undefined}
          icon={installing ? <Spinner size="sm" /> : undefined}
        >
          {installing ? 'Встановлення…' : 'Оновити зараз'}
        </Button>
        <Button variant="secondary" size="sm" disabled={installing} onClick={handleLater}>
          Пізніше
        </Button>
      </div>
    </div>
  );
};
