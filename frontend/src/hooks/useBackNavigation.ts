import { useCallback } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';

/**
 * Хук для навігації "Назад".
 * 
 * Використовує нативний window.history.back() — це гарантує,
 * що програмна кнопка "Назад" працює ТОЧНО ТАК САМО, як кнопка браузера.
 * 
 * Якщо історія порожня (не можна повернутись назад) — 
 * переходимо на батьківський маршрут (наприклад, /receipts/:id -> /receipts).
 */
export function useBackNavigation() {
  const navigate = useNavigate();
  const location = useLocation();

  const goBack = useCallback(() => {
    // Використовуємо нативний history API — це те саме, що кнопка браузера "Назад"
    if (window.history.length > 1) {
      window.history.back();
    } else {
      // Якщо історія порожня — повертаємось на батьківський маршрут
      navigate(getParentPath(location.pathname));
    }
  }, [navigate, location.pathname]);

  return { goBack };
}

function getParentPath(path: string): string {
  // Видаляємо останній сегмент шляху
  const parts = path.split('/').filter(Boolean);
  parts.pop();
  return parts.length > 0 ? '/' + parts.join('/') : '/';
}
