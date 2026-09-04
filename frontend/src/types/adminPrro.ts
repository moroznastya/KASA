/**
 * «ПРРО централізовано» (Етап 5 адмін-панелі, ТЗ 5.7) — типи відповіді
 * GET /api/v1/admin/stores/:store_id/prro-settings.
 *
 * ВАЖЛИВО (аномалія, зафіксована в admin_prro.rs): фактична модель зберігає
 * ОДИН глобальний ПРРО-реєстр на сервер (prro_settings/prro_shifts без
 * store_id), КЕП — файл ключа поза БД. Тому вкладка READ-ONLY: editable=false,
 * per-store PUT моделлю не підтримується (потрібна зміна моделі + sync).
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
  /** "global" — модель: один реєстр на сервер. */
  scope: string;
  /** false: per-store PUT не підтримується моделлю (read-only). */
  editable: boolean;
  reason: string;
  configured: boolean;
  settings: PrroSettingsView;
  key: PrroKeyStatus;
  last_shift: PrroLastShift | null;
  settings_updated_at: string | null;
}
