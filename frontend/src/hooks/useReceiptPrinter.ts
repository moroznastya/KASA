import { useState, useEffect, useCallback, createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { QRCodeSVG } from 'qrcode.react';
import { printTemplateService } from '@/services/printTemplateService';
import { settingsService } from '@/services/settingsService';
import type { PrintTemplate } from '@/types/printTemplate';
import type { Receipt, ReceiptItem } from '@/types/receipt';
import { usePrintAsImage } from '@/hooks/usePrintAsImage';
import { isTauri } from '@/hooks/useTauri';

// ── QR-код ───────────────────────────────────
/**
 * Згенерувати SVG data-URI QR-коду (для вбудовування в HTML-шаблон друку).
 * Повертає порожній рядок, якщо значення порожнє.
 */
export function generateQrCodeDataUri(value: string | null | undefined, size = 60): string {
  if (!value) return '';
  try {
    const svg = renderToStaticMarkup(
      createElement(QRCodeSVG, { value, size, level: 'M', includeMargin: false })
    );
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  } catch {
    return '';
  }
}

// ── Фіскальні реквізити (Фаза 3.8: QR ДПС) ──

/** Видобути ФН (фіскальний номер ПРРО) з URL перевірки чеку (параметр fn=...). */
function parseFiscalFn(checkUrl: string): string {
  if (!checkUrl) return '';
  try {
    return new URL(checkUrl).searchParams.get('fn') || '';
  } catch {
    return '';
  }
}

/** Відформатувати дату/час фіскалізації (fiscal_sent_at) у форматі чеку. */
function formatFiscalDateTime(value: string | null | undefined): string {
  if (!value) return '';
  const d = new Date(value);
  if (isNaN(d.getTime())) return '';
  const dateStr = d.toLocaleDateString('uk-UA');
  const timeStr = d.toLocaleTimeString('uk-UA', {
    hour: '2-digit',
    minute: '2-digit',
  });
  return `${dateStr} ${timeStr}`;
}

// ── Інтерфейси ───────────────────────────────
export interface UseReceiptPrinterOptions {
  receipt?: Receipt;
}

export interface UseReceiptPrinterReturn {
  templates: PrintTemplate[];
  selectedTemplate: PrintTemplate | null;
  selectTemplate: (id: string) => void;
  previewHtml: string | null;
  isPreviewLoading: boolean;
  isPrinting: boolean;
  generatePreview: () => Promise<void>;
  printReceipt: () => Promise<void>;
  loadDefaultTemplate: (type?: string) => Promise<void>;
  receiptRef: React.RefObject<HTMLDivElement | null>;
}

// ── Конвертація чеку в шаблонні змінні HTML ─
export function receiptToRenderData(
  receipt: Receipt,
  shopInfo?: { shop_name: string; shop_address: string; tax_id: string },
  extra?: Record<string, string>,
): Record<string, string> {
  const name = shopInfo?.shop_name || '';
  const address = shopInfo?.shop_address || '';
  const taxId = shopInfo?.tax_id || '';
  const isReturn = receipt.receipt_type === 'return';

  // ── Покращене вирівнювання: CSS Grid для кожного рядка ──
  const itemsHtml = receipt.items
    .map((item: ReceiptItem) => {
      const qty = Number(item.quantity);
      const price = Number(item.price);
      const total = Number(item.total);
      const barcode = item.product_barcode
        ? `<div style="font-size:9px; color:#555; letter-spacing:0.3px; font-weight:bold;">${escapeHtml(item.product_barcode)}</div>`
        : '';
      return `
<div style="margin-bottom:3px; ${barcode ? 'border-top:1px dotted #ccc; padding-top:3px;' : ''}">
  ${barcode}
  <div style="display:grid; grid-template-columns:1fr auto; gap:4px; align-items:start;">
    <div style="font-size:11px; line-height:1.3; font-weight:bold; word-break:break-word; overflow-wrap:break-word;">
      ${escapeHtml(item.product_name)}
    </div>
    <div style="font-size:11px; text-align:right; white-space:nowrap; font-weight:bold;">
      ${total.toFixed(2)}
    </div>
  </div>
  <div style="font-size:9px; color:#555;">
    ${qty} × ${price.toFixed(2)}
  </div>
</div>`;
    })
    .join('\n');

  const date = new Date(receipt.created_at);
  const dateStr = date.toLocaleDateString('uk-UA');
  const timeStr = date.toLocaleTimeString('uk-UA', {
    hour: '2-digit',
    minute: '2-digit',
  });

  const total = Number(receipt.total_amount);
  const paid = Number(receipt.paid_amount || 0);
  const change = Number(receipt.change_amount || 0);

  const paymentLabel =
    receipt.payment_method === 'card'
      ? 'Картка'
      : receipt.payment_method === 'mixed'
        ? 'Змішаний'
        : 'Готівка';

  // ── Фіскалізація: URL перевірки + QR-код (для шаблону 58мм) ──
  const fiscalCheckUrl = receipt.fiscal_check_url || '';
  const qrDataUri = generateQrCodeDataUri(fiscalCheckUrl);
  const fiscalNumber = receipt.fiscal_number || '';
  const fiscalStatus = receipt.fiscal_status || 'none';
  // ФН (фіскальний номер ПРРО) — беремо з параметра fn= у URL перевірки ДПС
  const fiscalFn = fiscalCheckUrl ? parseFiscalFn(fiscalCheckUrl) : '';
  // Дата/час фіскалізації (fiscal_sent_at)
  const fiscalDateTime = formatFiscalDateTime(receipt.fiscal_sent_at);

  // HTML-блок QR-коду для вставки в шаблон (порожній, якщо немає URL)
  const qrCodeHtml = qrDataUri
    ? `<div style="text-align:center; margin:4px 0;">
        <img src="${qrDataUri}" width="60" height="60" alt="QR" style="display:inline-block;" />
        <div style="font-size:8px; color:#555;">Для перевірки чеку</div>
      </div>`
    : '';

  // Повний фіскальний блок: реквізити (ФН, № фіскального чека, дата/час) + QR.
  // Порожній, якщо fiscal_check_url відсутній → звичайні (нефіскальні) чеки
  // друкуються БЕЗ QR та без будь-яких змін.
  const fiscalBlockHtml = fiscalCheckUrl
    ? `<div style="border-top:1px dashed #000; margin:6px 0 4px 0;"></div>
<div style="text-align:center; font-size:8px; line-height:1.5;">
  ${fiscalFn ? `<div>ФН: ${escapeHtml(fiscalFn)}</div>` : ''}
  ${fiscalNumber ? `<div>Фіскальний №: ${escapeHtml(fiscalNumber)}</div>` : ''}
  ${fiscalDateTime ? `<div>${escapeHtml(fiscalDateTime)}</div>` : ''}
  <div style="margin-top:4px;">
    <img src="${qrDataUri}" width="60" height="60" alt="QR" style="display:inline-block;" />
  </div>
  <div style="margin-top:2px;">Для перевірки чеку</div>
</div>`
    : '';

  return {
    shop_name: name,
    shop_address: address,
    tax_id: taxId,
    receipt_number: receipt.receipt_number,
    date: dateStr,
    time: timeStr,
    cashier: receipt.cashier_name || '',
    items: itemsHtml,
    total: total.toFixed(2),
    payment_method: paymentLabel,
    paid: paid.toFixed(2),
    change: change.toFixed(2),
    original_receipt_number: receipt.original_receipt_number || '',
    return_reason: receipt.return_reason || '',
    // ── Фіскальні змінні ──
    fiscal_check_url: fiscalCheckUrl,
    fiscal_number: fiscalNumber,
    fiscal_status: fiscalStatus,
    fiscal_fn: fiscalFn,
    fiscal_date_time: fiscalDateTime,
    // HTML-блок QR (img з data-URI) — шаблон може вставити {{qr_code}}
    qr_code: qrCodeHtml,
    // HTML-блок фіскальних реквізитів + QR — шаблон може вставити {{fiscal_block}}
    fiscal_block: fiscalBlockHtml,
    // ⚠️ footer НЕ передаємо — шаблони вже містять потрібний текст в HTML
    // footer: isReturn ? 'Повернення оформлено' : 'Дякуємо за покупку!',
    // Додаткові змінні (наприклад show_logo) — зливаємо зверху
    ...extra,
  };
}

function escapeHtml(text: string): string {
  const map: Record<string, string> = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#039;',
  };
  return text.replace(/[&<>"']/g, (c) => map[c] || c);
}

/**
 * Стилі для друку через браузер (window.print()).
 * Імітують термічний чек 58мм.
 */
const BROWSER_PRINT_STYLES = `
  @media print {
    @page {
      width: 58mm;
      margin: 0;
      padding: 0;
    }
    html, body {
      width: 58mm;
      margin: 0;
      padding: 0;
      font-family: 'Courier New', 'Consolas', monospace;
      font-size: 10px;
      line-height: 1.2;
      color: #000;
    }
    * {
      box-sizing: border-box;
    }
  }
`;

// ── Хук ──────────────────────────────────────
export function useReceiptPrinter(options: UseReceiptPrinterOptions = {}): UseReceiptPrinterReturn {
  const { receipt } = options;

  const [templates, setTemplates] = useState<PrintTemplate[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<PrintTemplate | null>(null);
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPrinting, setIsPrinting] = useState(false);
  const [shopInfo, setShopInfo] = useState<{ shop_name: string; shop_address: string; tax_id: string }>({
    shop_name: '',
    shop_address: '',
    tax_id: '',
  });
  const [printerName, setPrinterName] = useState<string | undefined>(undefined);
  const [defaultTemplateType, setDefaultTemplateType] = useState<string>('receipt_58mm');

  // ── Налаштування друку: копії, автовідрізання, логотип ──
  const [printCopies, setPrintCopies] = useState<number | null>(null);
  const [autoCutPaper, setAutoCutPaper] = useState<boolean | null>(null);
  const [showLogo, setShowLogo] = useState<boolean>(true);

  // ── Print-as-Image через html2canvas ──
  const {
    receiptRef,
    captureAndPrint: printAsImage,
  } = usePrintAsImage({ showErrors: true });

  // Завантаження налаштувань магазину, принтера та друку
  useEffect(() => {
    const loadSettings = async () => {
      try {
        const all = await settingsService.getAll();
        const modules = all.modules || {};
        const generalSettings = modules['general'] || [];
        const printingSettings = modules['printing'] || [];
        const getName = (key: string) => generalSettings.find((s: any) => s.key === key)?.value || '';
        setShopInfo({
          shop_name: getName('company_name'),
          shop_address: getName('company_address'),
          tax_id: getName('company_edrpou'),
        });
        const printer = printingSettings.find((s: any) => s.key === 'printer_name')?.value;
        if (printer) setPrinterName(printer);
        const templateType = printingSettings.find((s: any) => s.key === 'default_template_type')?.value;
        if (templateType) setDefaultTemplateType(templateType);

        // ── Копії чеків: print_copies (або receipt_print_copies) ──
        const copiesStr =
          printingSettings.find((s: any) => s.key === 'print_copies')?.value ||
          printingSettings.find((s: any) => s.key === 'receipt_print_copies')?.value;
        if (copiesStr) {
          const copies = parseInt(copiesStr, 10);
          if (!isNaN(copies) && copies > 0) setPrintCopies(copies);
        }

        // ── Автовідрізання паперу ──
        const autoCutStr = printingSettings.find((s: any) => s.key === 'auto_cut_paper')?.value;
        if (autoCutStr !== undefined && autoCutStr !== null && autoCutStr !== '') {
          setAutoCutPaper(autoCutStr === 'true' || autoCutStr === '1');
        }

        // ── Показ логотипу на чеку ──
        const showLogoStr = printingSettings.find((s: any) => s.key === 'show_logo')?.value;
        if (showLogoStr !== undefined && showLogoStr !== null && showLogoStr !== '') {
          setShowLogo(showLogoStr === 'true' || showLogoStr === '1');
        }
      } catch {
        // Ігноруємо
      }
    };
    loadSettings();
  }, []);

  // ═══════════════════════════════════════════════════════════════════════
  // Зміна 2: оновлення defaultTemplateType на основі receipt.receipt_type
  // ═══════════════════════════════════════════════════════════════════════
  useEffect(() => {
    const templateType = receipt?.receipt_type === 'return' ? 'return_receipt_58mm' : 'receipt_58mm';
    setDefaultTemplateType(templateType);
  }, [receipt?.receipt_type]);

  // Завантаження списку шаблонів
  useEffect(() => {
    loadTemplates();
  }, []);

  const loadTemplates = async () => {
    try {
      const list = await printTemplateService.getAll();
      setTemplates(list);
    } catch {
      setTemplates([]);
    }
  };

  const loadDefaultTemplate = useCallback(async (type?: string) => {
    const effectiveType = type || defaultTemplateType;
    try {
      const defaultTemplate = await printTemplateService.getDefault(effectiveType);
      if (defaultTemplate) {
        setSelectedTemplate(defaultTemplate);
      } else if (templates.length > 0) {
        const firstOfType = templates.find((t) => t.type === effectiveType);
        setSelectedTemplate(firstOfType || templates[0]);
      }
    } catch {
      if (templates.length > 0) {
        setSelectedTemplate(templates[0]);
      }
    }
  }, [templates, defaultTemplateType]);

  // ═══════════════════════════════════════════════════════════════════════
  // Зміна 1: автоматичний вибір шаблону при ініціалізації
  // ═══════════════════════════════════════════════════════════════════════
  useEffect(() => {
    if (templates.length > 0 && !selectedTemplate) {
      const templateType = receipt?.receipt_type === 'return' ? 'return_receipt_58mm' : 'receipt_58mm';
      loadDefaultTemplate(templateType);
    }
  }, [templates, selectedTemplate, loadDefaultTemplate, receipt?.receipt_type]);

  const selectTemplate = useCallback((id: string) => {
    const template = templates.find((t) => t.id === id) || null;
    setSelectedTemplate(template);
    setPreviewHtml(null);
  }, [templates]);

  // ── Додаткові змінні для рендеру (show_logo) ──
  const buildExtraRenderData = useCallback((): Record<string, string> => {
    return {
      show_logo: showLogo ? 'true' : 'false',
    };
  }, [showLogo]);

  // ── Генерація прев'ю HTML ─────────────────
  const generatePreview = useCallback(async () => {
    if (!selectedTemplate || !receipt) return;
    setIsPreviewLoading(true);
    setPreviewHtml(null);
    try {
      const renderData = {
        ...receiptToRenderData(receipt, shopInfo),
        ...buildExtraRenderData(),
      };
      const html = await printTemplateService.render(selectedTemplate.id, renderData);
      setPreviewHtml(html);
    } catch {
      // Помилка
    } finally {
      setIsPreviewLoading(false);
    }
  }, [selectedTemplate, receipt, shopInfo, buildExtraRenderData]);

  // ── Отримати HTML чека (з кешу або новий) ─
  const getReceiptHtml = useCallback(async (): Promise<string> => {
    if (previewHtml) return previewHtml;

    if (!selectedTemplate) {
      throw new Error('Не вибрано шаблон для друку');
    }

    const renderData = {
      ...receiptToRenderData(receipt!, shopInfo),
      ...buildExtraRenderData(),
    };
    return await printTemplateService.render(selectedTemplate.id, renderData);
  }, [previewHtml, selectedTemplate, receipt, shopInfo, buildExtraRenderData]);

  // ── Забезпечити HTML у DOM перед друком ────
  const ensureHtmlInDom = useCallback(async (): Promise<void> => {
    if (previewHtml) return;

    if (!selectedTemplate || !receipt) {
      throw new Error('Немає шаблону або даних чеку');
    }

    setIsPreviewLoading(true);
    try {
      const renderData = {
        ...receiptToRenderData(receipt, shopInfo),
        ...buildExtraRenderData(),
      };
      const html = await printTemplateService.render(selectedTemplate.id, renderData);
      setPreviewHtml(html);

      // ⏳ Чекаємо, поки React оновить DOM після setPreviewHtml
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          setTimeout(resolve, 100);
        });
      });
    } finally {
      setIsPreviewLoading(false);
    }
  }, [previewHtml, selectedTemplate, receipt, shopInfo, buildExtraRenderData]);

  // ─── Друк чеку ──────────────────────────────
  const printReceipt = useCallback(async () => {
    if (!receipt) throw new Error('Немає даних чеку для друку');
    setIsPrinting(true);

    try {
      if (isTauri()) {
        // ═══ ЄДИНИЙ ШЛЯХ: PRINT-AS-IMAGE (html2canvas → PNG → Rust) ═══
        // Передаємо копії та автовідрізання у printImage
        await ensureHtmlInDom();
        await printAsImage(printerName, {
          copies: printCopies,
          autoCut: autoCutPaper,
        });
      } else {
        // ═══ Браузер ═══
        const html = await getReceiptHtml();
        printViaBrowser(html);
      }
    } catch (err) {
      console.error('Помилка друку:', err);
      throw err;
    } finally {
      setIsPrinting(false);
    }
  }, [receipt, printerName, printCopies, autoCutPaper, ensureHtmlInDom, printAsImage, getReceiptHtml]);

  return {
    templates,
    selectedTemplate,
    selectTemplate,
    previewHtml,
    isPreviewLoading,
    isPrinting,
    generatePreview,
    printReceipt,
    loadDefaultTemplate,
    receiptRef,
  };
}

