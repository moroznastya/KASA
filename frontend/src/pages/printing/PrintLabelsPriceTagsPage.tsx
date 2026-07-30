import React, { useState, useCallback, useEffect, useMemo } from 'react';
import { Printer, ArrowLeft, Eye, Loader2, Tag } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { printService } from '@/services/printService';
import { settingsService } from '@/services/settingsService';
import { isTauri } from '@/hooks/useTauri';
import { usePrintAsImage } from '@/hooks/usePrintAsImage';
import PrintProductSelector from '@/components/printing/PrintProductSelector';
import PrintPreview from '@/components/printing/PrintPreview';
import PrintSettingsPanel from '@/components/printing/PrintSettingsPanel';
import PrinterSelector from '@/components/printing/PrinterSelector';
import { PRINT_TYPES } from '@/types/print';
import type { Product } from '@/types/product';
import type { SelectedProduct, PrintType, BarcodeType } from '@/types/print';
import toast from 'react-hot-toast';

// ── Тип для зручного onChange ────────────────────
interface PrintSettings {
  templateId: string;
  widthMm: number;
  heightMm: number;
  gapMm: number;
  marginMm: number;
}

// ── Ключі налаштувань для збереження між сесіями ─
const SETTINGS_KEYS = {
  printerName: 'printer_name',
  templateId: 'print_template_id',
  widthMm: 'print_width_mm',
  heightMm: 'print_height_mm',
  gapMm: 'print_gap_mm',
  marginMm: 'print_margin_mm',
  barcodeType: 'print_barcode_type',
} as const;

// ── Функція: витягнути тільки вміст <body> з HTML ─
function extractBodyContent(html: string): string {
  let clean = html.replace(/<!DOCTYPE[^>]*>/gi, '');
  clean = clean.replace(/<\/?html[^>]*>/gi, '');
  clean = clean.replace(/<head[^>]*>[\s\S]*?<\/head>/gi, '');
  const bodyMatch = clean.match(/<body[^>]*>([\s\S]*)<\/body>/i);
  if (bodyMatch) return bodyMatch[1];
  return clean.trim();
}

