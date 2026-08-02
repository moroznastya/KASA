/**
 * Сервіс системних команд Tauri (v2 Desktop API).
 *
 * ✅ Правильний шар викликів Rust-команд з src-tauri/src/commands/system.rs:
 *   - get_app_version           → invoke('get_app_version')
 *   - get_platform              → invoke('get_platform')
 *   - get_system_status         → invoke('get_system_status')
 *   - get_barcode_scanner_info  → invoke('get_barcode_scanner_info')
 *   - get_usb_devices           → invoke('get_usb_devices')
 *   - get_keyboard_layout       → invoke('get_keyboard_layout')
 *
 * ⚠️ Викликати ТІЛЬКИ у Tauri-режимі (перевірка isTauri() з @/hooks/useTauri).
 *    У браузері invoke буде кидати помилку — сторінка показує заглушку.
 */

import { invoke } from '@tauri-apps/api/core';

// ─── Типи (відповідають серіалізованим Rust-структурам) ────────────────────

export interface SystemStatus {
  version: string;
  platform: string;
  arch: string;
  online: boolean;
  hostname: string;
  username: string;
  app_data_dir: string;
}

export interface ScannerDevice {
  path: string;
  type: string;
}

export interface BarcodeScannerInfo {
  mode: string;
  devices: ScannerDevice[];
  note: string;
}

export interface UsbDevice {
  name: string;
  product: string;
  device: string;
}

// ─── Системні команди ───────────────────────────────────────────────────────

/**
 * Отримати версію застосунку (CARGO_PKG_VERSION).
 */
export async function getAppVersion(): Promise<string> {
  return invoke<string>('get_app_version');
}

/**
 * Отримати назву платформи (linux / windows / macos).
 */
export async function getPlatform(): Promise<string> {
  return invoke<string>('get_platform');
}

/**
 * Отримати стан системи: версія, платформа, архітектура, онлайн-статус,
 * hostname, користувач, шлях до app-data.
 */
export async function getSystemStatus(): Promise<SystemStatus> {
  return invoke<SystemStatus>('get_system_status');
}

/**
 * Отримати інформацію про сканер штрих-кодів (HID/keyboard-wedge).
 */
export async function getBarcodeScannerInfo(): Promise<BarcodeScannerInfo> {
  return invoke<BarcodeScannerInfo>('get_barcode_scanner_info');
}

/**
 * Отримати список USB-пристроїв (для налагодження).
 */
export async function getUsbDevices(): Promise<UsbDevice[]> {
  return invoke<UsbDevice[]>('get_usb_devices');
}

/**
 * Отримати клавіатурну розкладку (змінна LANG / LC_ALL).
 */
export async function getKeyboardLayout(): Promise<string> {
  return invoke<string>('get_keyboard_layout');
}
