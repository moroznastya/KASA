import React, { useEffect, useCallback, useRef, useId } from 'react';
import { X } from 'lucide-react';

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | '3xl' | '4xl';
  showCloseButton?: boolean;
}

const sizeClasses: Record<string, string> = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-lg',
  xl: 'max-w-xl',
  '2xl': 'max-w-2xl',
  '3xl': 'max-w-3xl',
  '4xl': 'max-w-4xl',
};

/** Селектор фокусованих елементів (кнопки, посилання, поля, tabindex) */
const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export const Modal: React.FC<ModalProps> = ({
  isOpen,
  onClose,
  title,
  children,
  size = 'md',
  showCloseButton = true,
}) => {
  const modalRef = useRef<HTMLDivElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const titleId = useId();

  /**
   * Accessibility (a11y):
   *  - Esc закриває модалку
   *  - Focus trap: Tab/Shift+Tab не виходять за межі діалогу
   *  - Фокус переміщується всередину при відкритті та повертається
   *    до елемента-тригера при закритті
   */
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const modal = modalRef.current;
      if (!modal) return;

      // Подія з вкладеного діалогу (напр. ConfirmDialog поверх Modal) —
      // не втручаємось, вкладений обробить сам
      const targetIsInModal =
        e.target instanceof HTMLElement && e.target.closest('[role="dialog"]') === modal;

      if (e.key === 'Escape') {
        if (!targetIsInModal) return;
        e.stopPropagation();
        onClose();
        return;
      }

      if (e.key !== 'Tab') return;

      const focusables = Array.from(modal.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
      if (focusables.length === 0) return;

      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement as HTMLElement | null;

      // Фокус поза модалкою (наприклад, клік по фону) → повертаємо всередину
      if (!modal.contains(active)) {
        e.preventDefault();
        first.focus();
        return;
      }

      // Циклічний Tab у межах діалогу
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    },
    [onClose]
  );

  useEffect(() => {
    if (!isOpen) return;

    // Запам'ятовуємо елемент, який відкрив модалку
    previouslyFocusedRef.current = document.activeElement as HTMLElement | null;

    document.addEventListener('keydown', handleKeyDown);
    document.body.style.overflow = 'hidden';

    // Переміщуємо фокус у модалку
    const modal = modalRef.current;
    if (modal) {
      const firstFocusable = modal.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
      if (firstFocusable) {
        firstFocusable.focus();
      } else {
        modal.focus();
      }
    }

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = 'unset';
      // Повертаємо фокус елементу-тригеру
      previouslyFocusedRef.current?.focus?.();
      previouslyFocusedRef.current = null;
    };
  }, [isOpen, handleKeyDown]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Modal content */}
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={`
          relative w-full ${sizeClasses[size]}
          bg-white dark:bg-slate-800
          rounded-2xl shadow-xl
          transform transition-all duration-200
          animate-in fade-in zoom-in-95
          outline-none
        `}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-slate-700">
          <h2 id={titleId} className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {title}
          </h2>
          {showCloseButton && (
            <button
              onClick={onClose}
              aria-label="Закрити діалог"
              title="Закрити (Esc)"
              className="p-1 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary-500"
            >
              <X className="w-5 h-5" aria-hidden="true" />
            </button>
          )}
        </div>

        {/* Body */}
        <div className="px-6 py-4 max-h-[85vh] overflow-y-auto">{children}</div>
      </div>
    </div>
  );
};
