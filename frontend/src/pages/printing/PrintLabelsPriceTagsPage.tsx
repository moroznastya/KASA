import React, { useState, useCallback, useEffect, useMemo } from 'react';
import { Printer, ArrowLeft, Eye, Loader2, Tag } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { printService } from '@/services/printService';
import { settingsService } from '@/services/settingsService';
import { printTemplateService } from '@/services/printTemplateService';
import { isTauri } from '@/hooks/useTauri';
import { printHtml } from '@/services/tauri/print';
import { usePrintAsImage } from '@/hooks/usePrintAsImage';
import PrintProductSelector from '@/components/printing/PrintProductSelector';
import PrintPreview from '@/components/printing/PrintPreview';
import PrintSettingsPanel from '@/components/printing/PrintSettingsPanel';
import PrinterSelector from '@/components/printing/PrinterSelector';
import { PRINT_TYPES } from '@/types/print';
import type { Product } from '@/types/product';
import type { SelectedProduct, PrintType, BarcodeType } from '@/types/print';
import toast from 'react-hot-toast';
import { extractBodyWithStyles } from '@/utils/printHtmlUtils';

// ── Тип для зручного onChange ────────────────────
interface PrintSettings {
  templateId: string;
  widthMm: number;
  heightMm: number;
  gapMm: number;
  marginMm: number;
}

/**
 * Єдині ключі system_settings (module='printing'):
 *
 *   price_tag: price_tag_width, price_tag_height, price_tag_gap,
 *              price_tag_margin, price_tag_template_id
 *   label:     label_width, label_height, label_gap, label_template_id
 *   спільне:   printer_name, barcode_type
 *
 * НЕ використовуємо застарілі print_width_mm / print_height_mm /
 * print_gap_mm / print_margin_mm / print_barcode_type / print_template_id.
 */

// ── Ключі налаштувань для збереження між сесіями ─
const SETTINGS_KEYS = {
  printerName: 'printer_name',
  barcodeType: 'barcode_type',
} as const;

