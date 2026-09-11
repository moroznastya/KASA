/**
 * «Один магазин — один ПРРО» (адмін-панель) — типи
 * GET/PUT /api/v1/admin/stores/:store_id/prro-settings.
 *
 * Модель per-store: prro_settings/prro_shifts/prro_queue_items мають
 * store_id NOT NULL + RLS; конфіг точки пишеться ключами (store_id, key_name);
 * КЕП точки — окреме сховище (PrroKeyStore::for_store) — ключ/пароль
 * НІКОЛИ не повертаються у plaintext.
 */

export interface PrroSettingsView {
  /** prro_fn — фіскальний номер ФН (RRO-номер реєстру). */
  prro_fn: string;
  /** prro_tn — податковий номер. */
  prro_tn: string;
  /** prro_zn — заводський номер. */
  prro_zn: string;
  mode: string;
  url: string;
}

export interface PrroKeyStatus {
  /** "env" | "keystore" | "none". */
  source: string;
  file_configured: boolean;
  file_name: string | null;
  password_configured: boolean;
  /** Серійний № сертифіката КЕП (з останньої зміни; публічний атрибут). */
  signer_serial: string | null;
  signer_name: string | null;
}

export interface PrroLastShift {
  shift_number: number;
  status: string;
  opened_at: string | null;
  closed_at: string | null;
  receipt_count: number;
  zreport_number: string | null;
}

export interface StorePrroSettings {
  store_id: string;
  store_name: string;
  /** "store" — модель: окремий ПРРО-конфіг/зміни/черга на точку. */
  scope: string;
  /** true — per-store PUT підтримується (аномалію Етапа 5 закрито). */
  editable: boolean;
  reason: string | null;
  configured: boolean;
  settings: PrroSettingsView;
  key: PrroKeyStatus;
  last_shift: PrroLastShift | null;
  settings_updated_at: string | null;
}

/** Тіло PUT /admin/stores/:store_id/prro-settings (multipart). */
export interface PrroSettingsUpdateInput {
  prro_fn?: string;
  prro_tn?: string;
  prro_zn?: string;
  mode?: 'test' | 'prod';
  url?: string;
  key_password?: string;
  /** Файл ключа КЕП (опційно). */
  keyFile?: File | null;
}
