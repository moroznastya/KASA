import { useState, useEffect, useCallback } from 'react';
import { printTemplateService } from '@/services/printTemplateService';
import { settingsService } from '@/services/settingsService';
import type { PrintTemplate } from '@/types/printTemplate';
import type { Receipt, ReceiptItem } from '@/types/receipt';
import { usePrintAsImage } from '@/hooks/usePrintAsImage';
import { isTauri } from '@/hooks/useTauri';

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

  return {
    shop_name: name,
    shop_address: address,
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
    footer: isReturn ? 'Повернення оформлено' : 'Дякуємо за покупку!',
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

  // ── Print-as-Image через html2canvas ──
  const {
    receiptRef,
    captureAndPrint: printAsImage,
  } = usePrintAsImage({ showErrors: true });

  // Завантаження налаштувань магазину та принтера
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
      } catch {
        // Ігноруємо
      }
    };
    loadSettings();
  }, []);

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

  useEffect(() => {
    if (templates.length > 0 && !selectedTemplate) {
      loadDefaultTemplate();
    }
  }, [templates, selectedTemplate, loadDefaultTemplate]);

  const selectTemplate = useCallback((id: string) => {
    const template = templates.find((t) => t.id === id) || null;
    setSelectedTemplate(template);
    setPreviewHtml(null);
  }, [templates]);

  // ── Генерація прев'ю HTML ─────────────────
  const generatePreview = useCallback(async () => {
    if (!selectedTemplate || !receipt) return;
    setIsPreviewLoading(true);
    setPreviewHtml(null);
    try {
      const renderData = receiptToRenderData(receipt, shopInfo);
      const html = await printTemplateService.render(selectedTemplate.id, renderData);
      setPreviewHtml(html);
    } catch {
      // Помилка
    } finally {
      setIsPreviewLoading(false);
    }
  }, [selectedTemplate, receipt, shopInfo]);

  // ── Отримати HTML чека (з кешу або новий) ─
  const getReceiptHtml = useCallback(async (): Promise<string> => {
    if (previewHtml) return previewHtml;

    if (!selectedTemplate) {
      throw new Error('Не вибрано шаблон для друку');
    }

    const renderData = receiptToRenderData(receipt!, shopInfo);
    return await printTemplateService.render(selectedTemplate.id, renderData);
  }, [previewHtml, selectedTemplate, receipt, shopInfo]);

  // ── Забезпечити HTML у DOM перед друком ────
  const ensureHtmlInDom = useCallback(async (): Promise<void> => {
    if (previewHtml) return;

    if (!selectedTemplate || !receipt) {
      throw new Error('Немає шаблону або даних чеку');
    }

    setIsPreviewLoading(true);
    try {
      const renderData = receiptToRenderData(receipt, shopInfo);
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
  }, [previewHtml, selectedTemplate, receipt, shopInfo]);

  // ─── Друк чеку ──────────────────────────────
  const printReceipt = useCallback(async () => {
    if (!receipt) throw new Error('Немає даних чеку для друку');
    setIsPrinting(true);

    try {
      if (isTauri()) {
        // ═══ ЄДИНИЙ ШЛЯХ: PRINT-AS-IMAGE (html2canvas → PNG → Rust) ═══
        await ensureHtmlInDom();
        await printAsImage(printerName);
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
  }, [receipt, printerName, ensureHtmlInDom, printAsImage, getReceiptHtml]);

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
  <title>Друк — Kasa POS</title>
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
