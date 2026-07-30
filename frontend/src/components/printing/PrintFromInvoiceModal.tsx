import React, { useState, useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Printer, Tag, Loader2, Eye, FileText } from 'lucide-react';
import { printService } from '@/services/printService';
import { printTemplateService } from '@/services/printTemplateService';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import PrintPreview from './PrintPreview';
import type { InvoicePrintRequest, InvoicePrintResponse } from '@/types/print';
import type { PrintTemplate } from '@/types/printTemplate';

// ═══════════════════════════════════════════════════════════════
// ПРОПСИ
// ═══════════════════════════════════════════════════════════════

interface PrintFromInvoiceModalProps {
  isOpen: boolean;
  onClose: () => void;
  /** ID прибуткової накладної */
  invoiceId: string;
  /** Загальна кількість товарів у накладній */
  totalItems: number;
  /** Кількість товарів, у яких змінилась ціна (порівняно з поточною) */
  changedPriceCount: number;
}

// ═══════════════════════════════════════════════════════════════
// КОНФІГУРАЦІЯ ЗА ЗАМОВЧУВАННЯМ
// ═══════════════════════════════════════════════════════════════

const DEFAULT_SETTINGS: Record<string, { widthMm: number; heightMm: number; gapMm: number; marginMm: number }> = {
  price_tag: { widthMm: 40, heightMm: 25, gapMm: 3, marginMm: 10 },
  label: { widthMm: 58, heightMm: 40, gapMm: 2, marginMm: 0 },
};

// ═══════════════════════════════════════════════════════════════
// КОМПОНЕНТ
// ═══════════════════════════════════════════════════════════════

