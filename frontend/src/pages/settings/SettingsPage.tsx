import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import api from '@/services/api';
import { Button } from '@/components/ui/Button';
import { Select, SelectOption } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { toast } from 'react-hot-toast';
import {PRICE_TAG_FIELD_OPTIONS} from '@/types/printTemplate';
import {
  Building2,
  ShoppingCart,
  Printer,
  Percent,
  Bell,
  Shield,
  Database,
  Link2,
  Save,
  CheckCircle2,
  FileText,
  Tag,
  Sticker,
  Ruler,
  Eye,
  Loader2,
  Monitor,
  Store,
  Rocket,
  RefreshCw,
  Download,
  Plug,
} from 'lucide-react';
import { isTauri } from '@/hooks/useTauri';
import { getPrinters } from '@/services/tauri/print';
import {
  enable as enableAutostart,
  disable as disableAutostart,
  isEnabled as isAutostartEnabled,
} from '@tauri-apps/plugin-autostart';
import { useUpdater } from '@/hooks/useUpdater';

// ── Типи ──────────────────────────────────────
interface SystemSetting {
  id: string;
  module: string;
  key: string;
  value: string | null;
  value_type: string;
  label: string;
  description: string | null;
  options: string | null;
  is_active: boolean;
}

interface SettingsData {
  modules: Record<string, SystemSetting[]>;
}

// ── Мапа модулів ──────────────────────────────
const MODULE_CONFIG: Record<string, { icon: React.ReactNode; label: string; description: string }> = {
  general: {
    icon: <Building2 className="w-5 h-5" />,
    label: 'Загальні',
    description: 'Інформація про підприємство',
  },
  pos: {
    icon: <ShoppingCart className="w-5 h-5" />,
    label: 'Каса (POS)',
    description: 'Налаштування робочого місця касира',
  },
  printing: {
    icon: <Printer className="w-5 h-5" />,
    label: 'Друк',
    description: 'Принтери, шаблони чеків, цінники та етикетки',
  },
  pricing: {
    icon: <Percent className="w-5 h-5" />,
    label: 'Ціноутворення',
    description: 'Правила розрахунку цін',
  },
  notifications: {
    icon: <Bell className="w-5 h-5" />,
    label: 'Сповіщення',
    description: 'Ліміти та попередження',
  },
  integrations: {
    icon: <Link2 className="w-5 h-5" />,
    label: 'Інтеграції',
    description: 'Підключення зовнішніх сервісів',
  },
  security: {
    icon: <Shield className="w-5 h-5" />,
    label: 'Безпека',
    description: 'Політики доступу та паролі',
  },
  backup: {
    icon: <Database className="w-5 h-5" />,
    label: 'Резервування',
    description: 'Автоматичне резервне копіювання',
  },
};

// ── Мапа зрозумілих назв для опцій ────────────
const ROUNDING_LABELS: Record<string, string> = {
  '1': '1 коп (без заокруглення)',
  '10': '10 коп',
  '50': '50 коп',
  '100': '1 грн',
  '500': '5 грн',
};

// ── Компонент поля налаштування ──────────────
const SettingField: React.FC<{
  setting: SystemSetting;
  value: string;
  onChange: (key: string, value: string) => void;
}> = ({ setting, value, onChange }) => {
  const handleChange = (newValue: string) => {
    onChange(setting.key, newValue);
  };

  // Select field
  if (setting.value_type === 'select' && setting.options) {
    let options: SelectOption[] = [];
    try {
      const parsed = JSON.parse(setting.options);
      options = parsed.map((opt: string) => {
        // Красиві мітки для типів
        const typeLabels: Record<string, string> = {
          receipt_58mm: 'Чек 58 мм',
          receipt_80mm: 'Чек 80 мм',
          fiscal: 'Фіскальний',
          custom: 'Кастомний',
          price_tag: 'Цінник',
          label: 'Етикетка',
        };
        return {
          value: opt,
          label: typeLabels[opt] || ROUNDING_LABELS[opt] || opt + (opt === '1' ? ' коп' : ' коп'),
        };
      });
    } catch {
      options = [{ value: setting.options, label: setting.options }];
    }
    return (
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
          {setting.label}
        </label>
        {setting.description && (
          <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
            {setting.description}
          </p>
        )}
        <Select
          options={options}
          value={value}
          onChange={(e) => handleChange(e.target.value)}
        />
      </div>
    );
  }

  // Boolean toggle
  if (setting.value_type === 'boolean') {
    return (
      <div className="flex items-center justify-between">
        <div>
          <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
            {setting.label}
          </label>
          {setting.description && (
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {setting.description}
            </p>
          )}
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={value === 'true'}
          onClick={() => handleChange(value === 'true' ? 'false' : 'true')}
          className={`
            relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full
            border-2 border-transparent transition-colors duration-200 ease-in-out
            focus:outline-none focus:ring-2 focus:ring-primary-500 focus:ring-offset-2
            ${value === 'true' ? 'bg-primary-600' : 'bg-gray-200 dark:bg-slate-600'}
          `}
        >
          <span
            className={`
              pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow
              ring-0 transition duration-200 ease-in-out
              ${value === 'true' ? 'translate-x-5' : 'translate-x-0'}
            `}
          />
        </button>
      </div>
    );
  }

  // Text / number input
  return (
    <div>
      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
        {setting.label}
      </label>
      {setting.description && (
        <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
          {setting.description}
        </p>
      )}
      <input
        type={setting.value_type === 'number' ? 'number' : 'text'}
        value={value}
        onChange={(e) => handleChange(e.target.value)}
        className="block w-full rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 px-3 py-2 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:border-primary-500 focus:ring-1 focus:ring-primary-500"
        placeholder={`Введіть ${setting.label.toLowerCase()}`}
      />
    </div>
  );
};

