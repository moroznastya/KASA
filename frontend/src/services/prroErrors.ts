/**
 * Витяг тексту помилки з відповіді бекенду (FastAPI/Rust-шлюз) або виключення.
 *
 * Порядок (від найбільш інформативного до fallback):
 *   1. `response.data.detail` — FastAPI-контракт (string | array | object);
 *   2. `response.data.error` — Rust-шлюз ({"error": ...}) та Python
 *      test_connection ({"status":0,"ok":false,"error":"[КОД] текст"});
 *   3. `response.data.message` — загальне повідомлення;
 *   4. `response.data` як рядок — text/plain відповідь;
 *   5. `error.message` — виключення (мережева помилка);
 *   6. fallback «Помилка запиту до ПРРО» — ЛИШЕ якщо нічого немає.
 *
 * Вимога UX: причина помилки ПРРО (код + текст) НЕ ховається за fallback.
 */
export function extractErrorMessage(error: unknown): string {
  const data = (error as { response?: { data?: unknown } })?.response?.data;

  // 1-3: JSON-відповідь (об'єкт)
  if (data && typeof data === 'object') {
    const obj = data as { detail?: unknown; error?: unknown; message?: unknown };

    // 1. detail — FastAPI-контракт (Python HTTPException / Rust api_err)
    const detail = obj.detail;
    if (typeof detail === 'string' && detail.trim()) return detail;
    if (Array.isArray(detail)) return detail.map((d: any) => d?.msg || String(d)).join('; ');
    if (detail && typeof detail === 'object') return JSON.stringify(detail);

    // 2. error — Rust-шлюз / Python test_connection / інші
    const errorField = obj.error;
    if (typeof errorField === 'string' && errorField.trim()) return errorField;
    if (errorField && typeof errorField === 'object') {
      const msg = (errorField as { message?: unknown }).message;
      if (typeof msg === 'string' && msg.trim()) return msg;
      return JSON.stringify(errorField);
    }

    // 3. message — загальне повідомлення
    if (typeof obj.message === 'string' && obj.message.trim()) return obj.message;
  }

  // 4. data як рядок (text/plain відповідь)
  if (typeof data === 'string' && data.trim()) return data;

  // 5. текст виключення (мережева помилка, таймаут тощо)
  if (error instanceof Error && error.message) return error.message;

  // 6. рядок-аргумент
  if (typeof error === 'string' && error.trim()) return error;

  return 'Помилка запиту до ПРРО';
}
