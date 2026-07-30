/**
 * Сервіс для друку через Tauri v2 Desktop API.
 *
 * Надає функції для:
 *   - Прямого ESC/POS друку (генерація байтів на Rust стороні)
 *   - Print-as-Image (Base64 → PNG → ESC/POS)
 *   - HTML-друку через CUPS/lp
 *   - Текстового друку
 *   - Перегляду перед друком
 *   - Відкриття грошової скриньки
 *   - Отримання списку принтерів
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

export interface ReceiptItemPrintData {
  barcode?: string | null;
  name: string;
  quantity: number;
  price: number;
  total: number;
}

export interface ReceiptPrintData {
  shop_name: string;
  shop_address: string;
  tax_id: string;
  receipt_number: string;
  date: string;
  time: string;
  cashier: string;
  items: ReceiptItemPrintData[];
  total: number;
  payment_method: string;
  paid: number;
  change: number;
  footer?: string | null;
  original_receipt_number?: string | null;
  return_reason?: string | null;
}

/**
 * Дані для друку зображення (Print-as-Image).
 */
export interface PrintImageData {
  /** Base64-рядок зображення (PNG, без префіксу data:image/png;base64,) */
  imageBase64: string;
  /** Назва принтера (опціонально) */
  printerName?: string;
}

// ─── Основні функції друку ──────────────────────────────────────────────────

/**
 * Прямий ESC/POS друк чека.
 * Дані передаються в Rust, який генерує сирі ESC/POS байти
 * та надсилає на принтер.
 *
 * @param data - Дані для друку чека
 * @param printerName - Назва принтера (опціонально)
 * @param devicePath - Шлях до пристрою (опціонально)
 */
export async function printReceiptEscpos(
  data: ReceiptPrintData,
  printerName?: string,
  devicePath?: string,
): Promise<PrintResult> {
  return invoke<PrintResult>('print_receipt_escpos', {
    data,
    printerName: printerName ?? null,
    devicePath: devicePath ?? null,
  });
}

/**
 * Друк HTML-документа через системний друк (CUPS/lp).
 *
 * @param html - HTML-вміст
 * @param printerName - Назва принтера (опціонально)
 */
export async function printDocument(html: string, printerName?: string): Promise<PrintResult> {
  return invoke<PrintResult>('print_document', {
    html,
    printerName: printerName ?? null,
  });
}

/**
 * Друк текстового чека.
 *
 * @param text - Текст чека
 * @param printerName - Назва принтера (опціонально)
 */
export async function printReceiptText(text: string, printerName?: string): Promise<PrintResult> {
  return invoke<PrintResult>('print_receipt', {
    text,
    printerName: printerName ?? null,
  });
}

/**
 * Друк HTML-чека (через Chrome headless + ESC/POS).
 *
 * @param html - HTML-вміст
 * @param printerName - Назва принтера (опціонально)
 */
export async function printReceiptHtml(html: string, printerName?: string): Promise<PrintResult> {
  return invoke<PrintResult>('print_receipt_html', {
    html,
    printerName: printerName ?? null,
  });
}

/**
 * Друк растрового зображення (PNG).
 *
 * @param imageData - Байти зображення
 * @param printerName - Назва принтера (опціонально)
 */
export async function printRasterImage(imageData: number[], printerName?: string): Promise<PrintResult> {
  return invoke<PrintResult>('print_raster_image', {
    imageData,
    printerName: printerName ?? null,
  });
}

/**
 * Друк зображення (Base64) — Print-as-Image.
 *
 * React рендерить чек, конвертує в Base64 PNG, надсилає в Rust.
 * Base64 має бути без префіксу `data:image/png;base64,` — тільки чистий Base64.
 *
 * @param imageBase64 - Base64-рядок зображення (PNG)
 * @param printerName - Назва принтера (опціонально)
 */
export async function printImage(
  imageBase64: string,
  printerName?: string,
): Promise<PrintResult> {
  return invoke<PrintResult>('print_image', {
    data: {
      imageBase64,
      printerName: printerName ?? null,
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

// ─── Попередній перегляд ────────────────────────────────────────────────────

/**
 * Відкрити попередній перегляд чека.
 *
 * @param html - HTML-вміст для перегляду
 */
export async function printPreview(html: string): Promise<PrintResult> {
  return invoke<PrintResult>('print_preview', { html });
}

// ─── Грошова скринька ───────────────────────────────────────────────────────

/**
 * Відкрити грошову скриньку.
 *
 * @param devicePath - Шлях до пристрою (опціонально)
 */
export async function openCashDrawer(devicePath?: string): Promise<PrintResult> {
  return invoke<PrintResult>('open_cash_drawer', {
    devicePath: devicePath ?? null,
  });
}

// ─── Системна інформація ────────────────────────────────────────────────────

/**
 * Отримати інформацію про систему.
 */
export async function getSystemInfo(): Promise<SystemInfo> {
  return invoke<SystemInfo>('get_system_info');
}