const PrintFromInvoiceModal: React.FC<PrintFromInvoiceModalProps> = ({
  isOpen,
  onClose,
  invoiceId,
  totalItems,
  changedPriceCount,
}) => {
  // ── Стан ──────────────────────────────────────
  const [printType, setPrintType] = useState<'price_tag' | 'label'>('price_tag');
  const [templateId, setTemplateId] = useState('');
  const [onlyChanged, setOnlyChanged] = useState(false);
  const [widthMm, setWidthMm] = useState(DEFAULT_SETTINGS.price_tag.widthMm);
  const [heightMm, setHeightMm] = useState(DEFAULT_SETTINGS.price_tag.heightMm);
  const [gapMm, setGapMm] = useState(DEFAULT_SETTINGS.price_tag.gapMm);
  const [marginMm, setMarginMm] = useState(DEFAULT_SETTINGS.price_tag.marginMm);
  const [barcodeType, setBarcodeType] = useState<'code128' | 'qr'>('code128');
  const [barcodeHeightMm, setBarcodeHeightMm] = useState(10);

  const [previewData, setPreviewData] = useState<InvoicePrintResponse | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPrinting, setIsPrinting] = useState(false);

  // ── Завантаження шаблонів за типом ────────────
  const templateType = printType === 'price_tag' ? 'price_tag' : 'label';
  const { data: templates, isLoading: templatesLoading } = useQuery<PrintTemplate[]>({
    queryKey: ['print-templates', templateType],
    queryFn: () => printTemplateService.getAllIncludingInactive(),
  });

  // Фільтруємо шаблони за типом
  const filteredTemplates = useMemo(() => {
    return (templates || []).filter((t) => t.type === templateType);
  }, [templates, templateType]);

  // Знаходимо шаблон за замовчуванням
  const defaultTemplate = useMemo(() => {
    return filteredTemplates.find((t) => t.is_default) || filteredTemplates[0];
  }, [filteredTemplates]);

  // Автоматично вибираємо дефолтний шаблон при зміні типу
  React.useEffect(() => {
    if (defaultTemplate && !templateId) {
      setTemplateId(defaultTemplate.id);
    }
  }, [defaultTemplate, templateId]);

  // При зміні типу друку — скидаємо розміри та шаблон
  const handlePrintTypeChange = useCallback((type: 'price_tag' | 'label') => {
    setPrintType(type);
    setTemplateId('');
    setPreviewData(null);
    const defs = DEFAULT_SETTINGS[type];
    setWidthMm(defs.widthMm);
    setHeightMm(defs.heightMm);
    setGapMm(defs.gapMm);
    setMarginMm(defs.marginMm);
  }, []);

  // ── Прев'ю ────────────────────────────────────
  const handlePreview = useCallback(async () => {
    if (!templateId) {
      toast.error('Виберіть шаблон');
      return;
    }
    setIsPreviewLoading(true);
    setPreviewData(null);
    try {
      const data: InvoicePrintRequest = {
        print_type: printType,
        only_changed: onlyChanged,
        template_id: templateId,
        width_mm: widthMm,
        height_mm: heightMm,
        gap_mm: gapMm,
        margin_mm: marginMm,
        barcode_type: barcodeType,
        barcode_height_mm: barcodeHeightMm,
      };
      const result = await printService.renderInvoicePrintItems(invoiceId, data);
      setPreviewData(result);
    } catch (err: any) {
      const msg = err?.response?.data?.detail || 'Помилка генерації';
      toast.error(msg);
    } finally {
      setIsPreviewLoading(false);
    }
  }, [templateId, printType, onlyChanged, widthMm, heightMm, gapMm, marginMm, barcodeType, barcodeHeightMm, invoiceId]);

  // ── Друк ──────────────────────────────────────
  const handlePrint = useCallback(async () => {
    if (!templateId) {
      toast.error('Виберіть шаблон');
      return;
    }
    if (!previewData) {
      // Якщо прев'ю ще не завантажено — завантажуємо і одразу друкуємо
      setIsPrinting(true);
      try {
        const data: InvoicePrintRequest = {
          print_type: printType,
          only_changed: onlyChanged,
          template_id: templateId,
          width_mm: widthMm,
          height_mm: heightMm,
          gap_mm: gapMm,
          margin_mm: marginMm,
          barcode_type: barcodeType,
          barcode_height_mm: barcodeHeightMm,
        };
        const result = await printService.renderInvoicePrintItems(invoiceId, data);
        setPreviewData(result);
        await printHTML(result.html);
        showSummary(result);
      } catch (err: any) {
        const msg = err?.response?.data?.detail || 'Помилка друку';
        toast.error(msg);
      } finally {
        setIsPrinting(false);
      }
      return;
    }
    // Якщо прев'ю є — друкуємо його
    setIsPrinting(true);
    try {
      await printHTML(previewData.html);
      showSummary(previewData);
    } catch (err: any) {
      toast.error(err?.message || 'Помилка друку');
    } finally {
      setIsPrinting(false);
    }
  }, [templateId, printType, onlyChanged, widthMm, heightMm, gapMm, marginMm, barcodeType, barcodeHeightMm, invoiceId, previewData]);

  // ── Допоміжні функції ─────────────────────────
  const printHTML = async (html: string): Promise<void> => {
    const win = window.open('', '_blank');
    if (!win) {
      toast.error('Блокувальник спливних вікон. Дозвольте спливні вікна для цього сайту.');
      return;
    }
    win.document.write(html);
    win.document.close();
    // Чекаємо завантаження зображень
    await new Promise<void>((resolve) => {
      if (win.document.readyState === 'complete') {
        resolve();
      } else {
        win.addEventListener('load', () => resolve());
      }
    });
    win.focus();
    win.print();
  };

  const showSummary = (result: InvoicePrintResponse) => {
    const label = printType === 'price_tag' ? 'цінників' : 'етикеток';
    const parts: string[] = [`Створено ${result.total_labels} ${label}`];
    if (result.total_pages !== undefined) {
      parts.push(`на ${result.total_pages} стор.`);
    }
    if (result.changed_count !== undefined) {
      parts.push(`(змінних цін: ${result.changed_count})`);
    }
    toast.success(parts.join(' '));
  };

  // ── Опції для Select ──────────────────────────
  const templateOptions = useMemo(() => {
    return filteredTemplates.map((t) => ({
      value: t.id,
      label: `${t.name}${t.is_default ? ' ★' : ''}`,
    }));
  }, [filteredTemplates]);

  // Кількість товарів для друку
  const effectiveCount = onlyChanged ? changedPriceCount : totalItems;

  // ── Render ────────────────────────────────────
  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Друк цінників / етикеток з накладної"
      size="4xl"
    >
      <div className="space-y-5">
        {/* 1. Вибір типу друку */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Тип друку
          </label>
          <div className="flex items-center gap-3">
            <button
              onClick={() => handlePrintTypeChange('price_tag')}
              className={`
                flex-1 px-4 py-3 rounded-lg text-sm font-medium transition-all duration-150 border
                ${printType === 'price_tag'
                  ? 'bg-primary-600 text-white shadow-sm ring-1 ring-primary-700 border-primary-600'
                  : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600 border-transparent'
                }
              `}
            >
              <div className="flex items-center justify-center gap-2">
                <Tag className="w-5 h-5" />
                <span>Цінники (A4 аркуш)</span>
              </div>
              <p className="text-xs mt-1 opacity-70">Для звичайного принтера</p>
            </button>
            <button
              onClick={() => handlePrintTypeChange('label')}
              className={`
                flex-1 px-4 py-3 rounded-lg text-sm font-medium transition-all duration-150 border
                ${printType === 'label'
                  ? 'bg-primary-600 text-white shadow-sm ring-1 ring-primary-700 border-primary-600'
                  : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600 border-transparent'
                }
              `}
            >
              <div className="flex items-center justify-center gap-2">
                <Printer className="w-5 h-5" />
                <span>Етикетки (термопринтер)</span>
              </div>
              <p className="text-xs mt-1 opacity-70">Для термопринтера</p>
            </button>
          </div>
        </div>

        {/* 2. Вибір шаблону */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
            Шаблон {printType === 'price_tag' ? 'цінника' : 'етикетки'}
          </label>
          {templatesLoading ? (
            <div className="flex items-center gap-2 text-sm text-gray-400">
              <Spinner size="sm" />
              Завантаження шаблонів...
            </div>
          ) : (
            <Select
              options={templateOptions}
              value={templateId}
              onChange={(e) => setTemplateId(e.target.value)}
              placeholder="Виберіть шаблон..."
            />
          )}
          {!templatesLoading && filteredTemplates.length === 0 && (
            <p className="text-sm text-amber-600 dark:text-amber-400 mt-1">
              Немає шаблонів для {printType === 'price_tag' ? 'цінників' : 'етикеток'}. 
              Створіть спочатку шаблон у налаштуваннях друку.
            </p>
          )}
        </div>

        {/* 3. Налаштування розміру */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Розмір та параметри
          </label>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            <div>
              <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Ширина (мм)</label>
              <input
                type="number"
                value={widthMm}
                onChange={(e) => setWidthMm(Number(e.target.value))}
                min={10}
                max={200}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Висота (мм)</label>
              <input
                type="number"
                value={heightMm}
                onChange={(e) => setHeightMm(Number(e.target.value))}
                min={10}
                max={300}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Проміжок (мм)</label>
              <input
                type="number"
                value={gapMm}
                onChange={(e) => setGapMm(Number(e.target.value))}
                min={0}
                max={50}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
              />
            </div>
            {printType === 'price_tag' && (
              <div>
                <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Поля (мм)</label>
                <input
                  type="number"
                  value={marginMm}
                  onChange={(e) => setMarginMm(Number(e.target.value))}
                  min={0}
                  max={50}
                  className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
                />
              </div>
            )}
          </div>
        </div>

        {/* Barcode налаштування */}
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Тип штрих-коду</label>
            <select
              value={barcodeType}
              onChange={(e) => setBarcodeType(e.target.value as 'code128' | 'qr')}
              className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
            >
              <option value="code128">Code 128</option>
              <option value="qr">QR-код</option>
            </select>
          </div>
          <div>
            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Висота штрих-коду (мм)</label>
            <input
              type="number"
              value={barcodeHeightMm}
              onChange={(e) => setBarcodeHeightMm(Number(e.target.value))}
              min={5}
              max={50}
              className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
            />
          </div>
        </div>

        {/* 4. Режим друку */}
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Режим друку
          </label>
          <div className="space-y-2">
            <button
              onClick={() => setOnlyChanged(false)}
              className={`
                w-full flex items-center gap-3 px-4 py-2.5 rounded-lg text-sm transition-colors border
                ${!onlyChanged
                  ? 'bg-primary-50 dark:bg-primary-900/20 border-primary-300 dark:border-primary-700 text-primary-700 dark:text-primary-400'
                  : 'bg-gray-50 dark:bg-slate-700/50 border-gray-200 dark:border-slate-600 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700'
                }
              `}
            >
              <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center ${
                !onlyChanged
                  ? 'border-primary-600 bg-primary-600'
                  : 'border-gray-400 dark:border-gray-500'
              }`}>
                {!onlyChanged && <div className="w-1.5 h-1.5 rounded-full bg-white" />}
              </div>
              <div className="text-left">
                <span className="font-medium">Для всіх товарів</span>
                <span className="ml-2 text-gray-500 dark:text-gray-400">({totalItems} шт.)</span>
              </div>
            </button>
            <button
              onClick={() => changedPriceCount > 0 && setOnlyChanged(true)}
              className={`
                w-full flex items-center gap-3 px-4 py-2.5 rounded-lg text-sm transition-colors border
                ${changedPriceCount === 0
                  ? 'opacity-50 cursor-not-allowed bg-gray-50 dark:bg-slate-700/50 border-gray-200 dark:border-slate-600 text-gray-500 dark:text-gray-400'
                  : onlyChanged
                    ? 'bg-primary-50 dark:bg-primary-900/20 border-primary-300 dark:border-primary-700 text-primary-700 dark:text-primary-400'
                    : 'bg-gray-50 dark:bg-slate-700/50 border-gray-200 dark:border-slate-600 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700'
                }
              `}
              disabled={changedPriceCount === 0}
            >
              <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center ${
                onlyChanged
                  ? 'border-primary-600 bg-primary-600'
                  : 'border-gray-400 dark:border-gray-500'
              }`}>
                {onlyChanged && <div className="w-1.5 h-1.5 rounded-full bg-white" />}
              </div>
              <div className="text-left">
                <span className="font-medium">Тільки товари зі змінною ціною</span>
                <span className="ml-2 text-gray-500 dark:text-gray-400">({changedPriceCount} шт.)</span>
                {changedPriceCount === 0 && (
                  <span className="ml-2 text-xs text-gray-400">(немає змін)</span>
                )}
              </div>
            </button>
          </div>
        </div>

        {/* 5. Прев'ю */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
              Попередній перегляд
            </label>
            <Button
              variant="secondary"
              size="sm"
              onClick={handlePreview}
              disabled={isPreviewLoading}
              icon={isPreviewLoading ? <Spinner size="sm" /> : <Eye className="w-4 h-4" />}
            >
              {isPreviewLoading ? 'Генерація...' : 'Оновити прев\'ю'}
            </Button>
          </div>
          <div className="h-72 md:h-96 border border-gray-200 dark:border-slate-700 rounded-lg overflow-hidden">
            <PrintPreview
              html={previewData?.html || null}
              isLoading={isPreviewLoading}
              totalPages={previewData?.total_pages}
              totalLabels={previewData?.total_labels}
              type={printType}
            />
          </div>
        </div>

        {/* Підсумок після прев'ю */}
        {previewData && !isPreviewLoading && (
          <div className="flex items-center gap-3 text-sm text-gray-600 dark:text-gray-400 bg-gray-50 dark:bg-slate-700/50 rounded-lg px-4 py-2.5">
            <FileText className="w-4 h-4 text-primary-500" />
            <span>
              Всього товарів: <strong>{previewData.total_count}</strong>
              {previewData.changed_count !== undefined && (
                <span>, зі змінною ціною: <strong>{previewData.changed_count}</strong></span>
              )}
              , створено {printType === 'price_tag' ? 'цінників' : 'етикеток'}: <strong>{previewData.total_labels}</strong>
              {previewData.total_pages !== undefined && (
                <span>, сторінок: <strong>{previewData.total_pages}</strong></span>
              )}
            </span>
          </div>
        )}
      </div>

      {/* Кнопки дій */}
      <div className="flex items-center justify-between pt-5 mt-5 border-t border-gray-200 dark:border-slate-700">
        <div className="text-xs text-gray-400 dark:text-gray-500">
          {printType === 'price_tag' ? 'Цінники (A4 аркуш)' : 'Етикетки (термопринтер)'}
          {effectiveCount > 0 && (
            <span> · {printType === 'price_tag' ? 'цінників' : 'етикеток'}: {effectiveCount}</span>
          )}
        </div>
        <div className="flex items-center gap-3">
          <Button variant="secondary" onClick={onClose}>
            Скасувати
          </Button>
          <Button
            variant="primary"
            onClick={handlePrint}
            disabled={isPrinting || !templateId || filteredTemplates.length === 0}
            icon={isPrinting ? <Spinner size="sm" /> : <Printer className="w-4 h-4" />}
          >
            {isPrinting ? 'Друк...' : 'Надрукувати'}
          </Button>
        </div>
      </div>
    </Modal>
  );
};

export default PrintFromInvoiceModal;
