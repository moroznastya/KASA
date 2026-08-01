/**
 * Типи ПРРО (програмний РРО) — відповідають DTO бекенду
 * (backend/app/application/dto/prro_dto.py та backend/app/api/v2/receipts.py).
 */

/** Налаштування ПРРО (пароль ключа — тільки маска) */
export interface PrroSettings {
  key_file: string | null;
  /** Маска пароля ключа ('••••' — якщо пароль збережено) */
  key_password_masked: string | null;
  /** Формат ключа: pfx/p12/jks/pem/dat */
  key_format: string | null;
  /** Фіскальний номер ПРРО (ФН) */
  prro_fn: string | null;
  /** Податковий номер платника ПДВ (ТН) */
  prro_tn: string | null;
  /** Заводський номер ПРРО (ЗН) */
  prro_zn: string | null;
  /** Режим роботи: test / prod */
  mode: 'test' | 'prod';
  /** Адреса фіскального сервера (залежить від mode) */
  url: string | null;
  /** Чи відкрита поточна зміна ПРРО */
  shift_open: boolean;
  /** ПРРО онлайн (за даними statusRro) */
  online: boolean;
  /** Автоматична фіскалізація чеків після створення */
  auto_fiscalize: boolean;
}

/** Запит на збереження налаштувань ПРРО */
export interface PrroSettingsSaveRequest {
  prro_fn?: string | null;
  prro_tn?: string | null;
  prro_zn?: string | null;
  mode?: 'test' | 'prod' | null;
  key_password?: string | null;
  key_file_name?: string | null;
  key_file_base64?: string | null;
  auto_fiscalize?: boolean | null;
}

/** Статус ПРРО (statusRro/infoRro + локальний стан) */
export interface PrroStatus {
  /** Зміна відкрита */
  open_shift: boolean;
  /** ПРРО онлайн */
  online: boolean;
  /** Останній підписант (серійний номер ключа) */
  last_signer: string | null;
  /** Назва ПРРО */
  name: string | null;
  /** Адреса ТО */
  addr: string | null;
  /** Фіскальний номер ПРРО */
  fn: string | null;
}

/** Зміна ПРРО (касова зміна / Z-звіт) */
export interface PrroShift {
  id: string;
  shift_number: number;
  opened_at: string;
  closed_at: string | null;
  signer_name: string | null;
  status: 'open' | 'closed';
  receipt_count: number;
  total_amount: string;
  zreport_number: string | null;
}

/** Результат фіскалізації чеку */
export interface FiscalizeResult {
  receipt_id: string;
  /** Статус: none / sent / failed / pending */
  fiscal_status: 'none' | 'sent' | 'failed' | 'pending';
  /** Фіскальний номер чеку, присвоєний податковою */
  fiscal_number: string | null;
  /** Фіскальний серійний номер (id_sign з CheckResponse) */
  fiscal_serial: string | null;
  /** Дата/час успішної відправки у податкову */
  fiscal_sent_at: string | null;
  /** Текст помилки при відправці */
  error: string | null;
  /** ID пов'язаного чеку при розділенні */
  split_receipt_id: string | null;
  /** URL перевірки фіскального чеку (для QR-коду на друку) */
  fiscal_check_url: string | null;
  /** Попередження (наприклад, часткова фіскалізація) */
  warning: string | null;
}

/** Запис журналу офлайн-черги ПРРО */
export interface PrroQueueItem {
  id: string;
  receipt_id: string | null;
  shift_id: string | null;
  local_number: number;
  check_type: string;
  /** pending / sent / failed */
  status: 'pending' | 'sent' | 'failed';
  error: string | null;
  created_at: string | null;
  sent_at: string | null;
}

/** Пагінована відповідь списку змін ПРРО */
export interface PrroShiftsResponse {
  items: PrroShift[];
  total: number;
  page: number;
  size: number;
}

/** Пагінована відповідь журналу черги ПРРО */
export interface PrroQueueResponse {
  items: PrroQueueItem[];
  total: number;
  page: number;
  size: number;
}

/** Відповідь test-connection */
export interface PrroTestConnectionResult {
  ok?: boolean;
  success?: boolean;
  message?: string;
  detail?: string;
  ping_ms?: number;
  [key: string]: unknown;
}

/** Фіскальні реквізити чеку (з v2 receipts) */
export interface ReceiptFiscalInfo {
  id: string;
  is_fiscal: boolean;
  fiscal_status: string;
  fiscal_number: string | null;
  fiscal_serial: string | null;
  fiscal_sent_at: string | null;
  fiscal_error: string | null;
  fiscal_check_url: string | null;
}

/** Мапа зрозумілих повідомлень для помилок ПРРО */
export const PRRO_ERROR_MESSAGES: Record<string, string> = {
  ERROR_NOT_OPEN_SHIFT: 'Зміна не відкрита',
  ERROR_OPEN_SHIFT_ALREADY: 'Зміна вже відкрита',
  ERROR_NOT_FOUND_RECEIPT: 'Чек не знайдено',
  ERROR_ALREADY_FISCALIZED: 'Чек вже фіскалізовано',
  ERROR_OFFLINE: 'ПРРО офлайн — чек додано до черги',
  ERROR_INVALID_KEY: 'Недійсний ключ КЕП',
  ERROR_KEY_PASSWORD: 'Невірний пароль ключа',
  ERROR_CONNECTION: 'Немає зв’язку з фіскальним сервером',
  ERROR_INVALID_FN: 'Недійсний фіскальний номер ПРРО',
  ERROR_INVALID_TN: 'Недійсний податковий номер',
  ERROR_INVALID_ZN: 'Недійсний заводський номер',
  ERROR_TAXPAYER: 'Платника не знайдено в ДПС',
  ERROR_BAD_REQUEST: 'Невірний запит до ДПС',
  ERROR_TIMEOUT: 'Час очікування ДПС вичерпано',
};

/** Отримати зрозумілий текст помилки ПРРО */
export function getPrroErrorMessage(code: string | null | undefined): string {
  if (!code) return 'Невідома помилка ПРРО';
  return PRRO_ERROR_MESSAGES[code] || code;
}
