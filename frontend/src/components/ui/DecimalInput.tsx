import React, { useEffect, useState } from 'react';
import { DECIMAL_INPUT_REGEX } from '@/utils/decimal';

interface DecimalInputProps {
  /** Числове значення з батьківського стану */
  value: number;
  /** Викликається при blur/Enter з розпарсеним числом */
  onCommit: (value: number) => void;
  className?: string;
  title?: string;
  placeholder?: string;
  /** Кількість знаків після коми при відображенні (не обмежує введення) */
  precision?: number;
  disabled?: boolean;
  autoFocus?: boolean;
}

/**
 * Контрольований input для дробових чисел (quantity / price / amount):
 *
 *  - type="text" + inputMode="decimal" → мобільна/екранна клавіатура з крапкою
 *  - дозволяє проміжні стани введення («», «.», «0.», «12.») — НЕ перезаписує
 *  - кома автоматично замінюється на крапку
 *  - число парситься ЛИШЕ при onCommit (blur / Enter)
 *  - onFocus → select() — вміст виділяється для швидкої заміни
 *  - синхронізація з зовнішнім значенням (перерахунки цін) — лише коли поле
 *    не у фокусі, щоб не «стрибав курсор»
 */
export const DecimalInput: React.FC<DecimalInputProps> = ({
  value,
  onCommit,
  className = '',
  title,
  placeholder,
  precision,
  disabled,
  autoFocus,
}) => {
  const [local, setLocal] = useState<string>(() =>
    precision !== undefined ? value.toFixed(precision) : String(value)
  );
  const [focused, setFocused] = useState(false);

  // Синхронізація з зовнішнім значенням (напр. перерахунок ціни) — тільки поза фокусом
  useEffect(() => {
    if (!focused) {
      setLocal(precision !== undefined ? value.toFixed(precision) : String(value));
    }
  }, [value, focused, precision]);

  const commit = () => {
    setFocused(false);
    const num = DECIMAL_INPUT_REGEX.test(local)
      ? parseFloat(local.replace(',', '.'))
      : NaN;
    const result = Number.isFinite(num) ? num : 0;
    onCommit(result);
    setLocal(precision !== undefined ? result.toFixed(precision) : String(result));
  };

  return (
    <input
      type="text"
      inputMode="decimal"
      value={local}
      disabled={disabled}
      autoFocus={autoFocus}
      onChange={(e) => {
        const normalized = e.target.value.replace(',', '.');
        if (DECIMAL_INPUT_REGEX.test(normalized)) {
          setLocal(normalized);
        }
      }}
      onFocus={(e) => {
        setFocused(true);
        e.target.select();
      }}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          e.currentTarget.blur();
        }
      }}
      className={className}
      title={title}
      placeholder={placeholder}
    />
  );
};
