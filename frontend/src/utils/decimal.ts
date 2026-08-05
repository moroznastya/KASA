/**
 * Утиліти для введення дробових чисел з крапкою/комою.
 *
 * Проблема, яку вирішують: controlled <input type="number"> з onChange,
 * який одразу робить parseFloat(value)||0 і записує число в state —
 * через це проміжні стани введення («», «.», «0.», «12.») перезаписуються,
 * і крапку неможливо ввести.
 *
 * Рішення: дозволяємо state у вигляді рядка, що відповідає regex
 * /^\d*[.,]?\d*$/, а число парсимо ЛИШЕ при обчисленні сум, сабміті
 * або передачі в updatePrice/updateQuantity.
 */

/** Валідний проміжний стан дробового числа: «», «.», «0.», «12.», «12.75» */
export const DECIMAL_INPUT_REGEX = /^\d*[.,]?\d*$/;

/**
 * Нормалізує введений рядок: кома → крапка.
 * Повертає рядок, якщо він валідний за regex, інакше null.
 */
export const normalizeDecimalInput = (raw: string): string | null => {
  const normalized = raw.replace(',', '.');
  return DECIMAL_INPUT_REGEX.test(normalized) ? normalized : null;
};

/**
 * Парсить рядок/число у дробове число (кома → крапка).
 * Невалідні значення («», «.», NaN) → 0.
 */
export const parseDecimal = (value: string | number): number => {
  const num = parseFloat(String(value).replace(',', '.'));
  return Number.isFinite(num) ? num : 0;
};
