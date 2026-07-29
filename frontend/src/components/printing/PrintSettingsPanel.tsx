import React, { useState, useEffect, useRef } from 'react';
import { ChevronDown, Check, Star, Settings2, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { settingsService } from '@/services/settingsService';
import { printTemplateService } from '@/services/printTemplateService';
import type { PrintTemplate } from '@/types/printTemplate';
import toast from 'react-hot-toast';

// ── Пропси ───────────────────────────────────────
interface PrintSettingsPanelProps {
  templateId: string;
  widthMm: number;
  heightMm: number;
  gapMm: number;
  marginMm: number;
  onChange: (field: string, value: string | number) => void;
  type: 'price_tag' | 'label';
}

// ── Конфігурація полів за типами ─────────────────
interface FieldConfig {
  key: string;
  label: string;
  default: number;
  min: number;
  max: number;
  step: number;
  unit: string;
}

const COMMON_FIELDS: FieldConfig[] = [
  { key: 'widthMm', label: 'Ширина, мм', default: 40, min: 10, max: 100, step: 1, unit: 'мм' },
  { key: 'heightMm', label: 'Висота, мм', default: 25, min: 10, max: 100, step: 1, unit: 'мм' },
  { key: 'gapMm', label: 'Відступ між, мм', default: 3, min: 0, max: 20, step: 0.5, unit: 'мм' },
];

const PRICE_TAG_FIELDS: FieldConfig[] = [
  ...COMMON_FIELDS,
  { key: 'marginMm', label: 'Відступ краю, мм', default: 10, min: 0, max: 30, step: 1, unit: 'мм' },
];

const LABEL_FIELDS: FieldConfig[] = [
  ...COMMON_FIELDS,
];

// ── Компонент ────────────────────────────────────
const PrintSettingsPanel: React.FC<PrintSettingsPanelProps> = ({
  templateId,
  widthMm,
  heightMm,
  gapMm,
  marginMm,
  onChange,
  type,
}) => {
  const [templates, setTemplates] = useState<PrintTemplate[]>([]);
  const [isLoadingTemplates, setIsLoadingTemplates] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [isLoadingSettings, setIsLoadingSettings] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const fields = type === 'price_tag' ? PRICE_TAG_FIELDS : LABEL_FIELDS;

  // Знайти поточний шаблон
  const selectedTemplate = templates.find((t) => t.id === templateId);

  // Завантажити шаблони відповідного типу
  useEffect(() => {
    loadTemplates();
  }, [type]);

  // Закриття dropdown
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const loadTemplates = async () => {
    setIsLoadingTemplates(true);
    try {
      const all = await printTemplateService.getAll();
      // Фільтруємо за типом: price_tag або label
      const filtered = all.filter(
        (t) => t.type === type || t.type === 'custom'
      );
      setTemplates(filtered);
    } catch {
      toast.error('Помилка завантаження шаблонів');
      setTemplates([]);
    } finally {
      setIsLoadingTemplates(false);
    }
  };

  // Завантажити налаштування з системних
  const handleLoadSystemSettings = async () => {
    setIsLoadingSettings(true);
    try {
      const prefix = type === 'price_tag' ? 'price_tag' : 'label';

      const widthStr = await settingsService.getValue(`${prefix}_width`);
      const heightStr = await settingsService.getValue(`${prefix}_height`);
      const gapStr = await settingsService.getValue(`${prefix}_gap`);
      const marginStr = type === 'price_tag'
        ? await settingsService.getValue('price_tag_margin')
        : null;

      if (widthStr) onChange('widthMm', parseFloat(widthStr));
      if (heightStr) onChange('heightMm', parseFloat(heightStr));
      if (gapStr) onChange('gapMm', parseFloat(gapStr));
      if (marginStr && type === 'price_tag') onChange('marginMm', parseFloat(marginStr));

      // Спробуємо завантажити шаблон за замовчуванням
      try {
        const defaultTemplate = await printTemplateService.getDefault(type);
        if (defaultTemplate) {
          onChange('templateId', defaultTemplate.id);
        }
      } catch {
        // Ігноруємо
      }

      toast.success('Налаштування завантажено');
    } catch {
      toast.error('Помилка завантаження налаштувань');
    } finally {
      setIsLoadingSettings(false);
    }
  };

  // Отримати значення поля
  const getValue = (key: string): number => {
    switch (key) {
      case 'widthMm': return widthMm;
      case 'heightMm': return heightMm;
      case 'gapMm': return gapMm;
      case 'marginMm': return marginMm;
      default: return 0;
    }
  };

  return (
    <div className="space-y-4">
      {/* Вибір шаблону */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
          Шаблон {type === 'price_tag' ? 'цінника' : 'етикетки'}
        </label>
        <div ref={dropdownRef} className="relative">
          <button
            type="button"
            onClick={() => setDropdownOpen(!dropdownOpen)}
            disabled={isLoadingTemplates}
            className="w-full flex items-center justify-between px-3 py-2.5 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-sm text-gray-900 dark:text-gray-100 hover:border-gray-400 dark:hover:border-slate-500 focus:outline-none focus:ring-2 focus:ring-primary-500 transition-all disabled:opacity-50"
          >
            <span className="truncate">
              {isLoadingTemplates
                ? 'Завантаження...'
                : selectedTemplate
                  ? selectedTemplate.name
                  : 'Оберіть шаблон...'}
            </span>
            <ChevronDown
              className={`w-4 h-4 text-gray-400 transition-transform flex-shrink-0 ml-2 ${
                dropdownOpen ? 'rotate-180' : ''
              }`}
            />
          </button>

          {dropdownOpen && (
            <div className="absolute z-50 w-full mt-1 bg-white dark:bg-slate-700 border border-gray-200 dark:border-slate-600 rounded-lg shadow-lg max-h-60 overflow-y-auto">
              {templates.length === 0 ? (
                <div className="px-4 py-3 text-sm text-gray-400 text-center">
                  Немає шаблонів типу «{type === 'price_tag' ? 'цінник' : 'етикетка'}»
                </div>
              ) : (
                templates.map((template) => (
                  <button
                    key={template.id}
                    onClick={() => {
                      onChange('templateId', template.id);
                      setDropdownOpen(false);
                    }}
                    className={`w-full px-4 py-2.5 text-left text-sm flex items-center justify-between hover:bg-gray-50 dark:hover:bg-slate-600 transition-colors ${
                      templateId === template.id
                        ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-400'
                        : 'text-gray-900 dark:text-gray-100'
                    }`}
                  >
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      {templateId === template.id && (
                        <Check className="w-4 h-4 flex-shrink-0 text-primary-600" />
                      )}
                      <span className="truncate">{template.name}</span>
                    </div>
                    {template.is_default && (
                      <Star className="w-3.5 h-3.5 text-amber-500 flex-shrink-0" />
                    )}
                  </button>
                ))
              )}
            </div>
          )}
        </div>
      </div>

      {/* Розміри */}
      <div className="space-y-3">
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
          Розміри
        </label>
        {fields.map((field) => (
          <div key={field.key}>
            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">
              {field.label}
            </label>
            <input
              type="number"
              min={field.min}
              max={field.max}
              step={field.step}
              value={getValue(field.key)}
              onChange={(e) => {
                const val = parseFloat(e.target.value);
                if (!isNaN(val)) {
                  onChange(field.key, val);
                }
              }}
              className="input-field w-full text-sm"
            />
          </div>
        ))}
      </div>

      {/* Кнопка завантаження налаштувань */}
      <Button
        variant="secondary"
        size="sm"
        onClick={handleLoadSystemSettings}
        disabled={isLoadingSettings}
        icon={
          isLoadingSettings ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <Settings2 className="w-4 h-4" />
          )
        }
        className="w-full"
      >
        {isLoadingSettings ? 'Завантаження...' : 'Завантажити з системних налаштувань'}
      </Button>
    </div>
  );
};

export default PrintSettingsPanel;