// ── Друк через браузер ───────────────────────
function printViaBrowser(html: string): void {
  const fullHtml = `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <title>Друк — Torgashka</title>
  <style>
    ${BROWSER_PRINT_STYLES}
    /* Друк на всю ширину: body точно 58mm */
    body {
      width: 58mm;
      margin: 0 auto;
      padding: 1mm 1.5mm;
      font-family: 'Courier New', 'Consolas', monospace;
      font-size: 10px;
      line-height: 1.2;
      color: #000;
    }
    /* Переноси для довгих назв */
    * {
      word-break: break-word;
      overflow-wrap: break-word;
    }
  </style>
</head>
<body>
  ${html}
</body>
</html>`;

  const printWindow = window.open('', '_blank');
  if (!printWindow) {
    const iframe = document.createElement('iframe');
    iframe.style.position = 'fixed';
    iframe.style.top = '-9999px';
    document.body.appendChild(iframe);
    const iframeDoc = iframe.contentWindow?.document;
    if (iframeDoc) {
      iframeDoc.write(fullHtml);
      iframeDoc.close();
      setTimeout(() => {
        iframe.contentWindow?.print();
        setTimeout(() => document.body.removeChild(iframe), 1000);
      }, 500);
    }
    return;
  }

  printWindow.document.write(fullHtml);
  printWindow.document.close();
  printWindow.focus();

  setTimeout(() => {
    printWindow.print();
  }, 500);
}
