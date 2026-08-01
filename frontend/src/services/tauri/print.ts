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
  /** Кількість копій (опціонально, за замовчуванням — 1) */
  copies?: number | null;
  /** Автоматичне відрізання паперу після друку (опціонально, за замовчуванням — true) */
  auto_cut?: boolean | null;
  /**
   * Фізична ширина етикетки в мм (опціонально; для термо-етикеток).
   * Якщо задані width_mm/height_mm — Rust масштабує PNG ТОЧНО під мм:
   * 58×40мм @ 203dpi → (384, 320) dots = 48×40мм фізично.
   */
  width_mm?: number | null;
  /** Фізична висота етикетки в мм (опціонально; для термо-етикеток) */
  height_mm?: number | null;
  /** Роздільна здатність принтера, dots/inch (опціонально, за замовчуванням — 203) */
  dpi?: number | null;
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
 * ✅ invoke('print_image', { data: { image_base64, printer_name, device_path, copies, auto_cut, width_mm, height_mm, dpi } })
 *
 * @param imageBase64 - Base64-рядок зображення (PNG)
 * @param printerName - Назва принтера (опціонально)
 * @param devicePath  - Шлях до пристрою (опціонально, напр. /dev/usb/lp0)
 * @param copies      - Кількість копій (опціонально, null = за замовчуванням у Rust)
 * @param autoCut     - Автоматичне відрізання паперу (опціонально, null = за замовчуванням у Rust)
 * @param widthMm     - Фізична ширина етикетки в мм (опціонально; для термо-етикеток)
 * @param heightMm    - Фізична висота етикетки в мм (опціонально; для термо-етикеток)
 * @param dpi         - Роздільна здатність принтера dots/inch (опціонально, null = 203)
 *
 * ## Розміри термо-етикеток (widthMm/heightMm/dpi)
 * Якщо задані widthMm/heightMm — Rust масштабує PNG ТОЧНО під фізичні мм:
 *   - `target_w = min(round(width_mm * dpi / 25.4), 384)`
 *   - `target_h = round(height_mm * dpi / 25.4)`
 *   - resize ЗАВЖДИ через Lanczos3
 *
 * Приклад: етикетка 58×40мм @ 203dpi → (384, 320) dots = 48×40мм фізично
 * (ширина 48мм — фізичне обмеження 58мм принтера, висота ТОЧНО 40мм).
 *
 * Якщо НЕ задані (звичайні чеки) — стара логіка (масштаб до 384 лише якщо > 384px).
 */
export async function printImage(
  imageBase64: string,
  printerName?: string,
  devicePath?: string,
  copies?: number | null,
  autoCut?: boolean | null,
  widthMm?: number | null,
  heightMm?: number | null,
  dpi?: number | null,
): Promise<PrintResult> {
  return invoke<PrintResult>('print_image', {
    data: {
      image_base64: imageBase64,
      printer_name: printerName ?? null,
      device_path: devicePath ?? null,
      copies: copies ?? null,
      auto_cut: autoCut ?? null,
      width_mm: widthMm ?? null,
      height_mm: heightMm ?? null,
      dpi: dpi ?? null,
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


// ─── Друк HTML (A4) — нативний системний діалог ─────────────────────────────

/**
 * Дані для друку HTML-документа (A4).
 *
 * ⚠️ Tauri v2: поля Rust-структури передаються в snake_case.
 */
export interface PrintHtmlData {
  /** Повний HTML-документ для друку (з <html> та CSS-стилями) */
  html: string;
  /** Назва принтера (підказка для системного діалогу; опціонально) */
  printer_name?: string | null;
}

/**
 * Друк HTML-документа НАТИВНО через системний діалог друку (webkit2gtk).
 *
 * На відміну від html2canvas → PNG → ESC/POS, цей шлях використовує
 * нативний рендер webkit2gtk, тому ПІДТРИМУЄ:
 *   - CSS Grid (сітка цінників на A4)
 *   - page-break (багатосторінкові документи)
 *   - SVG, шрифти, повний CSS
 *
 * ✅ invoke('print_html', { data: { html, printer_name } })
 *
 * @param html        - Повний HTML-документ
 * @param printerName - Назва принтера (підказка; опціонально)
 */
export async function printHtml(
  html: string,
  printerName?: string,
): Promise<PrintResult> {
  return invoke<PrintResult>('print_html', {
    data: {
      html: html,
      printer_name: printerName ?? null,
    },
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
