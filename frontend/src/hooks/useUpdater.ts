/**
 * Хук для роботи з автооновленням Tauri (check / install / relaunch).
 *
 * Використання:
 *   const { checking, available, checkForUpdates, install } = useUpdater();
 */

import { useCallback, useState } from 'react';
import { isTauri } from '@/hooks/useTauri';
import {
  checkForUpdates as checkForUpdatesService,
  installAndRelaunch,
  UpdateInfo,
} from '@/services/tauri/updater';

export function useUpdater() {
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [available, setAvailable] = useState<UpdateInfo | null>(null);

  /** Перевірити наявність оновлень (тільки в Tauri). */
  const checkForUpdates = useCallback(async (): Promise<UpdateInfo | null> => {
    if (!isTauri()) return null;
    setChecking(true);
    try {
      const info = await checkForUpdatesService();
      setAvailable(info);
      return info;
    } finally {
      setChecking(false);
    }
  }, []);

  /** Встановити доступне оновлення та перезапустити застосунок. */
  const install = useCallback(async (): Promise<boolean> => {
    if (!isTauri() || !available) return false;
    setInstalling(true);
    try {
      return await installAndRelaunch();
    } finally {
      setInstalling(false);
    }
  }, [available]);

  return { checking, installing, available, checkForUpdates, install };
}