/** Повертає ключі системних налаштувань для типу друку */
function getTypeSettingsKeys(type: PrintType) {
  const prefix = type === 'price_tag' ? 'price_tag' : 'label';
  return {
    templateId: `${prefix}_template_id`,
    widthMm: `${prefix}_width`,
    heightMm: `${prefix}_height`,
    gapMm: `${prefix}_gap`,
    marginMm: type === 'price_tag' ? 'price_tag_margin' : null,
  };
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
    return extractBodyWithStyles(previewHtml);
  }, [previewHtml]);

  // ── Завантаження збережених налаштувань типу (з БД) ────────────
  // Винесено в окрему функцію: викликається і при монтуванні, і при
  // перемиканні типу друку (щоб розміри НОВОГО типу перечитувались з БД,
  // а не залишались від попереднього типу).
  const loadTypeSettings = useCallback(async (type: PrintType) => {
    try {
      const keys = getTypeSettingsKeys(type);

      // Завантажуємо всі ключі типу + спільні
      const keysToLoad = [
        SETTINGS_KEYS.printerName,
        SETTINGS_KEYS.barcodeType,
        keys.templateId,
        keys.widthMm,
        keys.heightMm,
        keys.gapMm,
        ...(keys.marginMm ? [keys.marginMm] : []),
      ];
      const values = await Promise.all(
        keysToLoad.map((key) =>
          settingsService.getValue(key).then((v) => ({ key, value: v }))
        )
      );
      const map = Object.fromEntries(
        values.filter((v) => v.value !== null && v.value !== '').map((v) => [v.key, v.value!])
      );

      if (map[SETTINGS_KEYS.printerName]) setPrinterName(map[SETTINGS_KEYS.printerName]);

      // Тип штрих-коду — з перевіркою типу
      const savedBarcodeType = map[SETTINGS_KEYS.barcodeType];
      if (savedBarcodeType === 'qr' || savedBarcodeType === 'code128') {
        setBarcodeType(savedBarcodeType);
      }

      // Оновлюємо розміри значеннями з БД; якщо ключа немає — залишаємо дефолт типу
      setSettings((prev) => {
        const next = { ...prev };
        if (map[keys.templateId]) next.templateId = map[keys.templateId];
        if (map[keys.widthMm]) next.widthMm = Number(map[keys.widthMm]);
        if (map[keys.heightMm]) next.heightMm = Number(map[keys.heightMm]);
        if (map[keys.gapMm]) next.gapMm = Number(map[keys.gapMm]);
        if (keys.marginMm && map[keys.marginMm]) next.marginMm = Number(map[keys.marginMm]);
        return next;
      });
    } catch {
      // ignore — використовуємо дефолтні значення
    }
  }, []);

  // ── Завантаження збережених налаштувань при монтуванні ──
  useEffect(() => {
    loadTypeSettings(printType).finally(() => setSettingsLoaded(true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── Бонус: якщо template_id порожній — підставляємо дефолтний шаблон типу ──
  useEffect(() => {
    if (!settingsLoaded) return;
    if (settings.templateId) return;

    const loadDefaultTemplate = async () => {
      try {
        const defaultTemplate = await printTemplateService.getDefault(printType);
        if (defaultTemplate) {
          setSettings((prev) => ({ ...prev, templateId: defaultTemplate.id }));
        }
      } catch {
        // Ігноруємо
      }
    };
    loadDefaultTemplate();
  }, [settingsLoaded, settings.templateId, printType]);

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
      // Скидаємо templateId — підставиться дефолтний через useEffect
      // Розміри — НА ДЕФОЛТ НОВОГО типу (НЕ prev.widthMm || ... — інакше
      // старі розміри попереднього типу залишаються: цінник 40×43 замість
      // етикетки 58×56)
      setSettings({
        templateId: '',
        widthMm: newConfig.defaultSettings.widthMm,
        heightMm: newConfig.defaultSettings.heightMm,
        gapMm: newConfig.defaultSettings.gapMm,
        marginMm: newConfig.defaultSettings.marginMm,
      });
      // Перечитуємо налаштування нового типу з БД (label_width/label_height...)
      void loadTypeSettings(newType);
    },
    [printType, loadTypeSettings],
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
  // Автозберігаємо в system_settings (єдині ключі типу)
  const handleSettingsChange = useCallback(
    (field: string, value: string | number) => {
      setSettings((prev) => {
        const next = { ...prev, [field]: value };
        const keys = getTypeSettingsKeys(printType);
        const keyMap: Record<string, string | null> = {
          templateId: keys.templateId,
          widthMm: keys.widthMm,
          heightMm: keys.heightMm,
          gapMm: keys.gapMm,
          marginMm: keys.marginMm,
        };
        const settingsKey = keyMap[field];
        if (settingsKey) {
          saveSetting(settingsKey, String(value));
        }
        return next;
      });
    },
    [saveSetting, printType],
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

    console.log('[PREVIEW] generate-start |', {
      printType,
      templateId: settings.templateId,
      productsCount: selected.length,
      widthMm: settings.widthMm,
      heightMm: settings.heightMm,
      barcodeType,
      scrollYBefore: window.scrollY,
    });
    const startTime = Date.now();

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
          barcode_height_mm: 7,
        });
        console.log('[PREVIEW] response |', {
          htmlLen: result.html?.length,
          totalPages: result.total_pages,
          totalLabels: result.total_labels,
          timeMs: Date.now() - startTime,
          scrollY: window.scrollY,
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
          barcode_height_mm: 19,
        });
        console.log('[PREVIEW] response |', {
          htmlLen: result.html?.length,
          totalPages: (result as { total_pages?: number }).total_pages,
          totalLabels: result.total_labels,
          timeMs: Date.now() - startTime,
          scrollY: window.scrollY,
        });
        setPreviewHtml(result.html);
        setRenderResult({ totalLabels: result.total_labels });
        toast.success(`Згенеровано ${result.total_labels} етикеток`);
      }
    } catch (err: any) {
      console.error('[PREVIEW] error |', err);
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
      if (isTauri() && printType === 'price_tag') {
        // НАТИВНИЙ друк A4: системний діалог webkit2gtk (grid/SVG/page-break працюють)
        const result = await printHtml(previewHtml, printerName || undefined);
        if (result.success === false) {
          toast.error(result.message);
        } else {
          toast.success('Відправлено на друк');
        }
      } else if (isTauri() && printType === 'label') {
        // html2canvas → ESC/POS растр (термо, flex-рендер; CSS зберігається через extractBodyWithStyles)
        // Передаємо фізичні розміри етикетки (мм) — Rust масштабує PNG точно під них.
        // itemsSelector='.label-item': кожна етикетка знімається ОКРЕМИМ знімком →
        // розмір знімка = одна етикетка → Rust масштабує рівномірно (без сплющення по висоті)
        await captureAndPrintLabel(printerName || undefined, {
          widthMm: settings.widthMm,
          heightMm: settings.heightMm,
          itemsSelector: '.label-item',
        });
        toast.success('Відправлено на друк');
      } else {
        // Не Tauri → браузерний друк
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
      <div className="flex items-center justify-center h-[calc(100vh-7rem)]">
        <div className="text-gray-400 dark:text-gray-500">Завантаження налаштувань...</div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-[calc(100vh-7rem)]">
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
            disabled={isPrintingActive || !previewHtml || isPreviewLoading}
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
        className="flex-1 grid grid-cols-[380px_280px_minmax(0,1fr)] gap-4 min-h-0 min-w-0 overflow-x-auto"
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
        <div className="overflow-hidden card p-4 shrink-0 flex flex-col min-h-0">
          <div className="flex-1 min-h-0 overflow-y-auto">
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

            {/* Інфо: друкована область термопринтера (58мм → фактично 48мм, 384 dots @203dpi) */}
            {printType === 'label' && settings.widthMm > 48 && (
              <div className="text-sm text-amber-600 dark:text-amber-500 bg-amber-50 dark:bg-slate-100 rounded-lg p-2 mt-3">
                <span className="mr-1">ℹ️</span>
                Друкована область 58мм принтера — 48мм. Етикетка {settings.widthMm}×{settings.heightMm}мм
                друкується як 48×{settings.heightMm}мм (висота точна).
              </div>
            )}
          </div>
        </div>

        {/* Колонка 3 — Прев'ю */}
        <div className="overflow-hidden card p-4 flex flex-col min-w-0 min-h-0">
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

      {/* Прихований div для html2canvas — ТІЛЬКИ під час захоплення (термо-друк).
          position: fixed (НЕ absolute): fixed-елемент не впливає на scrollHeight
          документа → під час друку не з'являється скролбар → верстка не пливе.
          html2canvas компенсує left:-9999px через ctx.translate(-options.x),
          тому друк працює ідентично. */}
      {isCapturing && (
        <div
          ref={receiptRef as React.RefObject<HTMLDivElement>}
          dangerouslySetInnerHTML={{ __html: cleanHtmlForReceipt }}
          style={{
            position: 'fixed',
            left: '-9999px',
            top: 0,
            width: previewHtml ? 'fit-content' : '1px',
            minWidth: previewHtml ? `${settings.widthMm * 3.78}px` : '1px',
            backgroundColor: 'white',
            color: 'black',
            zIndex: -1,
          }}
        />
      )}
    </div>
  );
};

export default PrintLabelsPriceTagsPage;
