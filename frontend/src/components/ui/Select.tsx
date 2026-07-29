import React, { forwardRef, useState, useRef, useEffect, useCallback, useMemo, ReactNode } from 'react';
import { ChevronDown } from 'lucide-react';

export interface SelectOption {
  value: string | number;
  label: string;
  disabled?: boolean;
}

interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'onChange' | 'value'> {
  label?: string;
  error?: string;
  options: SelectOption[];
  placeholder?: string;
  /** Клас для зовнішнього контейнера (div) */
  containerClassName?: string;
  value?: string | number;
  onChange?: (e: { target: { value: string } }) => void;
  /** Іконка зліва від тексту (наприклад, User, Calendar) */
  leftIcon?: ReactNode;
}

export const Select = forwardRef<HTMLButtonElement, SelectProps>(
  ({
    label,
    error,
    options,
    placeholder,
    className = '',
    containerClassName = '',
    value,
    onChange,
    disabled,
    required,
    name,
    id,
    leftIcon,
    ...props
  }, ref) => {
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const listRef = useRef<HTMLUListElement>(null);

    const selectedOption = useMemo(
      () => options.find((opt) => String(opt.value) === String(value)),
      [options, value]
    );

    const displayValue = selectedOption?.label || placeholder || '';

    // Закриття при кліку поза компонентом
    useEffect(() => {
      if (!isOpen) return;

      const handleClickOutside = (e: MouseEvent) => {
        if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
          setIsOpen(false);
        }
      };

      document.addEventListener('mousedown', handleClickOutside);
      return () => document.removeEventListener('mousedown', handleClickOutside);
    }, [isOpen]);

    // Закриття по Escape / Tab
    useEffect(() => {
      if (!isOpen) return;

      const handleKeyDown = (e: KeyboardEvent) => {
        if (e.key === 'Escape' || e.key === 'Tab') {
          setIsOpen(false);
          // Фокус на тригер при закритті
          containerRef.current?.querySelector('button')?.focus();
        }
      };

      document.addEventListener('keydown', handleKeyDown);
      return () => document.removeEventListener('keydown', handleKeyDown);
    }, [isOpen]);

    const handleSelect = useCallback(
      (option: SelectOption) => {
        if (option.disabled) return;
        setIsOpen(false);
        onChange?.({ target: { value: String(option.value) } });
      },
      [onChange]
    );

    const handleTriggerClick = useCallback(() => {
      if (disabled) return;
      setIsOpen((prev) => !prev);
    }, [disabled]);

    const handleTriggerKeyDown = useCallback(
      (e: React.KeyboardEvent) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          if (disabled) return;
          setIsOpen((prev) => !prev);
        }
      },
      [disabled]
    );

    const handleOptionKeyDown = useCallback(
      (e: React.KeyboardEvent, option: SelectOption) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          handleSelect(option);
        }
      },
      [handleSelect]
    );

    return (
      <div className={`w-full ${containerClassName}`} ref={containerRef}>
        {label && (
          <label
            htmlFor={id || name}
            className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5"
          >
            {label}
            {required && <span className="text-danger-500 ml-1">*</span>}
          </label>
        )}

        <div className="relative">
          <button
            ref={ref}
            id={id || name}
            type="button"
            role="combobox"
            aria-expanded={isOpen}
            aria-haspopup="listbox"
            aria-labelledby={label ? id || name : undefined}
            disabled={disabled}
            onClick={handleTriggerClick}
            onKeyDown={handleTriggerKeyDown}
            className={`
              input-field w-full flex items-center gap-2 px-3 py-2
              text-left cursor-default
              ${disabled ? 'opacity-50 cursor-not-allowed bg-gray-100 dark:bg-slate-700/50' : 'bg-white dark:bg-slate-800'}
              ${error ? 'border-danger-500 focus:ring-danger-500' : ''}
              ${!selectedOption && placeholder ? 'text-gray-400 dark:text-gray-500' : 'text-gray-900 dark:text-gray-100'}
              ${leftIcon ? 'pl-10' : ''}
              ${className}
            `}
            {...(props as any)}
          >
            {leftIcon && (
              <span className="absolute left-3 top-1/2 -translate-y-1/2 flex items-center pointer-events-none text-gray-400">
                {leftIcon}
              </span>
            )}
            <span className="flex-1 truncate">
              {displayValue || <span className="text-gray-400 dark:text-gray-500">&nbsp;</span>}
            </span>
            <ChevronDown
              className={`w-4 h-4 text-gray-400 flex-shrink-0 transition-transform duration-200 ${
                isOpen ? 'rotate-180' : ''
              }`}
            />
          </button>

          {/* Випадаючий список */}
          {isOpen && (
            <ul
              ref={listRef}
              role="listbox"
              aria-activedescendant={selectedOption ? `select-opt-${selectedOption.value}` : undefined}
              className="
                absolute z-50 mt-1 w-full
                bg-white dark:bg-slate-800
                border border-gray-300 dark:border-slate-600
                rounded-lg shadow-lg
                max-h-60 overflow-auto
                py-1
                animate-dropdown-in
              "
            >
              {options.length === 0 ? (
                <li className="px-3 py-2 text-sm text-gray-400 dark:text-gray-500 text-center">
                  Немає варіантів
                </li>
              ) : (
                options.map((option) => {
                  const isSelected = String(option.value) === String(value);
                  return (
                    <li
                      key={option.value}
                      id={`select-opt-${option.value}`}
                      role="option"
                      aria-selected={isSelected}
                      tabIndex={option.disabled ? -1 : 0}
                      onClick={() => handleSelect(option)}
                      onKeyDown={(e) => handleOptionKeyDown(e, option)}
                      className={`
                        flex items-center justify-between gap-2 px-3 py-2 text-sm cursor-pointer
                        transition-colors duration-100
                        ${
                          option.disabled
                            ? 'text-gray-300 dark:text-gray-600 cursor-not-allowed'
                            : isSelected
                            ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300 font-medium'
                            : 'text-gray-900 dark:text-gray-100 hover:bg-gray-100 dark:hover:bg-slate-700'
                        }
                      `}
                    >
                      <span className="truncate">{option.label}</span>
                      {isSelected && (
                        <svg
                          className="w-4 h-4 flex-shrink-0 text-primary-600 dark:text-primary-400"
                          fill="none"
                          viewBox="0 0 24 24"
                          stroke="currentColor"
                          strokeWidth={2}
                        >
                          <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                        </svg>
                      )}
                    </li>
                  );
                })
              )}
            </ul>
          )}
        </div>

        {error && <p className="mt-1 text-sm text-danger-600">{error}</p>}
      </div>
    );
  }
);

Select.displayName = 'Select';
