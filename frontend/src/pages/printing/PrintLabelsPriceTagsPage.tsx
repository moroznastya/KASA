import React, { useState, useCallback, useEffect } from 'react';
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
import type { SelectedProduct, PrintType } from '@/types/print';
import toast from 'react-hot-toast';

// ── Тип для зручного onChange ────────────────────
interface PrintSettings {
  templateId: string;
  widthMm: number;
  heightMm: number;
  gapMm: number;
  marginMm: number;
}

// ── Компонент сторінки ───────────────────────────
const PrintLabelsPriceTagsPage: React.FC = () => {
  // ═══════════════════════════════════════════════════════════
  // Стан
  // ═══════════════════════════════════════════════════════════

  // Поточний тип друку (цінник / етикетка)
  const [printType, setPrintType] = useState<PrintType>('price_tag');

  // Вибраний конфіг типу
  const config = PRINT_TYPES[printType];

  // Вибраний товари
  const [selected, setSelected] = useState<SelectedProduct[]>([]);

  // Налаштування друку (ініціалізуємо з дефолтних значень конфігу)
  const [settings, setSettings] = useState<PrintSettings>({
    templateId: config.defaultSettings.templateId,
    widthMm: config.defaultSettings.widthMm,
    heightMm: config.defaultSettings.heightMm,
    gapMm: config.defaultSettings.gapMm,
    marginMm: config.defaultSettings.marginMm,
  });

  // Вибраний принтер
  const [printerName, setPrinterName] = useState<string>('');

  // Стан прев'ю та друку
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPrinting, setIsPrinting] = useState(false);
  const [renderResult, setRenderResult] = useState<{
    totalPages?: number;
    totalLabels: number;
  } | null>(null);

  // ═══ Print-as-Image через html2canvas (для label + Tauri) ═══
  const {
    receiptRef,
    captureAndPrint: captureAndPrintLabel,
    isCapturing,
  } = usePrintAsImage({ showErrors: true });

  // Активний стан друку (або через print, або через html2canvas)
  const isPrintingActive = isPrinting || isCapturing;

  // ═══════════════════════════════════════════════════════════
  // Завантажити збережений принтер при монтуванні
  // ═══════════════════════════════════════════════════════════
  useEffect(() => {
    const loadPrinter = async () => {
      try {
        const saved = await settingsService.getValue('printer_name');
        if (saved) setPrinterName(saved);
      } catch { /* ignore */ }
    };
    loadPrinter();
  }, []);

  // ═══════════════════════════════════════════════════════════
  // Перемикання типу друку
  // ═══════════════════════════════════════════════════════════
  const handleTypeChange = useCallback((newType: PrintType) => {
    if (newType === printType) return;

    const newConfig = PRINT_TYPES[newType];

    // Скидаємо прев'ю (при зміні типу це виправдано — новий шаблон, нові розміри)
    setPreviewHtml(null);
    setRenderResult(null);

    // Оновлюємо тип
    setPrintType(newType);

    // Оновлюємо налаштування за замовчуванням для нового типу
    setSettings({
      templateId: newConfig.defaultSettings.templateId,
      widthMm: newConfig.defaultSettings.widthMm,
      heightMm: newConfig.defaultSettings.heightMm,
      gapMm: newConfig.defaultSettings.gapMm,
      marginMm: newConfig.defaultSettings.marginMm,
    });
  }, [printType]);

  // ═══════════════════════════════════════════════════════════
  // Зміна принтера (зберігаємо в налаштуваннях)
  // ═══════════════════════════════════════════════════════════
  const handlePrinterChange = useCallback(async (name: string) => {
    setPrinterName(name);
    try {
      await settingsService.update('printer_name', name);
    } catch { /* ignore */ }
  }, []);

  // ═══════════════════════════════════════════════════════════
  // Операції з товарами
  // ═══════════════════════════════════════════════════════════
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

  // ═══════════════════════════════════════════════════════════
  // Налаштування
  // ═══════════════════════════════════════════════════════════
  const handleSettingsChange = useCallback(
    (field: string, value: string | number) => {
      setSettings((prev) => ({ ...prev, [field]: value }));
    },
    [],
  );

  // ═══════════════════════════════════════════════════════════
  // Генерація прев'ю (БЕЗ моргання)
  // ═══════════════════════════════════════════════════════════
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
    // ❌ НЕ скидаємо previewHtml — старе прев'ю залишається до оновлення
    // ❌ НЕ скидаємо renderResult

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
        });
        // ✅ Оновлюємо тільки після успіху
        setPreviewHtml(result.html);
        setRenderResult({
          totalPages: result.total_pages,
          totalLabels: result.total_labels,
        });
        toast.success(`Згенеровано ${result.total_labels} цінників на ${result.total_pages} стор.`);
      } else {
        const result = await printService.renderLabels({
          template_id: settings.templateId,
          products,
          width_mm: settings.widthMm,
          height_mm: settings.heightMm,
          gap_mm: settings.gapMm,
        });
        // ✅ Оновлюємо тільки після успіху
        setPreviewHtml(result.html);
        setRenderResult({ totalLabels: result.total_labels });
        toast.success(`Згенеровано ${result.total_labels} етикеток`);
      }
    } catch (err: any) {
      // ❌ При помилці теж не скидаємо — залишаємо старе прев'ю
      const msg = err?.response?.data?.detail || `Помилка генерації ${label}`;
      toast.error(msg);
    } finally {
      setIsPreviewLoading(false);
    }
  }, [settings, selected, printType, label]);

  // ═══════════════════════════════════════════════════════════
  // Друк
  // ═══════════════════════════════════════════════════════════
  const handlePrint = useCallback(async () => {
    if (!previewHtml) {
      toast.error("Спочатку згенеруйте прев'ю");
      return;
    }

    setIsPrinting(true);
    try {
      if (isTauri() && printType === 'label') {
        // ═══ Tauri + label: Print-as-Image через html2canvas → Rust → ESC/POS ═══
        // Передаємо вибраний принтер (або undefined для системного)
        await captureAndPrintLabel(printerName || undefined);
        toast.success('Етикетки відправлено на друк');
      } else if (isTauri() && printType === 'price_tag') {
        // ═══ Tauri + price_tag: window.print() (A4 — не термо) ═══
        printViaBrowser(previewHtml, {
          width: settings.widthMm,
          height: settings.heightMm,
          isA4: true,
        });
        toast.success('Цінники відправлено на друк');
      } else {
        // ═══ Браузер (будь-який тип): window.print() ═══
        printViaBrowser(previewHtml, {
          width: settings.widthMm,
          height: settings.heightMm,
          isA4: printType === 'price_tag',
        });
      }
    } catch (err: any) {
      toast.error(err?.message || 'Помилка друку');
    } finally {
      setIsPrinting(false);
    }
  }, [previewHtml, printType, settings, printerName, captureAndPrintLabel]);

  // ═══════════════════════════════════════════════════════════
  // Допоміжна функція: друк через браузер (window.print)
  // ═══════════════════════════════════════════════════════════
  function printViaBrowser(
    html: string,
    opts: { width: number; height: number; isA4: boolean },
  ) {
    const pageSize = opts.isA4
      ? 'A4'
      : `${opts.width}mm ${opts.height}mm`;

    const fullHtml = `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <title>Друк — Kasa POS</title>
  <style>
    @media print {
      @page { margin: 0; size: ${pageSize}; }
      body { margin: 0; padding: 0; }
    }
  </style>
</head>
<body>${html}</body>
</html>`;

    const printWindow = window.open('', '_blank');
    if (printWindow) {
      printWindow.document.write(fullHtml);
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
          {/* Кнопка генерації прев'ю */}
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

          {/* Кнопка друку */}
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

      {/* ── Перемикач типу друку ─────────────────────── */}
      <div className="flex items-center gap-2 mb-4 flex-shrink-0">
        {(Object.values(PRINT_TYPES) as Array<{ id: PrintType; label: string }>).map((type) => (
          <button
            key={type.id}
            onClick={() => handleTypeChange(type.id)}
            className={`
              px-4 py-2 rounded-lg text-sm font-medium transition-all duration-150
              ${
                printType === type.id
                  ? 'bg-primary-600 text-white shadow-sm ring-1 ring-primary-700'
                  : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600'
              }
            `}
            title={type.id === 'price_tag' ? 'Друк цінників на A4' : 'Друк етикеток на термопринтер'}
          >
            <span className="flex items-center gap-2">
              {type.id === 'price_tag' ? (
                <Tag className="w-4 h-4" />
              ) : (
                <Printer className="w-4 h-4" />
              )}
              {type.label}
            </span>
          </button>
        ))}
      </div>

      {/* Три колонки — CSS Grid з явними колонками */}
      <div className="flex-1 grid grid-cols-1 lg:grid-cols-[560px_340px_minmax(0,1fr)] gap-4 min-h-0 min-w-0">
        {/* ═══════ КОЛОНКА 1 — Вибір товарів (320px) ═══════ */}
        <div className="overflow-hidden card p-4 flex flex-col">
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

        {/* ═══════ КОЛОНКА 2 — Налаштування (320px) ═══════ */}
        <div className="overflow-hidden card p-4 space-y-4">
          <div>
            <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
              Налаштування
            </h2>
            <div className="mb-4">
              <PrinterSelector value={printerName} onChange={handlePrinterChange} />
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

        {/* ═══════ КОЛОНКА 3 — Прев'ю (minmax(0, 1fr) — решта простору) ═══════ */}
        <div className="overflow-hidden card p-4 flex flex-col">
          <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3 flex-shrink-0">
            Попередній перегляд
          </h2>
          <div className="flex-1 min-h-0 overflow-hidden">
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

      {/* ═══ Прихований div для html2canvas (тільки для label + Tauri) ═══ */}
      <div
        ref={receiptRef as React.RefObject<HTMLDivElement>}
        dangerouslySetInnerHTML={{ __html: previewHtml || '' }}
        style={{
          position: 'absolute',
          left: '-9999px',
          top: 0,
          width: previewHtml ? `${settings.widthMm * 3.78}px` : '1px',
          backgroundColor: 'white',
          color: 'black',
          zIndex: -1,
        }}
      />
    </div>
  );
};

export default PrintLabelsPriceTagsPage;