// ── Компонент: вибір полів для цінника/етикетки ──
const FieldsSelector: React.FC<{
  label: string;
  description: string;
  selectedFields: string[];
  onChange: (fields: string[]) => void;
}> = ({ label, description, selectedFields, onChange }) => {
  const toggleField = (field: string) => {
    if (selectedFields.includes(field)) {
      onChange(selectedFields.filter(f => f !== field));
    } else {
      onChange([...selectedFields, field]);
    }
  };

  return (
    <div>
      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
        {label}
      </label>
      <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
        {description}
      </p>
      <div className="flex flex-wrap gap-2">
        {PRICE_TAG_FIELD_OPTIONS.map((opt) => (
          <button
            key={opt.value}
            type="button"
            onClick={() => toggleField(opt.value)}
            className={`
              inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium border transition-all
              ${
                selectedFields.includes(opt.value)
                  ? 'bg-primary-50 border-primary-300 text-primary-700 dark:bg-primary-900/30 dark:border-primary-700 dark:text-primary-300'
                  : 'bg-white dark:bg-slate-800 border-gray-200 dark:border-slate-600 text-gray-600 dark:text-gray-400 hover:border-gray-300 dark:hover:border-slate-500'
              }
            `}
          >
            {selectedFields.includes(opt.value) && (
              <CheckCircle2 className="w-3.5 h-3.5" />
            )}
            {opt.label}
          </button>
        ))}
      </div>
    </div>
  );
};

// ── Компонент: розміри цінника/етикетки ──────
const SizeInputs: React.FC<{
  widthKey: string;
  heightKey: string;
  widthValue: string;
  heightValue: string;
  label: string;
  onFieldChange: (key: string, value: string) => void;
}> = ({ widthKey, heightKey, widthValue, heightValue, label, onFieldChange }) => {
  return (
    <div>
      <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
        Розмір {label}
      </label>
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-1.5">
          <Ruler className="w-4 h-4 text-gray-400" />
          <input
            type="number"
            value={widthValue}
            onChange={(e) => onFieldChange(widthKey, e.target.value)}
            className="w-20 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 px-3 py-2 text-sm text-gray-900 dark:text-gray-100 text-center focus:border-primary-500 focus:ring-1 focus:ring-primary-500"
            min="10"
            max="200"
          />
          <span className="text-sm text-gray-500">×</span>
          <input
            type="number"
            value={heightValue}
            onChange={(e) => onFieldChange(heightKey, e.target.value)}
            className="w-20 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 px-3 py-2 text-sm text-gray-900 dark:text-gray-100 text-center focus:border-primary-500 focus:ring-1 focus:ring-primary-500"
            min="10"
            max="200"
          />
          <span className="text-sm text-gray-500">мм</span>
        </div>
      </div>
    </div>
  );
};

