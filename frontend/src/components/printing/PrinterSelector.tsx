import React, { useEffect, useState } from 'react';
import { Printer } from 'lucide-react';
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

  return (
    <div className="space-y-1.5">
      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
        Принтер
      </label>
      <div className="relative">
        <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
          <Printer className="w-4 h-4 text-gray-400" />
        </div>
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={isLoading}
          className="w-full pl-9 pr-3 py-2 text-sm border border-gray-300 dark:border-slate-600 rounded-lg
            bg-white dark:bg-slate-700 text-gray-900 dark:text-gray-100
            focus:ring-2 focus:ring-primary-500 focus:border-primary-500
            disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isTauri() ? (
            <>
              <option value="">{isLoading ? 'Завантаження...' : 'Автоматично (системний)'}</option>
              {printers.map((printer) => (
                <option key={printer} value={printer}>{printer}</option>
              ))}
            </>
          ) : (
            <>
              <option value="">Системний принтер (браузер)</option>
            </>
          )}
        </select>
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
