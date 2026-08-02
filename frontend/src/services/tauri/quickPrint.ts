/**
 * Tauri — швидкий друк чека через глобальну гарячу клавішу Ctrl+Shift+P.
 *
 * Rust-бекенд реєструє гарячу клавішу (див. frontend/src-tauri/src/lib.rs)
 * і при натисканні надсилає у frontend подію "quick-print-receipt".
 * Цей сервіс дозволяє підписатися на неї.
 *
 * Використання:
 *   const unlisten = await onQuickPrint(() => {
 *     // надрукувати останній чек без діалогу
 *   });
 *   // ... при розмонтуванні:
 *   unlisten?.();
 */

import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from '@/hooks/useTauri';

export type QuickPrintHandler = () => void;

/**
 * Підписатися на подію "quick-print-receipt" (Ctrl+Shift+P).
 * Повертає функцію скасування підписки або null поза Tauri.
 */
export async function onQuickPrint(
  handler: QuickPrintHandler,
): Promise<UnlistenFn | null> {
  if (!isTauri()) return null;
  return listen('quick-print-receipt', () => {
    handler();
  });
}