// ── Компонент: Desktop-налаштування (Tauri) ──
// Тільки в десктоп-обгортці: автозапуск, single-instance, автооновлення.
const DesktopSettingsCard: React.FC = () => {
  const [autostartEnabled, setAutostartEnabled] = useState<boolean>(false);
  const [autostartLoading, setAutostartLoading] = useState<boolean>(true);
  const { checking, installing, available, checkForUpdates, install } = useUpdater();

  // Поточний стан автозапуску (лише у Tauri-режимі)
  useEffect(() => {
    if (!isTauri()) return;
    let mounted = true;
    (async () => {
      try {
        const enabled = await isAutostartEnabled();
        if (mounted) setAutostartEnabled(enabled);
      } catch (err) {
        console.error('Не вдалося отримати стан автозапуску:', err);
      } finally {
        if (mounted) setAutostartLoading(false);
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  const handleAutostartToggle = async (next: boolean) => {
    setAutostartLoading(true);
    try {
      if (next) {
        await enableAutostart();
      } else {
        await disableAutostart();
      }
      setAutostartEnabled(next);
      toast.success(next ? 'Автозапуск увімкнено' : 'Автозапуск вимкнено');
    } catch (err) {
      console.error('Помилка зміни автозапуску:', err);
      toast.error('Помилка зміни автозапуску');
    } finally {
      setAutostartLoading(false);
    }
  };

  const handleCheckUpdates = async () => {
    const info = await checkForUpdates();
    if (info) {
      toast.success(`Доступна версія ${info.version}`);
    } else {
      toast('Оновлень немає — у вас актуальна версія');
    }
  };

  // У браузерній версії налаштування десктоп-обгортки не показуємо
  if (!isTauri()) return null;

  return (
    <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
      {/* Заголовок */}
      <div className="px-6 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
          <Monitor className="w-5 h-5" />
        </div>
        <div className="flex-1">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Робоче місце (Desktop)
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Налаштування десктоп-обгортки POS
          </p>
        </div>
      </div>

      {/* Поля */}
      <div className="px-6 py-4 space-y-5">
        {/* ── Автозапуск ──────────── */}
        <div className="flex items-center justify-between">
          <div className="pr-4">
            <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
              Автозапуск при вході в систему
            </label>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              POS-каса запускатиметься автоматично разом із системою
            </p>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={autostartEnabled}
            disabled={autostartLoading}
            onClick={() => handleAutostartToggle(!autostartEnabled)}
            className={`
              relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full
              border-2 border-transparent transition-colors duration-200 ease-in-out
              focus:outline-none focus:ring-2 focus:ring-primary-500 focus:ring-offset-2
              disabled:opacity-50 disabled:cursor-not-allowed
              ${autostartEnabled ? 'bg-primary-600' : 'bg-gray-200 dark:bg-slate-600'}
            `}
          >
            <span
              className={`
                pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow
                ring-0 transition duration-200 ease-in-out
                ${autostartEnabled ? 'translate-x-5' : 'translate-x-0'}
              `}
            />
          </button>
        </div>

        {/* ── Автооновлення ──────────── */}
        <hr className="border-gray-200 dark:border-slate-700" />
        <div className="flex items-center justify-between">
          <div className="pr-4">
            <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
              Автооновлення
            </label>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {available
                ? `Доступна версія ${available.version} (поточна ${available.currentVersion})`
                : 'Перевірка та встановлення нових версій застосунку'}
            </p>
          </div>
          {available ? (
            <Button
              variant="primary"
              size="sm"
              disabled={installing}
              onClick={() => install()}
              icon={installing ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Download className="w-3.5 h-3.5" />}
            >
              {installing ? 'Встановлення...' : 'Встановити та перезапустити'}
            </Button>
          ) : (
            <Button
              variant="secondary"
              size="sm"
              disabled={checking}
              onClick={handleCheckUpdates}
              icon={checking ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
            >
              {checking ? 'Перевірка...' : 'Перевірити оновлення'}
            </Button>
          )}
        </div>

        {/* ── Інфо про single-instance ──────────── */}
        <p className="text-xs text-gray-500 dark:text-gray-400 flex items-start gap-1.5">
          <Rocket className="w-3.5 h-3.5 mt-0.5 flex-shrink-0" />
          Захист від подвійного запуску: при повторному відкритті програми
          фокусується вже запущена каса.
        </p>
      </div>
    </div>
  );
};

// ── Секція модуля ─────────────────────────────
const ModuleSection: React.FC<{
  moduleKey: string;
  settings: SystemSetting[];
  values: Record<string, string>;
  onFieldChange: (key: string, value: string) => void;
  onNavigate: (path: string) => void;
}> = ({ moduleKey, settings, values, onFieldChange, onNavigate }) => {
  const config = MODULE_CONFIG[moduleKey];

  // Розділяємо налаштування на звичайні та специфічні для цінників/етикеток
  const regularSettings = settings.filter(s =>
    !['price_tag_fields', 'price_tag_width', 'price_tag_height',
      'label_fields', 'label_width', 'label_height',
      'printer_name', 'print_copies', 'return_receipt_template_type',
      'receipt_print_copies', 'report_print_copies'].includes(s.key)
    && !s.key.startsWith('print_font_')
  );
  const priceTagFields = settings.find(s => s.key === 'price_tag_fields');
  const priceTagWidth = settings.find(s => s.key === 'price_tag_width');
  const priceTagHeight = settings.find(s => s.key === 'price_tag_height');
  const labelFields = settings.find(s => s.key === 'label_fields');
  const labelWidth = settings.find(s => s.key === 'label_width');
  const labelHeight = settings.find(s => s.key === 'label_height');

  // Парсимо JSON-поля
  const parseJsonArray = (val: string | null | undefined): string[] => {
    if (!val) return [];
    try {
      return JSON.parse(val);
    } catch {
      return val ? val.split(',').map(s => s.trim()) : [];
    }
  };

  const selectedPriceTagFields = parseJsonArray(values.price_tag_fields);
  const selectedLabelFields = parseJsonArray(values.label_fields);
  const [previewReceiptHtml, setPreviewReceiptHtml] = useState<string | null>(null);
  // ── Стан для попереднього перегляду цінника/етикетки ──
  const [previewTagLabelHtml, setPreviewTagLabelHtml] = useState<string | null>(null);
  const [previewTagLabelType, setPreviewTagLabelType] = useState<'price_tag' | 'label'>('price_tag');
  const [isTestLoading, setIsTestLoading] = useState<'price_tag' | 'label' | null>(null);

  // ── Реальний список принтерів системи ──
  const [printers, setPrinters] = useState<string[]>([]);
  const [printersLoading, setPrintersLoading] = useState<boolean>(false);

  // Завантаження реальних принтерів:
  //  1) Tauri (desktop) → invoke get_printers (нативний список)
  //  2) Браузер → GET /api/v1/print/printers (повертає { printers: [...] })
  useEffect(() => {
    let cancelled = false;
    const loadPrinters = async () => {
      setPrintersLoading(true);
      try {
        let list: string[] = [];
        if (isTauri()) {
          list = await getPrinters();
        } else {
          const res = await api.get('/print/printers');
          list = res.data?.printers ?? [];
        }
        if (!cancelled) setPrinters(Array.isArray(list) ? list : []);
      } catch {
        // Помилка отримання списку — лишаємо порожнім (системний + ручний ввід)
        if (!cancelled) setPrinters([]);
      } finally {
        if (!cancelled) setPrintersLoading(false);
      }
    };
    void loadPrinters();
    return () => {
      cancelled = true;
    };
  }, []);

  // Об'єднуємо реальний список + збережене значення (щоб вибір не губився)
  const printerOptions = useMemo(() => {
    const current = values.printer_name || '';
    const merged = [...printers];
    if (current && current !== 'custom' && !merged.includes(current)) {
      merged.push(current);
    }
    return merged;
  }, [printers, values.printer_name]);

  // ── Тестовий друк / прев'ю цінника чи етикетки ──
  const handleTestPrintPreview = useCallback(async (testType: 'price_tag' | 'label') => {
    const prefix = testType;
    // Поточні налаштування з форми
    const width = parseFloat(values[`${prefix}_width`] || (testType === 'price_tag' ? '40' : '60'));
    const height = parseFloat(values[`${prefix}_height`] || (testType === 'price_tag' ? '25' : '40'));
    const gap = parseFloat(values[`${prefix}_gap`] || '3');
    const margin = testType === 'price_tag'
      ? parseFloat(values.price_tag_margin || '10')
      : 0;

    setIsTestLoading(testType);
    setPreviewTagLabelHtml(null);
    try {
      const res = await api.post('/print/test', {
        print_type: testType,
        width_mm: isNaN(width) ? 40 : width,
        height_mm: isNaN(height) ? 25 : height,
        gap_mm: isNaN(gap) ? 3 : gap,
        margin_mm: isNaN(margin) ? 10 : margin,
        // Порожній template_id → сервер візьме шаблон is_default
        template_id: values[`${prefix}_template_id`] || '',
      });

      const previewHtml = res.data?.preview_html;
      if (previewHtml) {
        setPreviewTagLabelHtml(previewHtml);
        setPreviewTagLabelType(testType);
        toast.success(`Прев'ю ${testType === 'price_tag' ? 'цінника' : 'етикетки'} згенеровано`);
      } else {
        // Якщо сервер одразу надрукував — просто підтверджуємо
        toast.success(`Тестовий друк ${testType === 'price_tag' ? 'цінника' : 'етикетки'} виконано`);
      }
    } catch (err: any) {
      toast.error(
        err?.response?.data?.detail ||
        `Помилка генерації ${testType === 'price_tag' ? 'цінника' : 'етикетки'}`,
      );
    } finally {
      setIsTestLoading(null);
    }
  }, [values]);

  // Опції вибору принтера для кастомного Select
  const printerOptionsArr: SelectOption[] = [
    { value: '', label: '— Системний за замовчуванням —' },
    ...printerOptions.map(p => ({ value: p, label: p })),
  ];
  if (printersLoading) {
    printerOptionsArr.push({
      value: '__loading__',
      label: 'Завантаження списку принтерів…',
      disabled: true,
    });
  }
  printerOptionsArr.push({ value: 'custom', label: '🔧 Інший (ввести вручну)...' });

  if (!config) return null;

  return (
    <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
      {/* Заголовок модуля */}
      <div className="px-6 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
          {config.icon}
        </div>
        <div className="flex-1">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {config.label}
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {config.description}
          </p>
        </div>
        {moduleKey === 'printing' && (
          <button
            type="button"
            onClick={() => onNavigate('/settings/print-templates')}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium
              bg-primary-50 text-primary-700 hover:bg-primary-100
              dark:bg-primary-900/20 dark:text-primary-400 dark:hover:bg-primary-900/30
              transition-colors"
          >
            <FileText className="w-4 h-4" />
            Шаблони
          </button>
        )}
        {moduleKey === 'general' && (
          <button
            type="button"
            onClick={() => onNavigate('/settings/stores')}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium
              bg-primary-50 text-primary-700 hover:bg-primary-100
              dark:bg-primary-900/20 dark:text-primary-400 dark:hover:bg-primary-900/30
              transition-colors"
          >
            <Store className="w-4 h-4" />
            Торгові точки
          </button>
        )}
      </div>

      {/* Поля налаштувань */}
      <div className="px-6 py-4 space-y-5">
        {/* Звичайні налаштування друку */}
        {regularSettings.map((setting) => (
          <SettingField
            key={setting.key}
            setting={setting}
            value={values[setting.key] ?? setting.value ?? ''}
            onChange={onFieldChange}
          />
        ))}

        {/* ── 🖨️ Тестовий друк ──────────── */}
        <button
          type="button"
          onClick={async () => {
            try {
              await api.post('/print/test', {
                printer_name: values.printer_name,
                template_type: values.default_template_type,
              });
              toast.success('Тестовий друк виконано');
            } catch {
              toast.error('Помилка тестового друку');
            }
          }}
          className="w-full flex items-center justify-center gap-2 px-4 py-3 rounded-xl
            font-semibold text-sm transition-all duration-200 shadow-sm
            bg-primary-500 hover:bg-primary-600 text-white"
        >
          <Printer className="w-5 h-5" />
          Тестовий друк
        </button>

        {/* ── 📋 Вибір принтера ──────────── */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
            Принтер за замовчуванням
          </label>
          <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
            Виберіть принтер зі списку або введіть назву вручну
          </p>
          <Select
            options={printerOptionsArr}
            value={values.printer_name || ''}
            onChange={(e) => onFieldChange('printer_name', e.target.value)}
          />
          {values.printer_name === 'custom' && (
            <input
              type="text"
              value={(values.printer_name_custom || '')}
              onChange={(e) => onFieldChange('printer_name', e.target.value)}
              placeholder="Введіть назву принтера"
              className="mt-2 w-full rounded-lg border border-gray-300 dark:border-slate-600 
                bg-white dark:bg-slate-800 px-3 py-2 text-sm
                text-gray-900 dark:text-gray-100
                focus:border-primary-500 focus:ring-1 focus:ring-primary-500"
            />
          )}
        </div>

        {/* ── 🔄 Тип чеку повернення ──────────── */}
        {(() => {
          const returnSetting = settings.find(s => s.key === 'return_receipt_template_type');
          if (!returnSetting) return null;
          const returnOptions = [
            { value: 'receipt_58mm', label: 'Чек 58 мм' },
            { value: 'receipt_80mm', label: 'Чек 80 мм' },
            { value: 'return_receipt_58mm', label: 'Чек повернення 58 мм' },
            { value: 'fiscal', label: 'Фіскальний' },
            { value: 'custom', label: 'Кастомний' },
          ];
          return (
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                {returnSetting.label}
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
                {returnSetting.description}
              </p>
              <Select
                options={returnOptions}
                value={values.return_receipt_template_type || returnSetting.value || 'return_receipt_58mm'}
                onChange={(e) => onFieldChange('return_receipt_template_type', e.target.value)}
              />
            </div>
          );
        })()}

        {/* ── 📄 Копії для різних типів ──────────── */}
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Копії для чеків
            </label>
            <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
              Скільки примірників друкувати для звичайних чеків
            </p>
            <input
              type="number"
              value={values.receipt_print_copies || '1'}
              onChange={(e) => onFieldChange('receipt_print_copies', e.target.value)}
              className="w-full rounded-lg border border-gray-300 dark:border-slate-600 
                bg-white dark:bg-slate-800 px-3 py-2 text-sm
                text-gray-900 dark:text-gray-100
                focus:border-primary-500 focus:ring-1 focus:ring-primary-500"
              min="1"
              max="10"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Копії для звітів
            </label>
            <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">
              Скільки примірників друкувати для X/Z-звітів
            </p>
            <input
              type="number"
              value={values.report_print_copies || '1'}
              onChange={(e) => onFieldChange('report_print_copies', e.target.value)}
              className="w-full rounded-lg border border-gray-300 dark:border-slate-600 
                bg-white dark:bg-slate-800 px-3 py-2 text-sm
                text-gray-900 dark:text-gray-100
                focus:border-primary-500 focus:ring-1 focus:ring-primary-500"
              min="1"
              max="10"
            />
          </div>
        </div>

        {/* ── Роздільник: Цінники ──────────── */}
        <hr className="border-gray-200 dark:border-slate-700 my-2" />
        <div className="flex items-center gap-3 mb-2">
          <div className="w-8 h-8 rounded-lg bg-amber-50 dark:bg-amber-900/20 flex items-center justify-center">
            <Tag className="w-4 h-4 text-amber-600 dark:text-amber-400" />
          </div>
          <div>
            <h4 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
              Цінники
            </h4>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Налаштування шаблону цінників
            </p>
          </div>
        </div>

        {/* Поля на ціннику */}
        {priceTagFields && (
          <FieldsSelector
            label={priceTagFields.label}
            description={priceTagFields.description || ''}
            selectedFields={selectedPriceTagFields}
            onChange={(fields) => onFieldChange('price_tag_fields', JSON.stringify(fields))}
          />
        )}

        {/* Розмір цінника */}
        {priceTagWidth && priceTagHeight && (
          <SizeInputs
            widthKey="price_tag_width"
            heightKey="price_tag_height"
            widthValue={values.price_tag_width ?? priceTagWidth.value ?? '40'}
            heightValue={values.price_tag_height ?? priceTagHeight.value ?? '25'}
            label="цінника"
            onFieldChange={onFieldChange}
          />
        )}

        {/* ── Роздільник: Етикетки ─────────── */}
        <hr className="border-gray-200 dark:border-slate-700 my-2" />
        <div className="flex items-center gap-3 mb-2">
          <div className="w-8 h-8 rounded-lg bg-teal-50 dark:bg-teal-900/20 flex items-center justify-center">
            <Sticker className="w-4 h-4 text-teal-600 dark:text-teal-400" />
          </div>
          <div>
            <h4 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
              Етикетки
            </h4>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Налаштування шаблону етикеток
            </p>
          </div>
        </div>

        {/* Поля на етикетці */}
        {labelFields && (
          <FieldsSelector
            label={labelFields.label}
            description={labelFields.description || ''}
            selectedFields={selectedLabelFields}
            onChange={(fields) => onFieldChange('label_fields', JSON.stringify(fields))}
          />
        )}

        {/* Розмір етикетки */}
        {labelWidth && labelHeight && (
          <SizeInputs
            widthKey="label_width"
            heightKey="label_height"
            widthValue={values.label_width ?? labelWidth.value ?? '60'}
            heightValue={values.label_height ?? labelHeight.value ?? '40'}
            label="етикетки"
            onFieldChange={onFieldChange}
          />
        )}

        {/* ── 👁️ Попередній перегляд чеку ──────────── */}
        <hr className="border-gray-200 dark:border-slate-700 my-2" />
        <div>
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-blue-50 dark:bg-blue-900/20 flex items-center justify-center">
                <Eye className="w-4 h-4 text-blue-600 dark:text-blue-400" />
              </div>
              <div>
                <h4 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                  Попередній перегляд чеку
                </h4>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  Як виглядатиме чек з поточними налаштуваннями
                </p>
              </div>
            </div>
            <Button
              variant="secondary"
              size="sm"
              onClick={async () => {
                try {
                  // 1. Отримуємо ID дефолтного шаблону для receipt_58mm
                  const templateRes = await api.get('/print-templates/default', {
                    params: { type: values.default_template_type || 'receipt_58mm' },
                  });
                  const templateId = templateRes.data.id;

                  // 2. Рендеримо шаблон з демо-даними
                  const renderRes = await api.post(`/print-templates/${templateId}/render`, {
                    data: {
                      shop_name: values.company_name || 'Мій магазин',
                      shop_address: values.company_address || 'вул. Шевченка, 1',
                      tax_id: values.company_edrpou || '12345678',
                      receipt_number: '0001',
                      date: new Date().toLocaleDateString('uk-UA'),
                      time: new Date().toLocaleTimeString('uk-UA', { hour: '2-digit', minute: '2-digit' }),
                      cashier: 'Іван Петренко',
                      items: '<div style="margin-bottom:4px;"><div style="display:flex;justify-content:space-between;"><span>Хліб білий</span><span>25.00</span></div><div style="display:flex;justify-content:space-between;font-size:10px;color:#666;"><span>2 × 25.00</span><span style="font-weight:bold;">50.00</span></div></div><div style="margin-bottom:4px;"><div style="display:flex;justify-content:space-between;"><span>Молоко 2.6%</span><span>38.50</span></div><div style="display:flex;justify-content:space-between;font-size:10px;color:#666;"><span>1 × 38.50</span><span style="font-weight:bold;">38.50</span></div></div><div style="margin-bottom:4px;"><div style="display:flex;justify-content:space-between;"><span>Яйця курячі (10шт)</span><span>65.00</span></div><div style="display:flex;justify-content:space-between;font-size:10px;color:#666;"><span>1 × 65.00</span><span style="font-weight:bold;">65.00</span></div></div>',
                      total: '153.50',
                      payment_method: 'Готівка',
                      paid: '200.00',
                      change: '46.50',
                      footer: 'Дякуємо за покупку!',
                    },
                  });
                  setPreviewReceiptHtml(renderRes.data.html);
                } catch (err) {
                  const error = err as any;
                  toast.error(`Не вдалося згенерувати прев'ю: ${error?.response?.data?.detail || error?.message || 'Невідома помилка'}`);
                }
              }}
            >
              <Eye className="w-3.5 h-3.5" />
              Оновити прев'ю
            </Button>
          </div>
          <div className="border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden bg-white">
            {previewReceiptHtml ? (
              <iframe
                srcDoc={previewReceiptHtml}
                title="Прев'ю чеку"
                className="w-full h-[400px]"
                sandbox="allow-same-origin"
              />
            ) : (
              <div className="flex items-center justify-center h-[200px] text-gray-500 dark:text-gray-400">
                <div className="text-center">
                  <FileText className="w-8 h-8 mx-auto mb-2 opacity-50" />
                  <p className="text-sm">Натисніть "Оновити прев'ю", щоб побачити чек</p>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* ── 🏷️ Попередній перегляд цінника/етикетки ── */}
        <hr className="border-gray-200 dark:border-slate-700 my-2" />
        <div>
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-amber-50 dark:bg-amber-900/20 flex items-center justify-center">
                <Tag className="w-4 h-4 text-amber-600 dark:text-amber-400" />
              </div>
              <div>
                <h4 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                  Попередній перегляд цінника / етикетки
                </h4>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  Тестовий рендер з поточними розмірами та шаблоном
                </p>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => handleTestPrintPreview('price_tag')}
                disabled={isTestLoading !== null}
                icon={isTestLoading === 'price_tag' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Tag className="w-3.5 h-3.5" />}
              >
                {isTestLoading === 'price_tag' ? 'Генерація...' : 'Тест цінника'}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => handleTestPrintPreview('label')}
                disabled={isTestLoading !== null}
                icon={isTestLoading === 'label' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Sticker className="w-3.5 h-3.5" />}
              >
                {isTestLoading === 'label' ? 'Генерація...' : 'Тест етикетки'}
              </Button>
            </div>
          </div>
          <div className="border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden bg-white">
            {previewTagLabelHtml ? (
              <iframe
                srcDoc={previewTagLabelHtml}
                title={`Прев'ю ${previewTagLabelType === 'price_tag' ? 'цінника' : 'етикетки'}`}
                className="w-full h-[400px]"
                sandbox="allow-same-origin"
              />
            ) : (
              <div className="flex items-center justify-center h-[200px] text-gray-500 dark:text-gray-400">
                <div className="text-center">
                  <Tag className="w-8 h-8 mx-auto mb-2 opacity-50" />
                  <p className="text-sm">
                    Натисніть «Тест цінника» або «Тест етикетки», щоб побачити результат
                  </p>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

// ── Основна сторінка ─────────────────────────
const SettingsPage: React.FC = () => {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [values, setValues] = useState<Record<string, string>>({});
  const [activeTab, setActiveTab] = useState<string | null>(null);

  // Завантаження налаштувань
  const { data, isLoading, error } = useQuery<SettingsData>({
    queryKey: ['settings'],
    queryFn: async () => {
      const response = await api.get('/settings');
      return response.data;
    },
  });

  // Ініціалізація значень при завантаженні
  useEffect(() => {
    if (data?.modules) {
      const initial: Record<string, string> = {};
      Object.values(data.modules).forEach((moduleSettings) => {
        moduleSettings.forEach((s) => {
          initial[s.key] = s.value ?? '';
        });
      });
      setValues(initial);

      // Встановлюємо перший доступний модуль як активний
      const keys = Object.keys(data.modules);
      if (keys.length > 0 && !activeTab) {
        setActiveTab(keys[0]);
      }
    }
  }, [data, activeTab]);

  // Мутація збереження
  const saveMutation = useMutation({
    mutationFn: async (newValues: Record<string, string>) => {
      const response = await api.put('/settings', { settings: newValues });
      return response.data;
    },
    onSuccess: () => {
      toast.success('Налаштування збережено');
      queryClient.invalidateQueries({ queryKey: ['settings'] });
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка збереження налаштувань');
    },
  });

  const handleFieldChange = useCallback((key: string, value: string) => {
    setValues((prev) => ({ ...prev, [key]: value }));
  }, []);

  const handleSave = useCallback(() => {
    // Збираємо всі значення
    const allValues = { ...values };
    saveMutation.mutate(allValues);
  }, [values, saveMutation]);

  const modules = data?.modules ?? {};
  const moduleKeys = Object.keys(modules);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <Spinner size="lg" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <div className="text-center">
          <p className="text-red-500 font-medium">Помилка завантаження налаштувань</p>
          <p className="text-sm text-gray-500 mt-1">Спробуйте пізніше</p>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto px-4 py-6 space-y-6">
      {/* Заголовок */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Налаштування
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Керування конфігурацією системи Torgashka
          </p>
        </div>
        <Button
          onClick={handleSave}
          disabled={saveMutation.isPending}
          className="flex items-center gap-2"
        >
          {saveMutation.isPending ? (
            <Spinner size="sm" />
          ) : (
            <Save className="w-4 h-4" />
          )}
          {saveMutation.isPending ? 'Збереження...' : 'Зберегти'}
        </Button>
      </div>

      {/* Таби модулів */}
      <div role="tablist" aria-label="Модулі налаштувань" className="flex flex-wrap gap-2">
        {moduleKeys.map((key) => {
          const config = MODULE_CONFIG[key];
          if (!config) return null;
          return (
            <button
              key={key}
              role="tab"
              aria-selected={activeTab === key}
              onClick={() => setActiveTab(key)}
              className={`
                flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all
                ${
                  activeTab === key
                    ? 'bg-primary-600 text-white shadow-sm'
                    : 'bg-white dark:bg-slate-800 text-gray-600 dark:text-gray-400 border border-gray-200 dark:border-slate-700 hover:bg-gray-50 dark:hover:bg-slate-700'
                }
              `}
            >
              {config.icon}
              {config.label}
              {modules[key].length > 0 && (
                <span className={`
                  text-xs px-1.5 py-0.5 rounded-full
                  ${activeTab === key ? 'bg-white/20 text-white' : 'bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400'}
                `}>
                  {modules[key].length}
                </span>
              )}
            </button>
          );
        })}

        {/* Окрема вкладка: Підключені пристрої (Tauri) */}
        <button
          type="button"
          role="tab"
          aria-selected={false}
          onClick={() => navigate('/settings/devices')}
          className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all
            bg-white dark:bg-slate-800 text-gray-600 dark:text-gray-400 border border-gray-200 dark:border-slate-700 hover:bg-gray-50 dark:hover:bg-slate-700"
        >
          <Plug className="w-4 h-4" />
          Підключені пристрої
        </button>
      </div>

      {/* Desktop-налаштування (Tauri): автозапуск + single-instance + оновлення */}
      <DesktopSettingsCard />

      {/* Контент активного модуля */}
      {activeTab && modules[activeTab] && (
        <div role="tabpanel" aria-label={MODULE_CONFIG[activeTab]?.label || activeTab}>
          <ModuleSection
            moduleKey={activeTab}
            settings={modules[activeTab]}
            values={values}
            onFieldChange={handleFieldChange}
            onNavigate={navigate}
          />
        </div>
      )}

      {!activeTab && moduleKeys.length === 0 && (
        <div className="text-center py-12 text-gray-500 dark:text-gray-400">
          <p>Немає доступних налаштувань</p>
        </div>
      )}
    </div>
  );
};

export default SettingsPage;
