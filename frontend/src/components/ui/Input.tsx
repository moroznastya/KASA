import React, { forwardRef } from 'react';

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
  helperText?: string;
  icon?: React.ReactNode;
  inputClassName?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ label, error, helperText, icon, className = '', inputClassName = '', value, onChange, placeholder, id, ...props }, ref) => {
    return (
      <div className="w-full">
        {label && (
          <label htmlFor={id} className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
            {label}
          </label>
        )}
        <div className="relative">
          {icon && (
            <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none text-gray-400">
              {icon}
            </div>
          )}
          <input
            ref={ref}
            id={id}
            value={value}
            onChange={onChange}
            placeholder={placeholder}
            onWheel={
              props.type === 'number'
                ? (e) => (e.target as HTMLInputElement).blur()
                : undefined
            }
            aria-invalid={error ? true : undefined}
            aria-describedby={
              error ? (id ? `${id}-error` : undefined) : helperText ? (id ? `${id}-helper` : undefined) : undefined
            }
            className={`
              input-field
              [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none
              ${icon ? 'pl-10' : 'px-3'}
              ${error ? 'border-danger-500 focus:ring-danger-500' : ''}
              ${className}
              ${inputClassName}
            `}
            {...props}
          />
        </div>
        {error && <p id={id ? `${id}-error` : undefined} className="mt-1 text-sm text-danger-600">{error}</p>}
        {helperText && !error && (
          <p id={id ? `${id}-helper` : undefined} className="mt-1 text-sm text-gray-500 dark:text-gray-400">{helperText}</p>
        )}
      </div>
    );
  }
);

Input.displayName = 'Input';
