import { useState, useEffect, useCallback } from 'react';
import { printTemplateService } from '@/services/printTemplateService';
import { settingsService } from '@/services/settingsService';
import type { PrintTemplate } from '@/types/printTemplate';
import type { Receipt, ReceiptItem } from '@/types/receipt';
import {
  isTauri,
  printDocument,
  printReceiptText,
  printReceiptHtml,
  printReceiptEscpos,
  type ReceiptPrintData,
} from '@/hooks/useTauri';

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
}

// ── НОВИЙ МЕТОД: Конвертація чеку в дані для прямого ESC/POS друку ────
//
// Збирає всі дані, які Rust потрібні для генерації ESC/POS байтів:
//   - Інформація про магазин (з налаштувань)
//   - Дані чеку (номер, дата, касир)
//   - Масив товарів зі штрих-кодами, назвами, кількістю, цінами
//   - Платіжна інформація
//
export function receiptToPrintData(
  receipt: Receipt,
  shopInfo: { shop_name: string; shop_address: string; tax_id: string },
): ReceiptPrintData {
  const date = new Date(receipt.created_at);
  const dateStr = date.toLocaleDateString('uk-UA');
  const timeStr = date.toLocaleTimeString('uk-UA', {
    hour: '2-digit',
    minute: '2-digit',
  });

  const paymentLabel =
    receipt.payment_method === 'card'
      ? 'Картка'
      : receipt.payment_method === 'mixed'
        ? 'Змішаний'
        : 'Готівка';

  const isReturn = receipt.receipt_type === 'return';

  return {
    shop_name: shopInfo.shop_name || '',
    shop_address: shopInfo.shop_address || '',
    tax_id: shopInfo.tax_id || '',
    receipt_number: receipt.receipt_number,
    date: dateStr,
    time: timeStr,
    cashier: receipt.cashier_name || '',
    items: receipt.items.map((item: ReceiptItem) => ({
      barcode: item.product_barcode || null,
      name: item.product_name || 'Товар',
      quantity: Number(item.quantity),
      price: Number(item.price),
      total: Number(item.total),
    })),
    total: Number(receipt.total_amount),
    payment_method: paymentLabel,
    paid: Number(receipt.paid_amount || 0),
    change: Number(receipt.change_amount || 0),
    footer: isReturn ? 'Повернення оформлено' : 'Дякуємо за покупку!',
  };
}

