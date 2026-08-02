/**
 * Tauri — системні сповіщення.
 *
 * Обгортає Rust-команду `send_notification`
 * (див. frontend/src-tauri/src/commands/system.rs).
 *
 * Використання (наприклад, у useOfflineSync, ПРРО, резервному копіюванні):
 *   import { sendSystemNotification } from '@/services/tauri/notifications';
 *   await sendSystemNotification('Синхронізація', 'Дані оновлено');
 */

import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '@/hooks/useTauri';

/**
 * Відправити системне сповіщення (лише в десктоп-обгортці Tauri).
 * У браузерному режимі виклик безпечно ігнорується.
 */
export async function sendSystemNotification(
  title: string,
  body: string,
): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('send_notification', { title, body });
  } catch (err) {
    console.error('Помилка відправки сповіщення:', err);
  }
}

/** Зручні пресети для типових сценаріїв */

/** Сповіщення про успішну синхронізацію офлайн-даних */
export function notifySyncComplete(amount: number): Promise<void> {
  return sendSystemNotification(
    'Синхронізація завершена',
    `Відправлено на сервер чеків: ${amount}`,
  );
}

/** Сповіщення про помилку (ПРРО, мережа тощо) */
export function notifyError(context: string, message: string): Promise<void> {
  return sendSystemNotification(`Помилка: ${context}`, message);
}
