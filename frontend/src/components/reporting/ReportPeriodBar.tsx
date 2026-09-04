import React from 'react';
import { Input } from '@/components/ui/Input';
import {
  REPORT_PERIOD_LABELS,
  ReportPeriod,
} from '@/utils/reportPeriod';

interface ReportPeriodBarProps {
  period: ReportPeriod;
  onPeriodChange: (p: ReportPeriod) => void;
  customFrom: string;
  customTo: string;
  onCustomFromChange: (v: string) => void;
  onCustomToChange: (v: string) => void;
}

/**
 * Селектор періоду звітів (Сьогодні / Тиждень / Місяць / Період + дати),
 * спільний для «Дашборд мережі» та «Фінанси мережі» (Етап 4, ТЗ 5.5/5.6).
 */
export const ReportPeriodBar: React.FC<ReportPeriodBarProps> = ({
  period,
  onPeriodChange,
  customFrom,
  customTo,
  onCustomFromChange,
  onCustomToChange,
}) => {
  const handleClick = (value: ReportPeriod) => {
    onPeriodChange(value);
    if (value !== 'custom') {
      onCustomFromChange('');
      onCustomToChange('');
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      {REPORT_PERIOD_LABELS.map((btn) => (
        <button
          key={btn.value}
          onClick={() => handleClick(btn.value)}
          className={`
            px-4 py-2 rounded-lg text-sm font-medium transition-colors
            ${
              period === btn.value
                ? 'bg-primary-600 text-white'
                : 'bg-white dark:bg-slate-800 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-slate-700 border border-gray-200 dark:border-slate-700'
            }
          `}
        >
          {btn.label}
        </button>
      ))}

      {period === 'custom' && (
        <div className="flex items-center gap-2 ml-2">
          <div className="w-40">
            <Input
              type="date"
              value={customFrom}
              onChange={(e) => onCustomFromChange(e.target.value)}
              placeholder="Від"
            />
          </div>
          <span className="text-gray-400">—</span>
          <div className="w-40">
            <Input
              type="date"
              value={customTo}
              onChange={(e) => onCustomToChange(e.target.value)}
              placeholder="До"
            />
          </div>
        </div>
      )}
    </div>
  );
};
