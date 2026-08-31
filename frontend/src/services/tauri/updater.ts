/**
 * Tauri Updater — автооновлення десктоп-обгортки Torgashka.
 *
 * Обгортає @tauri-apps/plugin-updater:
 *   - checkForUpdates()      → перевірка наявності нової версії
 *   - downloadAndInstall()   → завантаження + встановлення оновлення
 *   - checkAndInstall()      → перевірка + встановлення одним викликом
 *   - installAndRelaunch()   → встановлення + перезапуск застосунку
 *
 * У браузерному режимі (не Tauri) всі функції безпечно повертають null/false.
 */

import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { isTauri } from '@/hooks/useTauri';

/** Інформація про доступне оновлення */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
}

/**
 * Перевіряє наявність оновлення (endpoints з tauri.conf.json → plugins.updater).
 * Повертає інформацію про оновлення або null, якщо версія актуальна.
 */
export async function checkForUpdates(): Promise<UpdateInfo | null> {
  if (!isTauri()) return null;
  try {
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      currentVersion: update.currentVersion,
      date: update.date,
      body: update.body,
    };
  } catch (err) {
    console.error('Помилка перевірки оновлень:', err);
    return null;
  }
}

/**
 * Завантажує та встановлює доступне оновлення (без перезапуску).
 * Повертає true, якщо оновлення було знайдено і встановлено.
 */
export async function downloadAndInstall(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    const update = await check();
    if (!update) return false;
    await update.downloadAndInstall();
    return true;
  } catch (err) {
    console.error('Помилка встановлення оновлення:', err);
    return false;
  }
}

/**
 * Перевіряє оновлення і, якщо воно є, одразу встановлює його.
 */
export async function checkAndInstall(): Promise<boolean> {
  return downloadAndInstall();
}

/**
 * Встановлює доступне оновлення та перезапускає застосунок.
 * Використовує плагін process (relaunch) — має бути в capabilities.
 */
export async function installAndRelaunch(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    const update = await check();
    if (!update) return false;
    await update.downloadAndInstall();
    await relaunch();
    return true;
  } catch (err) {
    console.error('Помилка встановлення оновлення:', err);
    return false;
  }
}
