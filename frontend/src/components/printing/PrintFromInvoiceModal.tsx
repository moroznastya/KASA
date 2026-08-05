import React, { useState, useCallback, useMemo, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { Printer, Tag, Loader2, Eye, FileText } from 'lucide-react';
import { printService } from '@/services/printService';
import { printTemplateService } from '@/services/printTemplateService';
import { settingsService } from '@/services/settingsService';
import { printHtml } from '@/services/tauri/print';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import PrintPreview from './PrintPreview';
import { usePrintAsImage } from '@/hooks/usePrintAsImage';
import { isTauri } from '@/hooks/useTauri';
import { extractBodyWithStyles } from '@/utils/printHtmlUtils';
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
  const [printerName, setPrinterName] = useState<string | undefined>(undefined);
  // Режим друку етикеток: 'system' (CUPS, дефолт) | 'escpos' (ESC/POS растр)
  const [labelPrintMode, setLabelPrintMode] = useState<'system' | 'escpos'>('system');

  const [previewData, setPreviewData] = useState<InvoicePrintResponse | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPrinting, setIsPrinting] = useState(false);

  // Print-as-Image для етикеток у Tauri (термопринтер)
  const { receiptRef, captureAndPrint: captureAndPrintLabel } = usePrintAsImage({ showErrors: true });

  // Очищений HTML (без DOCTYPE/html/head/body) для hidden div
  const cleanLabelHtml = useMemo(() => {
    if (!previewData?.html) return '';
    return extractBodyWithStyles(previewData.html);
  }, [previewData]);

  // ── Завантаження налаштувань принтера та режиму етикеток ──
  useEffect(() => {
    if (!isOpen) return;
    const loadPrinter = async () => {
      const printer = await settingsService.getValue('printer_name');
      if (printer) setPrinterName(printer);
      const mode = await settingsService.getValue('label_print_mode');
      if (mode === 'system' || mode === 'escpos') setLabelPrintMode(mode);
    };
    loadPrinter();
  }, [isOpen]);

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
        // 'label': режим етикеток (system — повна ширина CUPS, escpos — 48мм термо)
        print_mode: printType === 'label' ? labelPrintMode : 'system',
      };
      const result = await printService.renderInvoicePrintItems(invoiceId, data);
      setPreviewData(result);
    } catch (err: any) {
      const msg = err?.response?.data?.detail || 'Помилка генерації';
      toast.error(msg);
    } finally {
      setIsPreviewLoading(false);
    }
  }, [templateId, printType, onlyChanged, widthMm, heightMm, gapMm, marginMm, barcodeType, barcodeHeightMm, invoiceId, labelPrintMode]);

  // ── Отримати результат рендеру (прев'ю або новий) ──
  const getRenderResult = useCallback(async (): Promise<InvoicePrintResponse> => {
    if (previewData) return previewData;
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
      // 'label': режим етикеток (system — повна ширина CUPS, escpos — 48мм термо)
      print_mode: printType === 'label' ? labelPrintMode : 'system',
    };
    const result = await printService.renderInvoicePrintItems(invoiceId, data);
    setPreviewData(result);
    return result;
  }, [previewData, templateId, printType, onlyChanged, widthMm, heightMm, gapMm, marginMm, barcodeType, barcodeHeightMm, invoiceId, labelPrintMode]);

  // ── Читання копій та автовідрізання з system_settings ──
  const readPrintCopies = useCallback(async (): Promise<number | null> => {
    try {
      const v =
        (await settingsService.getValue('print_copies')) ||
        (await settingsService.getValue('receipt_print_copies'));
      if (!v) return null;
      const n = parseInt(v, 10);
      return !isNaN(n) && n > 0 ? n : null;
    } catch {
      return null;
    }
  }, []);

  const readAutoCut = useCallback(async (): Promise<boolean | null> => {
    try {
      const v = await settingsService.getValue('auto_cut_paper');
      if (v === null || v === undefined || v === '') return null;
      return v === 'true' || v === '1';
    } catch {
      return null;
    }
  }, []);

  // ── Друк через браузер (window.print) ─────────
  const printHtmlViaBrowser = useCallback(async (html: string): Promise<void> => {
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
  }, []);

  // ── Друк етикеток у Tauri ─────────────────────
  // Режим 'escpos' → html2canvas → PNG → ESC/POS растр (Rust) — для термопринтерів.
  // Режим 'system' (дефолт, якщо ключа немає) → нативний printHtml → системний
  // діалог webkit2gtk → CUPS-драйвер (TSPL2) — для принтерів, що НЕ розуміють
  // ESC/POS (напр. Xprinter XP-420B / LABEL-9X00).
  const printLabelViaTauri = useCallback(async (html: string): Promise<void> => {
    // Режим друку етикеток зі стану (завантажується при відкритті модалки
    // разом із printer_name; дефолт — 'system')
    const mode = labelPrintMode;

    if (mode === 'escpos') {
      // Чекаємо, поки React оновить hidden div (dangerouslySetInnerHTML)
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          setTimeout(resolve, 150);
        });
      });

      // Захоплюємо та друкуємо (з копіями/автовідрізанням з налаштувань)
      const [copies, autoCut] = await Promise.all([readPrintCopies(), readAutoCut()]);
      await captureAndPrintLabel(printerName || undefined, {
        copies,
        autoCut,
      });
      return;
    }

    // Системний друк (CUPS) — як для price_tag
    const result = await printHtml(html, printerName || undefined);
    if (result.success === false) {
      toast.error(result.message);
    } else {
      toast.success('Відправлено на друк');
    }
  }, [readPrintCopies, readAutoCut, printerName, labelPrintMode, captureAndPrintLabel]);

  // ── Друк ──────────────────────────────────────
  const handlePrint = useCallback(async () => {
    if (!templateId) {
      toast.error('Виберіть шаблон');
      return;
    }

    setIsPrinting(true);
    try {
      const result = await getRenderResult();

      // ═══ ЛОГІКА ВИБОРУ СПОСОБУ ДРУКУ ═══
      // 1. Tauri + label → Print-as-Image (термопринтер отримує ESC/POS растр)
      // 2. Tauri + price_tag (A4 аркуш) → нативний printHtml (системний діалог webkit2gtk)
      // 3. Не Tauri → window.print()
      if (isTauri() && printType === 'label') {
        await printLabelViaTauri(result.html);
      } else if (isTauri() && printType === 'price_tag') {
        // НАТИВНИЙ друк A4: системний діалог webkit2gtk (grid/SVG/page-break працюють)
        const printResult = await printHtml(result.html, printerName || undefined);
        if (printResult.success === false) {
          toast.error(printResult.message);
        } else {
          toast.success('Відправлено на друк');
        }
      } else {
        await printHtmlViaBrowser(result.html);
      }

      showSummary(result);
    } catch (err: any) {
      const msg = err?.response?.data?.detail || err?.message || 'Помилка друку';
      toast.error(msg);
    } finally {
      setIsPrinting(false);
    }
  }, [templateId, printType, getRenderResult, printLabelViaTauri, printHtmlViaBrowser]);

  const showSummary = useCallback((result: InvoicePrintResponse) => {
    const label = printType === 'price_tag' ? 'цінників' : 'етикеток';
    const parts: string[] = [`Створено ${result.total_labels} ${label}`];
    if (result.total_pages !== undefined) {
      parts.push(`на ${result.total_pages} стор.`);
    }
    if (result.changed_count !== undefined) {
      parts.push(`(змінних цін: ${result.changed_count})`);
    }
    toast.success(parts.join(' '));
  }, [printType]);

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

      {/* Прихований div для html2canvas (етикетки в Tauri) */}
      <div
        ref={receiptRef as React.RefObject<HTMLDivElement>}
        data-print-receipt="true"
        dangerouslySetInnerHTML={{ __html: cleanLabelHtml }}
        style={{
          position: 'absolute',
          left: '-9999px',
          top: 0,
          width: 'fit-content',
          minWidth: previewData?.html ? `${widthMm * 3.78}px` : '1px',
          backgroundColor: 'white',
          color: 'black',
          zIndex: -1,
        }}
      />

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
