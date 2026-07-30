import React, { useEffect, useState, useRef } from 'react';
import { Printer, ChevronDown, Check } from 'lucide-react';
import { isTauri } from '@/hooks/useTauri';
import { getPrinters } from '@/services/tauri/print';

// ── Пропси ───────────────────────────────────────
interface PrinterSelectorProps {
  value: string;
  onChange: (printerName: string) => void;
}

// ── Компонент ────────────────────────────────────
const PrinterSelector: React.FC<PrinterSelectorProps> = ({ value, onChange }) => {
  const [printers, setPrinters] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const loadPrinters = async () => {
      if (!isTauri()) return;
      setIsLoading(true);
      try {
        const list = await getPrinters();
        setPrinters(list);
      } catch {
        setPrinters([]);
      } finally {
        setIsLoading(false);
      }
    };
    loadPrinters();
  }, []);

  // Закриття при кліку назовні
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const options = isTauri()
    ? [{ value: '', label: isLoading ? 'Завантаження...' : 'Автоматично (системний)' }]
        .concat(printers.map((p) => ({ value: p, label: p })))
    : [{ value: '', label: 'Системний принтер (браузер)' }];

  const selectedLabel = options.find((o) => o.value === value)?.label || 'Оберіть принтер...';

  return (
    <div className="space-y-1.5">
      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
        Принтер
      </label>
      <div ref={dropdownRef} className="relative">
        <button
          type="button"
          onClick={() => !isLoading && setDropdownOpen(!dropdownOpen)}
          disabled={isLoading}
          className="w-full flex items-center justify-between px-3 py-2.5 rounded-lg
            border border-gray-300 dark:border-slate-600
            bg-white dark:bg-slate-800
            text-sm text-gray-900 dark:text-gray-100
            hover:border-gray-400 dark:hover:border-slate-500
            focus:outline-none focus:ring-2 focus:ring-primary-500
            transition-all disabled:opacity-50"
        >
          <span className="flex items-center gap-2 truncate">
            <Printer className="w-4 h-4 text-gray-400 flex-shrink-0" />
            <span className="truncate">{isLoading ? 'Завантаження...' : selectedLabel}</span>
          </span>
          <ChevronDown
            className={`w-4 h-4 text-gray-400 transition-transform flex-shrink-0 ml-2 ${
              dropdownOpen ? 'rotate-180' : ''
            }`}
          />
        </button>

        {dropdownOpen && (
          <div
            className="absolute z-50 w-full mt-1
              bg-white dark:bg-slate-700
              border border-gray-200 dark:border-slate-600
              rounded-lg shadow-lg max-h-60 overflow-y-auto"
          >
            {options.map((opt) => (
              <button
                key={opt.value}
                onClick={() => {
                  onChange(opt.value);
                  setDropdownOpen(false);
                }}
                className={`w-full px-4 py-2.5 text-left text-sm flex items-center gap-2
                  hover:bg-gray-50 dark:hover:bg-slate-600 transition-colors
                  ${
                    value === opt.value
                      ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-400'
                      : 'text-gray-900 dark:text-gray-100'
                  }`}
              >
                {value === opt.value && (
                  <Check className="w-4 h-4 flex-shrink-0 text-primary-600" />
                )}
                <span className={value === opt.value ? '' : 'ml-6'}>{opt.label}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      {isTauri() && value && (
        <p className="text-xs text-gray-400 dark:text-gray-500">
          Вибрано: {value}
        </p>
      )}
    </div>
  );
};

export default PrinterSelector;
