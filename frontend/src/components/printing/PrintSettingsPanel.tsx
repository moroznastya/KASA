import React, { useState, useEffect, useRef, useCallback } from 'react';
import { ChevronDown, Check, Star, Settings2, Loader2, Save, Pencil } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { settingsService } from '@/services/settingsService';
import { printTemplateService } from '@/services/printTemplateService';
import type { PrintTemplate } from '@/types/printTemplate';
import PrintTemplateEditorModal from '@/components/printing/PrintTemplateEditorModal';
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
  /** Ключ у system_settings для збереження */
  settingsKey: string;
}

const COMMON_FIELDS: FieldConfig[] = [
  { key: 'widthMm', label: 'Ширина, мм', default: 40, min: 10, max: 100, step: 1, unit: 'мм', settingsKey: 'width' },
  { key: 'heightMm', label: 'Висота, мм', default: 25, min: 10, max: 100, step: 1, unit: 'мм', settingsKey: 'height' },
  { key: 'gapMm', label: 'Відступ між, мм', default: 3, min: 0, max: 20, step: 0.5, unit: 'мм', settingsKey: 'gap' },
];

const PRICE_TAG_FIELDS: FieldConfig[] = [
  ...COMMON_FIELDS,
  { key: 'marginMm', label: 'Відступ краю, мм', default: 10, min: 0, max: 30, step: 1, unit: 'мм', settingsKey: 'margin' },
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
  const [isSavingSettings, setIsSavingSettings] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // ── Стан редактора шаблону ───────────────────
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorTemplate, setEditorTemplate] = useState<PrintTemplate | null>(null);

  const fields = type === 'price_tag' ? PRICE_TAG_FIELDS : LABEL_FIELDS;

  // Знайти поточний шаблон
  const selectedTemplate = templates.find((t) => t.id === templateId);

  // Префікс ключів у system_settings
  const settingsPrefix = type === 'price_tag' ? 'price_tag' : 'label';

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

  // ── Відкриття редактора шаблону ───────────────
  const handleOpenEditor = useCallback(() => {
    if (!selectedTemplate) return;
    setEditorTemplate(selectedTemplate);
    setEditorOpen(true);
  }, [selectedTemplate]);

  // ── Після збереження — оновлюємо список шаблонів ──
  const handleTemplateSaved = useCallback(() => {
    loadTemplates();
  }, []);

  /**
   * Завантажити налаштування з system_settings.
   *
   * Ключі (module='printing'):
   *   price_tag: price_tag_width, price_tag_height, price_tag_gap, price_tag_margin, price_tag_template_id
   *   label:     label_width, label_height, label_gap, label_template_id
   */
  const handleLoadSystemSettings = useCallback(async () => {
    setIsLoadingSettings(true);
    try {
      const values = await Promise.all(
        fields.map((f) =>
          settingsService
            .getValue(`${settingsPrefix}_${f.settingsKey}`)
            .then((v) => ({ field: f, value: v })),
        ),
      );

      // Застосовуємо знайдені значення
      values.forEach(({ field, value }) => {
        if (value !== null && value !== '') {
          const num = parseFloat(value);
          if (!isNaN(num)) {
            onChange(field.key, num);
          }
        }
      });

      // ── Шаблон: спочатку збережений template_id, інакше дефолтний ──
      const savedTemplateId = await settingsService.getValue(`${settingsPrefix}_template_id`);
      if (savedTemplateId) {
        // Перевіряємо, що шаблон існує у списку
        const exists = templates.some((t) => t.id === savedTemplateId);
        if (exists) {
          onChange('templateId', savedTemplateId);
        } else {
          // Шаблон не знайдено — підставляємо дефолтний
          const defaultTemplate = await printTemplateService.getDefault(type);
          if (defaultTemplate) onChange('templateId', defaultTemplate.id);
        }
      } else {
        // Порожній template_id — підставляємо дефолтний шаблон типу (is_default)
        const defaultTemplate = await printTemplateService.getDefault(type);
        if (defaultTemplate) onChange('templateId', defaultTemplate.id);
      }

      toast.success('Налаштування завантажено');
    } catch {
      toast.error('Помилка завантаження налаштувань');
    } finally {
      setIsLoadingSettings(false);
    }
  }, [fields, settingsPrefix, onChange, templates, type]);

  /**
   * Зберегти ВСІ поля (включно з gap/margin) у system_settings.
   *
   * Використовує settingsService.update (PUT /settings/{key}) —
   * тепер це UPSERT, тож ключі створюються автоматично.
   */
  const handleSaveSystemSettings = useCallback(async () => {
    setIsSavingSettings(true);
    try {
      // Зберігаємо всі поля розмірів
      await Promise.all(
        fields.map((f) => {
          const value = getValueByKey(f.key);
          return settingsService.update(
            `${settingsPrefix}_${f.settingsKey}`,
            String(value),
          );
        }),
      );

      // Зберігаємо вибраний шаблон
      if (templateId) {
        await settingsService.update(`${settingsPrefix}_template_id`, templateId);
      }

      toast.success('Налаштування збережено в системні');
    } catch {
      toast.error('Помилка збереження налаштувань');
    } finally {
      setIsSavingSettings(false);
    }
  }, [fields, settingsPrefix, templateId]);

  // Отримати значення поля
  function getValueByKey(key: string): number {
    switch (key) {
      case 'widthMm': return widthMm;
      case 'heightMm': return heightMm;
      case 'gapMm': return gapMm;
      case 'marginMm': return marginMm;
      default: return 0;
    }
  }

  // Отримати значення поля (для рендеру)
  const getValue = (key: string): number => getValueByKey(key);

  return (
    <div className="space-y-4">
      {/* Вибір шаблону */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
          Шаблон {type === 'price_tag' ? 'цінника' : 'етикетки'}
        </label>
        <div className="flex items-start gap-2">
          <div ref={dropdownRef} className="relative flex-1 min-w-0">
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

          {/* ✏️ Редагувати шаблон — видима коли вибрано шаблон */}
          <button
            type="button"
            onClick={handleOpenEditor}
            disabled={!selectedTemplate}
            title={selectedTemplate ? `Редагувати шаблон «${selectedTemplate.name}»` : 'Оберіть шаблон для редагування'}
            className="
              flex-shrink-0 px-3 py-2.5 rounded-lg
              border border-gray-300 dark:border-slate-600
              bg-white dark:bg-slate-800
              text-gray-600 dark:text-gray-300
              hover:bg-primary-50 dark:hover:bg-primary-900/20
              hover:text-primary-700 dark:hover:text-primary-400
              hover:border-primary-300 dark:hover:border-primary-700
              focus:outline-none focus:ring-2 focus:ring-primary-500
              transition-all duration-150
              disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-white dark:disabled:hover:bg-slate-800 disabled:hover:text-gray-600 dark:disabled:hover:text-gray-300 disabled:hover:border-gray-300 dark:disabled:hover:border-slate-600
            "
          >
            <Pencil className="w-4 h-4" />
          </button>
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

      {/* Кнопки: завантажити / зберегти системні налаштування */}
      <div className="grid grid-cols-2 gap-2">
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
        >
          {isLoadingSettings ? 'Завантаження...' : 'Завантажити'}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={handleSaveSystemSettings}
          disabled={isSavingSettings}
          icon={
            isSavingSettings ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Save className="w-4 h-4" />
            )
          }
        >
          {isSavingSettings ? 'Збереження...' : 'Зберегти'}
        </Button>
      </div>

      {/* ── Модальне вікно редактора шаблону ── */}
      <PrintTemplateEditorModal
        isOpen={editorOpen}
        onClose={() => setEditorOpen(false)}
        template={editorTemplate}
        onSaved={handleTemplateSaved}
      />
    </div>
  );
};

export default PrintSettingsPanel;
