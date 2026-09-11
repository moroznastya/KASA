/* global console */

// Юніт-тест extractErrorMessage (TS-логіка помилок ПРРО).
// Запуск: bash tools/run_prro_errors_test.sh (esbuild → node:assert).
import assert from 'node:assert';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
// prroErrors.cjs — esbuild-збірка src/services/prroErrors.ts (див. скрипт нижче)
const { extractErrorMessage } = require('/tmp/prroErrors.cjs');

// 1. FastAPI-контракт: detail string
assert.strictEqual(
  extractErrorMessage({ response: { data: { detail: '[ERROR_SAVE] Помилка запису' } } }),
  '[ERROR_SAVE] Помилка запису'
);

// 2. FastAPI detail array (Pydantic-валідація)
assert.strictEqual(
  extractErrorMessage({ response: { data: { detail: [{ msg: 'a' }, { msg: 'b' }] } } }),
  'a; b'
);

// 3. Rust-шлюз: {"error": "..."} без detail — РАНІШЕ fallback, тепер причина
assert.strictEqual(
  extractErrorMessage({
    response: { data: { error: 'status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)' } },
  }),
  'status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)'
);

// 4. error object з message
assert.strictEqual(
  extractErrorMessage({ response: { data: { error: { message: 'мережева помилка' } } } }),
  'мережева помилка'
);

// 5. data string (text/plain відповідь)
assert.strictEqual(
  extractErrorMessage({ response: { data: 'status=-15 (ERROR_NOT_OPEN_SHIFT: Зміну не відкрито)' } }),
  'status=-15 (ERROR_NOT_OPEN_SHIFT: Зміну не відкрито)'
);

// 6. Виключення (мережева помилка): Error.message
assert.strictEqual(
  extractErrorMessage(new Error('Network Error: connect ECONNREFUSED 127.0.0.1:8000')),
  'Network Error: connect ECONNREFUSED 127.0.0.1:8000'
);

// 7. message поле
assert.strictEqual(
  extractErrorMessage({ response: { data: { message: '[PRRO_SETTINGS_ERROR] налаштуйте ПРРО' } } }),
  '[PRRO_SETTINGS_ERROR] налаштуйте ПРРО'
);

// 8. Fallback — ЛИШЕ коли нічого немає
assert.strictEqual(extractErrorMessage({}), 'Помилка запиту до ПРРО');
assert.strictEqual(extractErrorMessage(undefined), 'Помилка запиту до ПРРО');

// 9. Пріоритет detail над error
assert.strictEqual(
  extractErrorMessage({ response: { data: { detail: 'detail-переможець', error: 'error-програв' } } }),
  'detail-переможець'
);

console.log('extractErrorMessage: ALL TESTS PASSED');
