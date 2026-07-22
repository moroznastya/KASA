import { useCallback, useEffect, useRef } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';

const NAV_STACK_KEY = 'nav_stack';
const MAX_STACK_SIZE = 50;

interface NavEntry {
  path: string;
  key: string;
}

/**
 * Хук для правильної навігації "Назад".
 * Зберігає стек відвідуваних сторінок в sessionStorage.
 * 
 * Логіка роботи:
 * - Кожен новий шлях додається в стек (крім повторів поспіль)
 * - При виклику goBack() — видаляє поточний шлях зі стеку і повертає на попередній
 * - Якщо стек порожній — повертає на батьківський маршрут
 * - Використовує ref, щоб не додавати шлях назад після навігації "Назад"
 */
export function useBackNavigation() {
  const navigate = useNavigate();
  const location = useLocation();
  const isNavigatingBack = useRef(false);

  // Додаємо поточний шлях в стек при зміні location
  useEffect(() => {
    // Якщо це навігація "Назад" — не додаємо шлях знову
    if (isNavigatingBack.current) {
      isNavigatingBack.current = false;
      return;
    }

    try {
      const stack = getNavStack();
      const lastEntry = stack[stack.length - 1];

      // Додаємо тільки якщо це не той самий шлях
      if (!lastEntry || lastEntry.path !== location.pathname) {
        stack.push({
          path: location.pathname,
          key: location.key,
        });

        // Обмежуємо розмір стеку
        if (stack.length > MAX_STACK_SIZE) {
          stack.splice(0, stack.length - MAX_STACK_SIZE);
        }

        sessionStorage.setItem(NAV_STACK_KEY, JSON.stringify(stack));
      }
    } catch {
      // Ігноруємо помилки
    }
  }, [location.pathname, location.key]);

  const goBack = useCallback(() => {
    try {
      const stack = getNavStack();

      // Видаляємо поточний шлях зі стеку
      stack.pop();

      // Беремо попередній шлях
      const previousEntry = stack[stack.length - 1];

      if (previousEntry && previousEntry.path !== location.pathname) {
        // Позначаємо, що це навігація "Назад"
        isNavigatingBack.current = true;
        // Оновлюємо стек (без поточного шляху)
        sessionStorage.setItem(NAV_STACK_KEY, JSON.stringify(stack));
        navigate(previousEntry.path);
      } else {
        // Якщо попереднього шляху немає — повертаємось на батьківський
        isNavigatingBack.current = true;
        const parentPath = getParentPath(location.pathname);
        navigate(parentPath);
      }
    } catch {
      // У випадку помилки — на батьківський шлях
      isNavigatingBack.current = true;
      navigate(getParentPath(location.pathname));
    }
  }, [navigate, location.pathname]);

  return { goBack };
}

function getNavStack(): NavEntry[] {
  try {
    const saved = sessionStorage.getItem(NAV_STACK_KEY);
    return saved ? JSON.parse(saved) : [];
  } catch {
    return [];
  }
}

function getParentPath(path: string): string {
  // Видаляємо останній сегмент шляху
  const parts = path.split('/').filter(Boolean);
  parts.pop();
  return parts.length > 0 ? '/' + parts.join('/') : '/';
}
