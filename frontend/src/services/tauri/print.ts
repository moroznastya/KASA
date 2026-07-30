/**
 * Сервіс для друку через Tauri v2 Desktop API.
 *
 * Надає функції для:
 *   - Print-as-Image (Base64 → PNG → ESC/POS растр)
 *   - Растрового друку (байти → PNG → принтер)
 *   - Відкриття грошової скриньки
 *   - Отримання списку принтерів
 *   - 🖼️ Збереження PNG на диск (для дебагу)
 */

import { invoke } from '@tauri-apps/api/core';

// ─── Типи ───────────────────────────────────────────────────────────────────

export interface PrintResult {
  success: boolean;
  message: string;
  bytes_sent?: number;
}

export interface SystemInfo {
  platform: string;
  arch: string;
  hostname: string;
  username: string;
}

/**
 * Дані для друку зображення (Print-as-Image).
 *
 * ⚠️ Tauri v2: поля Rust-структури передаються в snake_case.
 */
export interface PrintImageData {
  /** Base64-рядок зображення (PNG, без префіксу data:image/png;base64,) */
  image_base64: string;
  /** Назва принтера (опціонально) */
  printer_name?: string | null;
  /** Шлях до пристрою (опціонально, напр. /dev/usb/lp0) */
  device_path?: string | null;
}

// ─── Основні функції друку ──────────────────────────────────────────────────

/**
 * Друк зображення (Base64) — Print-as-Image.
 *
 * React рендерить чек, конвертує в Base64 PNG, надсилає в Rust.
 * Base64 має бути без префіксу `data:image/png;base64,` — тільки чистий Base64.
 *
 * ⚠️ Tauri v2: команда `print_image(data: PrintImageData)` очікує
 *    аргумент з ІМЕНЕМ `data` та snake_case полями всередині.
 *
 * ✅ invoke('print_image', { data: { image_base64, printer_name, device_path } })
 *
 * @param imageBase64 - Base64-рядок зображення (PNG)
 * @param printerName - Назва принтера (опціонально)
 * @param devicePath  - Шлях до пристрою (опціонально, напр. /dev/usb/lp0)
 */
export async function printImage(
  imageBase64: string,
  printerName?: string,
  devicePath?: string,
): Promise<PrintResult> {
  return invoke<PrintResult>('print_image', {
    data: {
      image_base64: imageBase64,
      printer_name: printerName ?? null,
      device_path: devicePath ?? null,
    },
  });
}

/**
 * Друк растрового зображення (байти PNG) — низькорівнева команда.
 *
 * @param imageData   - Масив байтів зображення
 * @param printerName - Назва принтера (опціонально)
 * @param devicePath  - Шлях до пристрою (опціонально, напр. /dev/usb/lp0)
 */
export async function printRasterImage(
  imageData: number[],
  printerName?: string,
  devicePath?: string,
): Promise<PrintResult> {
  return invoke<PrintResult>('print_raster_image', {
    image_data: imageData,
    printer_name: printerName ?? null,
    device_path: devicePath ?? null,
  });
}

// ─── Керування принтерами ───────────────────────────────────────────────────

/**
 * Отримати список доступних принтерів.
 */
export async function getPrinters(): Promise<string[]> {
  return invoke<string[]>('get_printers');
}

// ─── Грошова скринька ───────────────────────────────────────────────────────

/**
 * Відкрити грошову скриньку.
 *
 * @param devicePath - Шлях до пристрою (опціонально)
 */
export async function openCashDrawer(devicePath?: string): Promise<PrintResult> {
  return invoke<PrintResult>('open_cash_drawer', {
    device_path: devicePath ?? null,
  });
}

// ─── Системна інформація ────────────────────────────────────────────────────

/**
 * Отримати інформацію про систему.
 */
export async function getSystemInfo(): Promise<SystemInfo> {
  return invoke<SystemInfo>('get_system_info');
}

// ═════════════════════════════════════════════════════════════════════════════
// 🖼️ Збереження зображення чека на диск (для дебагу/перевірки)
// ═════════════════════════════════════════════════════════════════════════════

/**
 * Зберегти PNG-зображення чека на диск у ~/Downloads/.
 *
 * Приймає чистий Base64 (без префіксу data:image/png;base64,).
 * Повертає повний шлях до збереженого файлу.
 *
 * ⚠️ Tauri v2: для простих параметрів camelCase = snake_case в Rust.
 *    Rust: `image_base64: String` → JS: `imageBase64: string`
 *
 * @param imageBase64 - Base64-рядок зображення (PNG)
 * @returns Шлях до збереженого файлу
 */
export async function saveReceiptImage(imageBase64: string): Promise<string> {
  return invoke<string>('save_receipt_image', {
    imageBase64: imageBase64,
  });
}
