import { useState, useCallback, useEffect } from 'react';
import { useLocation } from 'react-router-dom';

/**
 * Хук для збереження стану сторінки в sessionStorage.
 * При переході на іншу сторінку стан зберігається,
 * при поверненні назад — відновлюється.
 * 
 * @param keyPrefix - унікальний префікс для ключа в sessionStorage
 * @param initialState - початковий стан
 * @returns [state, setState] - стан і функція для його оновлення
 */
export function usePageState<T extends Record<string, any>>(
  keyPrefix: string,
  initialState: T
): [T, (newState: Partial<T> | ((prev: T) => Partial<T>)) => void] {
  const location = useLocation();
  const storageKey = `page_state_${keyPrefix}`;

  // Ініціалізація стану: спочатку з sessionStorage, потім initialState
  const [state, setStateInternal] = useState<T>(() => {
    try {
      const saved = sessionStorage.getItem(storageKey);
      if (saved) {
        const parsed = JSON.parse(saved);
        // Об'єднуємо з initialState, щоб нові поля додавались, а старі мали дефолти
        return { ...initialState, ...parsed };
      }
    } catch {
      // Ігноруємо помилки парсингу
    }
    return initialState;
  });

  // Зберігаємо стан в sessionStorage при кожній зміні
  useEffect(() => {
    try {
      sessionStorage.setItem(storageKey, JSON.stringify(state));
    } catch {
      // Ігноруємо помилки запису
    }
  }, [state, storageKey]);

  // Очищаємо збережений стан при розмонтуванні (якщо не повертаємось назад)
  // Але ми не можемо знати, чи повернеться користувач назад,
  // тому залишаємо стан в sessionStorage до наступного візиту
  // Він перезапишеться при новому вході на сторінку

  const setState = useCallback(
    (newState: Partial<T> | ((prev: T) => Partial<T>)) => {
      setStateInternal((prev) => {
        const update = typeof newState === 'function' ? newState(prev) : newState;
        return { ...prev, ...update };
      });
    },
    []
  );

  return [state, setState];
}
