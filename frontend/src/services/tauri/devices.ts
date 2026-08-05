/**
 * Сервіс Tauri-команд для POS-обладнання: касові ваги (COM/RS-232),
 * WiFi-термінали ПриватБанку (TCP/IP) та CUPS-принтери чеків.
 *
 * ✅ Відповідає Rust-командам (src-tauri/src/commands/devices.rs):
 *   - get_available_ports     → invoke('get_available_ports')            → string[]
 *   - get_devices             → invoke('get_devices')                    → DeviceConfig[]
 *   - save_device_config      → invoke('save_device_config', { config }) → DeviceConfig
 *   - delete_device           → invoke('delete_device', { id })          → void
 *   - connect_device          → invoke('connect_device', { id })         → DeviceStatus
 *   - disconnect_device       → invoke('disconnect_device', { id })      → DeviceStatus
 *   - get_devices_status      → invoke('get_devices_status')             → DeviceStatus[]
 *   - test_connection         → invoke('test_connection', { deviceType, config }) → boolean
 *   - get_system_printers     → invoke('get_system_printers')            → PrinterInfo[]
 *   - get_detected_devices    → invoke('get_detected_devices')           → DetectedDevices
 *
 * Події (listen):
 *   - "device-status-changed" → DeviceStatus  ({ id, status, error, lastWeight })
 *   - "weight-updated"        → WeightUpdatedEvent ({ deviceId, value })
 *
 * ⚠️ Викликати ТІЛЬКИ у Tauri-режимі (перевірка isTauri() з @/hooks/useTauri).
 *    У браузері invoke буде кидати помилку — сторінка показує заглушку.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { UsbDevice } from './system';

// ─── Типи (відповідають серіалізованим Rust-структурам) ────────────────────

export type DeviceType = 'scale' | 'terminal' | 'printer';

/** Конфігурація пристрою (зберігається в налаштуваннях) */
export interface DeviceConfig {
  id: string;
  name: string;
  deviceType: DeviceType;
  enabled: boolean;
  config: {
    /** COM-порт (scale) */
    port?: string;
    /** Швидкість (scale) */
    baudRate?: number;
    /** IP терміналу (terminal) */
    ip?: string;
    /** Порт терміналу (terminal) */
    tcpPort?: number;
    /** Назва CUPS-принтера (printer) */
    printerName?: string;
  };
}

/** Поточний статус підключення пристрою */
export interface DeviceStatus {
  id: string;
  status: 'connected' | 'disconnected' | 'error';
  error?: string | null;
  lastWeight?: number | null;
}

/** Результат передачі суми на термінал (команда terminal_payment) */
export interface TerminalPaymentResult {
  terminalName: string;
  amount: number;
  sent: boolean;
}

/** Payload події "weight-updated" */
export interface WeightUpdatedEvent {
  deviceId: string;
  value: number;
}

/** Інформація про CUPS-принтер системи */
export interface PrinterInfo {
  name: string;
  status: 'idle' | 'printing' | 'disabled' | 'error';
  isDefault: boolean;
}

/** Інформація про сканер (SANE) */
export interface ScannerInfo {
  device: string; // SANE-ідентифікатор
  name: string;   // опис
}

/** Результат автовиявлення всіх підключених пристроїв */
export interface DetectedDevices {
  printers: PrinterInfo[];
  serialPorts: string[];
  usbDevices: UsbDevice[];
  scanners: ScannerInfo[];
}

// ─── Команди ────────────────────────────────────────────────────────────────

export const devicesApi = {
  /**
   * Отримати список доступних COM-портів системи.
   * @returns масив імен портів, напр. ["/dev/ttyUSB0", "/dev/ttyS0"]
   */
  getAvailablePorts: (): Promise<string[]> => invoke<string[]>('get_available_ports'),

  /**
   * Отримати список збережених конфігурацій пристроїв.
   */
  getDevices: (): Promise<DeviceConfig[]> => invoke<DeviceConfig[]>('get_devices'),

  /**
   * Зберегти конфігурацію пристрою (створити або оновити).
   * @param config конфігурація пристрою
   * @returns збережена конфігурація (з актуальним id)
   */
  saveDeviceConfig: (config: DeviceConfig): Promise<DeviceConfig> =>
    invoke<DeviceConfig>('save_device_config', { config }),

  /**
   * Видалити пристрій за id.
   * @param id ідентифікатор пристрою
   */
  deleteDevice: (id: string): Promise<void> => invoke<void>('delete_device', { id }),

  /**
   * Підключитися до пристрою.
   * @param id ідентифікатор пристрою
   * @returns оновлений статус
   */
  connectDevice: (id: string): Promise<DeviceStatus> => invoke<DeviceStatus>('connect_device', { id }),

  /**
   * Відключитися від пристрою.
   * @param id ідентифікатор пристрою
   * @returns оновлений статус
   */
  disconnectDevice: (id: string): Promise<DeviceStatus> => invoke<DeviceStatus>('disconnect_device', { id }),

  /**
   * Отримати статуси всіх пристроїв.
   */
  getDevicesStatus: (): Promise<DeviceStatus[]> => invoke<DeviceStatus[]>('get_devices_status'),

  /**
   * Перевірити з'єднання з пристроєм за типом та конфігурацією (без збереження).
   * @param deviceType тип пристрою: 'scale' | 'terminal' | 'printer'
   * @param config конфігурація пристрою (поле config)
   * @returns true — з'єднання успішне, false — помилка
   */
  testConnection: (deviceType: DeviceType, config: object): Promise<boolean> =>
    invoke<boolean>('test_connection', { deviceType, config }),

  /**
   * Передати суму на підключений картковий термінал ПриватБанку.
   * @param amount сума у гривнях (копійки не підтримуються терміналом)
   * @returns TerminalPaymentResult ({ terminalName, amount, sent })
   */
  terminalPayment: (amount: number): Promise<TerminalPaymentResult> =>
    invoke<TerminalPaymentResult>('terminal_payment', { amount }),

  /**
   * Отримати список CUPS-принтерів системи.
   * @returns масив PrinterInfo ({ name, status, isDefault })
   */
  getSystemPrinters: (): Promise<PrinterInfo[]> => invoke<PrinterInfo[]>('get_system_printers'),

  /**
   * Автовиявлення всіх підключених пристроїв:
   * принтери (CUPS), COM-порти, USB-пристрої та сканери (SANE).
   * @returns DetectedDevices ({ printers, serialPorts, usbDevices, scanners })
   */
  getDetectedDevices: (): Promise<DetectedDevices> => invoke<DetectedDevices>('get_detected_devices'),
};

// ─── Події ──────────────────────────────────────────────────────────────────

export const devicesEvents = {
  /**
   * Підписатися на зміну статусу пристрою ("device-status-changed").
   * @returns Promise<UnlistenFn> — функція скасування підписки
   */
  onDeviceStatusChanged: (callback: (status: DeviceStatus) => void): Promise<UnlistenFn> =>
    listen<DeviceStatus>('device-status-changed', (event) => callback(event.payload)),

  /**
   * Підписатися на оновлення ваги ("weight-updated").
   * @returns Promise<UnlistenFn> — функція скасування підписки
   */
  onWeightUpdated: (callback: (payload: WeightUpdatedEvent) => void): Promise<UnlistenFn> =>
    listen<WeightUpdatedEvent>('weight-updated', (event) => callback(event.payload)),
};