// ═══════════════════════════════════════════════════════════
// Компонент сторінки
// ═══════════════════════════════════════════════════════════
const PrintLabelsPriceTagsPage: React.FC = () => {
  // ── Стан ─────────────────────────────────────────
  const [printType, setPrintType] = useState<PrintType>('price_tag');
  const config = PRINT_TYPES[printType];
  const [selected, setSelected] = useState<SelectedProduct[]>([]);
  const [settings, setSettings] = useState<PrintSettings>({
    templateId: config.defaultSettings.templateId,
    widthMm: config.defaultSettings.widthMm,
    heightMm: config.defaultSettings.heightMm,
    gapMm: config.defaultSettings.gapMm,
    marginMm: config.defaultSettings.marginMm,
  });
  const [barcodeType, setBarcodeType] = useState<BarcodeType>('code128');
  const [printerName, setPrinterName] = useState<string>('');
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPrinting, setIsPrinting] = useState(false);
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [renderResult, setRenderResult] = useState<{
    totalPages?: number;
    totalLabels: number;
  } | null>(null);

  // Print-as-Image (Tauri + label)
  const { receiptRef, captureAndPrint: captureAndPrintLabel, isCapturing } =
    usePrintAsImage({ showErrors: true });
  const isPrintingActive = isPrinting || isCapturing;

  // Очищений HTML для hidden div (без DOCTYPE/html/head/body)
  const cleanHtmlForReceipt = useMemo(() => {
    if (!previewHtml) return '';
    return extractBodyContent(previewHtml);
  }, [previewHtml]);

  // ── Завантаження збережених налаштувань при монтуванні ──
  useEffect(() => {
    const load = async () => {
      try {
        const values = await Promise.all(
          Object.values(SETTINGS_KEYS).map((key) =>
            settingsService.getValue(key).then((v) => ({ key, value: v }))
          )
        );
        const map = Object.fromEntries(
          values.filter((v) => v.value !== null).map((v) => [v.key, v.value!])
        );

        if (map.printer_name) setPrinterName(map.printer_name);
        if (map.print_barcode_type === 'qr' || map.print_barcode_type === 'code128') {
          setBarcodeType(map.print_barcode_type);
        }

        setSettings((prev) => ({
          templateId: map.print_template_id ?? prev.templateId,
          widthMm: map.print_width_mm ? Number(map.print_width_mm) : prev.widthMm,
          heightMm: map.print_height_mm ? Number(map.print_height_mm) : prev.heightMm,
          gapMm: map.print_gap_mm ? Number(map.print_gap_mm) : prev.gapMm,
          marginMm: map.print_margin_mm ? Number(map.print_margin_mm) : prev.marginMm,
        }));
      } catch {
        // ignore — використовуємо дефолтні значення
      } finally {
        setSettingsLoaded(true);
      }
    };
    load();
  }, []);

  // ── Автозбереження налаштувань при зміні ────────
  const saveSetting = useCallback(async (key: string, value: string) => {
    try {
      await settingsService.update(key, value);
    } catch {
      // silent
    }
  }, []);

  // ── Перемикання типу друку ──────────────────────
  const handleTypeChange = useCallback(
    (newType: PrintType) => {
      if (newType === printType) return;
      const newConfig = PRINT_TYPES[newType];
      setPreviewHtml(null);
      setRenderResult(null);
      setPrintType(newType);
      setSettings((prev) => ({
        templateId: prev.templateId || newConfig.defaultSettings.templateId,
        widthMm: prev.widthMm || newConfig.defaultSettings.widthMm,
        heightMm: prev.heightMm || newConfig.defaultSettings.heightMm,
        gapMm: prev.gapMm || newConfig.defaultSettings.gapMm,
        marginMm: prev.marginMm || newConfig.defaultSettings.marginMm,
      }));
    },
    [printType],
  );

  // ── Зміна принтера ──────────────────────────────
  const handlePrinterChange = useCallback(
    async (name: string) => {
      setPrinterName(name);
      await saveSetting(SETTINGS_KEYS.printerName, name);
    },
    [saveSetting],
  );

  // ── Зміна налаштувань друку ────────────────────
  const handleSettingsChange = useCallback(
    (field: string, value: string | number) => {
      setSettings((prev) => {
        const next = { ...prev, [field]: value };
        const keyMap: Record<string, string> = {
          templateId: SETTINGS_KEYS.templateId,
          widthMm: SETTINGS_KEYS.widthMm,
          heightMm: SETTINGS_KEYS.heightMm,
          gapMm: SETTINGS_KEYS.gapMm,
          marginMm: SETTINGS_KEYS.marginMm,
        };
        const settingsKey = keyMap[field];
        if (settingsKey) {
          saveSetting(settingsKey, String(value));
        }
        return next;
      });
    },
    [saveSetting],
  );

  // ── Зміна типу штрих-коду ──────────────────────
  const handleBarcodeTypeChange = useCallback(
    async (type: BarcodeType) => {
      setBarcodeType(type);
      await saveSetting(SETTINGS_KEYS.barcodeType, type);
    },
    [saveSetting],
  );

  // ── Операції з товарами ────────────────────────
  const handleAdd = useCallback((product: Product) => {
    setSelected((prev) => [
      ...prev,
      {
        id: product.id,
        title: product.title,
        price: product.price,
        barcode: product.barcode || '',
        sku: product.sku,
        category_id: product.category_id,
        copies: 1,
      },
    ]);
  }, []);

  const handleRemove = useCallback((id: string) => {
    setSelected((prev) => prev.filter((item) => item.id !== id));
  }, []);

  const handleUpdateCopies = useCallback((id: string, copies: number) => {
    setSelected((prev) =>
      prev.map((item) => (item.id === id ? { ...item, copies } : item))
    );
  }, []);

  // ── Генерація прев'ю ─────────────────────────────
  const label = printType === 'price_tag' ? 'цінників' : 'етикеток';

  const handleGeneratePreview = useCallback(async () => {
    if (!settings.templateId) {
      toast.error(`Виберіть шаблон ${printType === 'price_tag' ? 'цінника' : 'етикетки'}`);
      return;
    }
    if (selected.length === 0) {
      toast.error('Додайте хоча б один товар');
      return;
    }

    setIsPreviewLoading(true);

    try {
      const products = selected.map((item) => ({
        id: item.id,
        title: item.title,
        price: item.price,
        barcode: item.barcode,
        article: item.sku || undefined,
        copies: item.copies,
      }));

      if (printType === 'price_tag') {
        const result = await printService.renderPriceTags({
          template_id: settings.templateId,
          products,
          width_mm: settings.widthMm,
          height_mm: settings.heightMm,
          gap_mm: settings.gapMm,
          margin_mm: settings.marginMm,
          barcode_type: barcodeType,
          barcode_height_mm: 12,
        });
        setPreviewHtml(result.html);
        setRenderResult({ totalPages: result.total_pages, totalLabels: result.total_labels });
        toast.success(`Згенеровано ${result.total_labels} цінників на ${result.total_pages} стор.`);
      } else {
        const result = await printService.renderLabels({
          template_id: settings.templateId,
          products,
          width_mm: settings.widthMm,
          height_mm: settings.heightMm,
          gap_mm: settings.gapMm,
          barcode_type: barcodeType,
          barcode_height_mm: 12,
        });
        setPreviewHtml(result.html);
        setRenderResult({ totalLabels: result.total_labels });
        toast.success(`Згенеровано ${result.total_labels} етикеток`);
      }
    } catch (err: any) {
      const msg = err?.response?.data?.detail || `Помилка генерації ${label}`;
      toast.error(msg);
    } finally {
      setIsPreviewLoading(false);
    }
  }, [settings, selected, printType, label, barcodeType]);

  // ── Друк ────────────────────────────────────────
  const handlePrint = useCallback(async () => {
    if (!previewHtml) {
      toast.error("Спочатку згенеруйте прев'ю");
      return;
    }

    setIsPrinting(true);
    try {
      if (isTauri()) {
        await captureAndPrintLabel(printerName || undefined);
        toast.success('Відправлено на друк');
      } else {
        printViaBrowser(previewHtml);
      }
    } catch (err: any) {
      toast.error(err?.message || 'Помилка друку');
    } finally {
      setIsPrinting(false);
    }
  }, [previewHtml, printType, printerName, captureAndPrintLabel]);

  function printViaBrowser(html: string) {
    const printWindow = window.open('', '_blank');
    if (printWindow) {
      printWindow.document.write(html);
      printWindow.document.close();
      printWindow.focus();
      setTimeout(() => printWindow.print(), 500);
    } else {
      toast.error('Блокувальник спливних вікон. Дозвольте спливні вікна для цього сайту.');
    }
  }

  // ═══════════════════════════════════════════════════════════
  // Рендер
  // ═══════════════════════════════════════════════════════════
  if (!settingsLoaded) {
    return (
      <div className="flex items-center justify-center h-[calc(100vh-4rem)]">
        <div className="text-gray-400 dark:text-gray-500">Завантаження налаштувань...</div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* Заголовок */}
      <div className="flex items-center justify-between mb-4 flex-shrink-0">
        <div className="flex items-center gap-3">
          <button
            onClick={() => window.history.back()}
            className="p-2 rounded-lg text-gray-500 hover:text-gray-700 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
            title="Назад"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100">
              Друк {printType === 'price_tag' ? 'цінників' : 'етикеток'}
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              {config.description}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            onClick={handleGeneratePreview}
            disabled={isPreviewLoading || selected.length === 0 || !settings.templateId}
            icon={
              isPreviewLoading ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Eye className="w-4 h-4" />
              )
            }
          >
            {isPreviewLoading ? 'Генерація...' : "Оновити прев'ю"}
          </Button>

          <Button
            onClick={handlePrint}
            disabled={isPrintingActive || !previewHtml}
            size="lg"
            icon={
              isPrintingActive ? (
                <Loader2 className="w-5 h-5 animate-spin" />
              ) : (
                <Printer className="w-5 h-5" />
              )
            }
          >
            {isPrintingActive ? 'Друк...' : `🖨️ Друк ${printType === 'price_tag' ? 'цінників' : 'етикеток'}`}
          </Button>
        </div>
      </div>

      {/* Перемикач типу друку */}
      <div className="flex items-center gap-2 mb-4 flex-shrink-0">
        {(Object.values(PRINT_TYPES) as Array<{ id: PrintType; label: string }>).map((type) => (
          <button
            key={type.id}
            onClick={() => handleTypeChange(type.id)}
            className={`
              px-4 py-2 rounded-lg text-sm font-medium transition-all duration-150
              ${printType === type.id
                ? 'bg-primary-600 text-white shadow-sm ring-1 ring-primary-700'
                : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600'
              }
            `}
          >
            <span className="flex items-center gap-2">
              {type.id === 'price_tag' ? <Tag className="w-4 h-4" /> : <Printer className="w-4 h-4" />}
              {type.label}
            </span>
          </button>
        ))}
      </div>

      {/* Три колонки */}
      <div
        data-print-grid="true"
        className="flex-1 grid grid-cols-1 lg:grid-cols-[560px_340px_minmax(0,1fr)] gap-4 min-h-0 min-w-0"
      >
        {/* Колонка 1 — Вибір товарів */}
        <div className="overflow-hidden card p-4 flex flex-col shrink-0">
          <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3 flex-shrink-0">
            Вибір товарів
          </h2>
          <div className="flex-1 min-h-0 overflow-hidden">
            <PrintProductSelector
              selected={selected}
              onAdd={handleAdd}
              onRemove={handleRemove}
              onUpdateCopies={handleUpdateCopies}
            />
          </div>
        </div>

        {/* Колонка 2 — Налаштування */}
        <div className="overflow-hidden card p-4 space-y-4 shrink-0">
          <div>
            <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
              Налаштування
            </h2>
            <div className="mb-4">
              <PrinterSelector value={printerName} onChange={handlePrinterChange} />
            </div>

            {/* ─── Вибір типу коду ───────────────── */}
            <div className="mb-4">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
                Тип коду
              </label>
              <div className="flex gap-2">
                {(['code128', 'qr'] as BarcodeType[]).map((type) => (
                  <button
                    key={type}
                    onClick={() => handleBarcodeTypeChange(type)}
                    className={`
                      flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-all duration-150
                      ${barcodeType === type
                        ? 'bg-primary-600 text-white shadow-sm ring-1 ring-primary-700'
                        : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600'
                      }
                    `}
                  >
                    {type === 'code128' ? '📊 Штрих-код' : '📱 QR-код'}
                  </button>
                ))}
              </div>
            </div>

            <PrintSettingsPanel
              templateId={settings.templateId}
              widthMm={settings.widthMm}
              heightMm={settings.heightMm}
              gapMm={settings.gapMm}
              marginMm={settings.marginMm}
              onChange={handleSettingsChange}
              type={printType}
            />
          </div>
        </div>

        {/* Колонка 3 — Прев'ю */}
        <div className="overflow-hidden card p-4 flex flex-col min-w-0">
          <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3 flex-shrink-0">
            Попередній перегляд
          </h2>
          <div className="flex-1 min-h-0 min-w-0 overflow-hidden">
            <PrintPreview
              html={previewHtml}
              isLoading={isPreviewLoading}
              totalPages={renderResult?.totalPages}
              totalLabels={renderResult?.totalLabels}
              type={printType}
            />
          </div>
        </div>
      </div>

      {/* Прихований div для html2canvas */}
      <div
        ref={receiptRef as React.RefObject<HTMLDivElement>}
        dangerouslySetInnerHTML={{ __html: cleanHtmlForReceipt }}
        style={{
          position: 'absolute',
          left: '-9999px',
          top: 0,
          width: previewHtml ? 'fit-content' : '1px',
          minWidth: previewHtml ? `${settings.widthMm * 3.78}px` : '1px',
          backgroundColor: 'white',
          color: 'black',
          zIndex: -1,
        }}
      />
    </div>
  );
};

export default PrintLabelsPriceTagsPage;
