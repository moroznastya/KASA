import { useCallback, useEffect } from 'react';
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
 * При виклику goBack() повертає на попередню сторінку зі стеку.
 * Якщо стек порожній — повертає на батьківський маршрут.
 */
export function useBackNavigation() {
  const navigate = useNavigate();
  const location = useLocation();

  // Додаємо поточний шлях в стек при монтуванні
  useEffect(() => {
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
      
      // Видаляємо поточний шлях
      stack.pop();
      
      // Беремо попередній шлях
      const previousEntry = stack[stack.length - 1];
      
      if (previousEntry && previousEntry.path !== location.pathname) {
        // Оновлюємо стек (без поточного шляху)
        sessionStorage.setItem(NAV_STACK_KEY, JSON.stringify(stack));
        navigate(previousEntry.path);
      } else {
        // Якщо попереднього шляху немає — повертаємось на батьківський
        const parentPath = getParentPath(location.pathname);
        navigate(parentPath);
      }
    } catch {
      // У випадку помилки — на батьківський шлях
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
