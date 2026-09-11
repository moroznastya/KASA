import React, { useEffect, useState } from 'react';
import { Download } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { useUpdater } from '@/hooks/useUpdater';
import { isTauri } from '@/hooks/useTauri';

/**
 * Смуга «Доступне оновлення» (десктоп-обгортка Tauri).
 *
 * Це ЗВИЧАЙНИЙ блок У ПОТОЦІ документа: жодних шарів поверх контенту — смуга
 * НЕ перекриває UI, а резервує своє місце між хедером і контентом. Поки смуга
 * видима, її висота публікується у CSS-змінній --update-banner-h, щоб сторінки
 * з 100vh-розкладкою (POS, друк) віднімали її від calc(100vh - ...).
 *
 * Логіка:
 *  • ЛИШЕ Tauri: у web-режимі компонент не рендериться і не робить запитів.
 *  • Перевірка одноразова на сесію, через 5 с після монтування (гасить
 *    StrictMode double-mount через module-level прапорці).
 *  • БЕЗ автовстановлення: install() — тільки по кліку «Оновити зараз».
 *  • «Пізніше» — in-memory (до наступного запуску). Свідомо НЕ localStorage.
 *  • Помилки мережі/404 тихо логуються сервісом updater — банер просто не
 *    рендериться, без toast.
 */

/** Висота смуги у px — мусить дорівнювати класу h-11 (44px). */
export const UPDATE_BANNER_H = 44;

const CHECK_DELAY_MS = 5000;

/** Сесійні (in-memory) прапорці — навмисно НЕ localStorage. */
let pendingTimer: ReturnType<typeof setTimeout> | null = null;
let checkDone = false;
let dismissedThisSession = false;

export const UpdateBanner: React.FC = () => {
  const { installing, available, checkForUpdates, install } = useUpdater();
  const [dismissed, setDismissed] = useState(dismissedThisSession);

  // Перевірка одноразово на сесію, через 5 с після монтування.
  useEffect(() => {
    if (!isTauri()) return; // web-режим: жодних запитів
    if (checkDone || pendingTimer) return;
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

  const visible = isTauri() && !dismissed && !!available;

  // Публікуємо висоту смуги в CSS-змінну, щоб сторінки віднімали її від 100vh.
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--update-banner-h', visible ? `${UPDATE_BANNER_H}px` : '0px');
    return () => {
      root.style.setProperty('--update-banner-h', '0px');
    };
  }, [visible]);

  if (!visible || !available) return null;

  const handleInstall = async () => {
    // installAndRelaunch() сам перезапускає застосунок при успіху; false —
    // лишаємо смугу як є, нічого не ламаємо.
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
      className="h-11 w-full flex items-center gap-3 border-b border-primary-200 dark:border-slate-700 bg-primary-50 dark:bg-slate-800 px-6 text-sm"
    >
      <Download className="w-4 h-4 text-primary-600 shrink-0" />
      <p className="flex-1 min-w-0 truncate text-gray-700 dark:text-gray-200">
        Доступна версія <b>{available.version}</b> (у вас {available.currentVersion})
      </p>
      <div className="ml-auto flex items-center gap-2 shrink-0">
        <Button
          variant="primary"
          size="sm"
          disabled={installing}
          onClick={() => void handleInstall()}
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