// ── СТАРИЙ МЕТОД (для сумісності з HTML-шаблонами) ────────────────────
//
// Конвертує дані чеку в шаблонні змінні HTML
//
export function receiptToRenderData(
  receipt: Receipt,
  shopInfo?: { shop_name: string; shop_address: string; tax_id: string },
): Record<string, string> {
  const name = shopInfo?.shop_name || '';
  const address = shopInfo?.shop_address || '';
  const taxId = shopInfo?.tax_id || '';
  const isReturn = receipt.receipt_type === 'return';

  const itemsHtml = receipt.items
    .map((item: ReceiptItem) => {
      const qty = Number(item.quantity);
      const price = Number(item.price);
      const total = Number(item.total);
      const barcode = item.product_barcode
        ? `<div style="font-size: 16px; color: #000; letter-spacing: 0.5px; font-weight: bold;">${escapeHtml(item.product_barcode)}</div>`
        : '';
      return `
<div style="margin-bottom: 2px;">
  ${barcode}
  <div style="font-size: 22px; line-height: 1.3; font-weight: bold;">${escapeHtml(item.product_name)}</div>
  <div style="font-size: 18px; color: #333; margin-left: 2px;">
    ${qty} × ${price.toFixed(2)} = ${total.toFixed(2)} грн
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

// ── Друк простим текстом (для старого методу) ─────────────────────────
export function receiptToPlainText(
  receipt: Receipt,
  width: number = 32,
  shopInfo?: { shop_name: string; shop_address: string; tax_id: string },
): string {
  const name = shopInfo?.shop_name || 'KASA SHOP';
  const address = shopInfo?.shop_address || '';
  const taxId = shopInfo?.tax_id || '';
  const isReturn = receipt.receipt_type === 'return';
  const date = new Date(receipt.created_at);
  const dateStr = date.toLocaleDateString('uk-UA');
  const timeStr = date.toLocaleTimeString('uk-UA', {
    hour: '2-digit',
    minute: '2-digit',
  });
  const cashier = receipt.cashier_name || '';
  const total = Number(receipt.total_amount);
  const paid = Number(receipt.paid_amount || 0);
  const change = Number(receipt.change_amount || 0);

  const paymentLabel =
    receipt.payment_method === 'card'
      ? 'Картка'
      : receipt.payment_method === 'mixed'
        ? 'Змішаний'
        : 'Готівка';

  const lines: string[] = [];

  // Шапка
  lines.push(centerText(name, width));
  if (address) lines.push(centerText(address, width));

  lines.push(separator(width));

  // Номер чеку, дата, касир
  lines.push(centerText(`ПОВЕРНЕННЯ № ${receipt.receipt_number}`, width));
  // Номер оригінального чеку (якщо є)
  if (isReturn && receipt.original_receipt_number) {
    lines.push(centerText(`(Ориг. чек №${receipt.original_receipt_number})`, width));
  }
  // Причина повернення (якщо є)
  if (isReturn && receipt.return_reason) {
    lines.push(centerText(`Причина: ${receipt.return_reason}`, width));
  }
  lines.push(`${dateStr}  ${timeStr}`);
  if (cashier) lines.push(`Касир: ${cashier}`);
  lines.push(separator(width));

  // Товари
  for (const item of receipt.items) {
    const qty = Number(item.quantity);
    const price = Number(item.price);
    const itemTotal = Number(item.total);
    const nameStr = item.product_name || 'Товар';
    const qtyStr = formatNumber(qty);
    const totalStr = itemTotal.toFixed(2);

    const maxNameLen = width - 14;
    const shortName = nameStr.length > maxNameLen
      ? nameStr.substring(0, maxNameLen - 1) + '…'
      : nameStr;

    const line = `${shortName.padEnd(maxNameLen)} ${qtyStr.padStart(4)}  ${totalStr.padStart(7)}`;
    lines.push(line);
  }

  lines.push(separator(width));

  // Підсумок
  if (isReturn) {
    lines.push(`СУМА ПОВЕРНЕННЯ:${total.toFixed(2).padStart(width - 16)}`);
  } else {
    lines.push(`СУМА:${total.toFixed(2).padStart(width - 5)}`);
  }
  if (paid > 0) lines.push(`Оплата (${paymentLabel}):${paid.toFixed(2).padStart(width - 20)}`);
  if (change > 0) lines.push(`Решта:${change.toFixed(2).padStart(width - 6)}`);
  lines.push(separator(width));
  lines.push(centerText(isReturn ? 'Повернення оформлено' : 'Дякуємо за покупку!', width));

  return lines.join('\n');
}

// ── Допоміжні функції ────────────────────────
function centerText(text: string, width: number): string {
  if (text.length >= width) return text;
  const padding = Math.floor((width - text.length) / 2);
  return ' '.repeat(padding) + text;
}

function separator(width: number, char: string = '─'): string {
  return char.repeat(width);
}

function formatNumber(n: number): string {
  return n % 1 === 0 ? n.toString() : n.toFixed(3);
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
  const [devicePath, setDevicePath] = useState<string | undefined>(undefined);
  const [defaultTemplateType, setDefaultTemplateType] = useState<string>('receipt_58mm');

  // Завантаження налаштувань магазину, принтера, порту та типу шаблону
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
        // Назва принтера
        const printer = printingSettings.find((s: any) => s.key === 'printer_name')?.value;
        if (printer) setPrinterName(printer);
        // Шлях до порту принтера
        const port = printingSettings.find((s: any) => s.key === 'printer_port')?.value;
        if (port) setDevicePath(port);
        // Тип шаблону
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

  // Генерація прев'ю (HTML для візуального перегляду)
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

  // ─── Друк чеку ───
  const printReceipt = useCallback(async () => {
    if (!receipt) {
      throw new Error('Немає даних чеку для друку');
    }

    setIsPrinting(true);

    try {
      if (isTauri()) {
        // ─── ДРУК ЧЕРЕЗ HTML ШАБЛОН: Chrome headless → PNG → ESC/POS ──
        // Цей режим підтримує повний стиль шаблону (жирний, вирівнювання,
        // різні розміри, логотип) та кирилицю
        const effectiveTemplateType = receipt.receipt_type === 'return' ? 'return_receipt_58mm' : defaultTemplateType;
        const selectedTemplateForType = receipt.receipt_type === 'return'
          ? await printTemplateService.getDefault('return_receipt_58mm')
          : null;
        const finalTemplate = selectedTemplateForType || selectedTemplate;

        if (!finalTemplate) {
          throw new Error('Не вибрано шаблон для друку');
        }

        let html = previewHtml;
        if (!html) {
          const renderData = receiptToRenderData(receipt, shopInfo);
          html = await printTemplateService.render(finalTemplate.id, renderData);
        }

        const result = await printReceiptHtml(html, printerName);
        if (!result.success) {
          throw new Error(result.message);
        }
      } else {
        // ─── Браузер: друк HTML через window.print() ───────────
        if (!selectedTemplate) {
          throw new Error('Не вибрано шаблон для друку');
        }

        let html = previewHtml;
        if (!html) {
          const renderData = receiptToRenderData(receipt, shopInfo);
          html = await printTemplateService.render(selectedTemplate.id, renderData);
        }

        printViaBrowser(html);
      }
    } catch (err) {
      console.error('Помилка друку:', err);
      throw err;
    } finally {
      setIsPrinting(false);
    }
  }, [selectedTemplate, receipt, previewHtml, shopInfo, printerName, devicePath]);

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
  };
}

// ── Друк через браузер ───────────────────────
function printViaBrowser(html: string): void {
  const printWindow = window.open('', '_blank');
  if (!printWindow) {
    const iframe = document.createElement('iframe');
    iframe.style.position = 'fixed';
    iframe.style.top = '-9999px';
    document.body.appendChild(iframe);
    const iframeDoc = iframe.contentWindow?.document;
    if (iframeDoc) {
      iframeDoc.write(`
        <!DOCTYPE html>
        <html>
        <head>
          <meta charset="UTF-8">
          <title>Друк — Kasa POS</title>
        </head>
        <body>${html}</body>
        </html>
      `);
      iframeDoc.close();
      setTimeout(() => {
        iframe.contentWindow?.print();
        setTimeout(() => document.body.removeChild(iframe), 1000);
      }, 500);
    }
    return;
  }

  printWindow.document.write(`
    <!DOCTYPE html>
    <html>
    <head>
      <meta charset="UTF-8">
      <title>Друк — Kasa POS</title>
    </head>
    <body>${html}</body>
    </html>
  `);
  printWindow.document.close();
  printWindow.focus();

  setTimeout(() => {
    printWindow.print();
  }, 500);
}
