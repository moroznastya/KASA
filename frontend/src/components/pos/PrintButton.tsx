/**
 * Компонент кнопки друку
 *
 * Автоматично визначає оточення (Tauri / браузер):
 *   - В Tauri — друкує через системний принтер
 *   - В браузері — відкриває діалог window.print()
 *
 * Підтримує рендер через API шаблонів:
 *   - Якщо передано receipt + receiptTemplateId — спочатку рендерить шаблон через API
 */

import React, { useState, useCallback } from 'react';
import { Printer, Loader2 } from 'lucide-react';
import { useTauri, isTauri } from '@/hooks/useTauri';
import { printTemplateService } from '@/services/printTemplateService';
import type { Receipt, ReceiptItem } from '@/types/receipt';

interface PrintButtonProps {
  /** HTML-вміст для друку (якщо передано, друкує напряму) */
  content: string;
  /** Дані чеку — якщо передано з receiptTemplateId, рендерить шаблон перед друком */
  receipt?: Receipt;
  /** ID шаблону для рендеру (обов'язково з receipt) */
  receiptTemplateId?: string;
  /** Назва принтера (опціонально) */
  printerName?: string;
  /** Додаткові CSS-класи */
  className?: string;
  /** Текст кнопки */
  label?: string;
  /** Розмір кнопки */
  size?: 'sm' | 'md' | 'lg';
  /** Колірна схема */
  variant?: 'primary' | 'secondary' | 'outline';
  /** Викликається після успішного друку */
  onPrintSuccess?: () => void;
  /** Викликається при помилці друку */
  onPrintError?: (error: string) => void;
}

const sizeClasses = {
  sm: 'px-3 py-1.5 text-xs gap-1.5',
  md: 'px-4 py-2 text-sm gap-2',
  lg: 'px-6 py-3 text-base gap-2.5',
};

const variantClasses = {
  primary: 'bg-blue-600 text-white hover:bg-blue-700 focus:ring-blue-500',
  secondary: 'bg-gray-100 text-gray-700 hover:bg-gray-200 focus:ring-gray-400 dark:bg-gray-700 dark:text-gray-200',
  outline: 'border border-gray-300 text-gray-700 hover:bg-gray-50 focus:ring-gray-400 dark:border-gray-600 dark:text-gray-200',
};

/** Екранування HTML-спецсимволів */
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

/** Конвертація даних чеку в шаблонні змінні */
function receiptToRenderData(receipt: Receipt): Record<string, string> {
  const itemsHtml = receipt.items
    .map(
      (item: ReceiptItem) =>
        `<tr>
          <td style="text-align:left;">${escapeHtml(item.product_name)}</td>
          <td style="text-align:center;">${Number(item.quantity)}</td>
          <td style="text-align:right;">${Number(item.price).toFixed(2)}</td>
          <td style="text-align:right;">${Number(item.total).toFixed(2)}</td>
        </tr>`,
    )
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
    shop_name: '',
    shop_address: '',
    tax_id: '',
    receipt_number: receipt.receipt_number,
    date: dateStr,
    time: timeStr,
    cashier: receipt.cashier_name || '',
    items: itemsHtml,
    total: total.toFixed(2),
    payment_method: paymentLabel,
    paid: paid.toFixed(2),
    change: change.toFixed(2),
    footer: 'Дякуємо за покупку!',
  };
}

/** Друк HTML у новому вікні (для Tauri та браузера) */
function printViaNewWindow(html: string, printerName?: string): Promise<void> {
  return new Promise((resolve, reject) => {
    try {
      const printWindow = window.open('', '_blank');
      if (!printWindow) {
        reject(new Error('Блокувальник спливних вікон'));
        return;
      }
      printWindow.document.write(`
        <!DOCTYPE html>
        <html>
        <head>
          <meta charset="UTF-8">
          <title>Друк — Torgashka</title>
          <style>
            @media print {
              body { font-family: 'Courier New', monospace; font-size: 12px; }
              @page { margin: 5mm; }
            }
          </style>
        </head>
        <body>${html}</body>
        </html>
      `);
      printWindow.document.close();
      printWindow.focus();
      setTimeout(() => {
        printWindow.print();
        printWindow.close();
        resolve();
      }, 500);
    } catch (error) {
      reject(error);
    }
  });
}

export const PrintButton: React.FC<PrintButtonProps> = ({
  content,
  receipt,
  receiptTemplateId,
  printerName,
  className = '',
  label = 'Друк',
  size = 'md',
  variant = 'primary',
  onPrintSuccess,
  onPrintError,
}) => {
  const { isTauri: inTauri, printing } = useTauri();
  const [isBrowserPrinting, setIsBrowserPrinting] = useState(false);
  const [isRendering, setIsRendering] = useState(false);
  const [renderedHtml, setRenderedHtml] = useState<string | null>(null);

  const handlePrint = useCallback(async () => {
    // Якщо є чек і шаблон — рендеримо через API перед друком
    if (receipt && receiptTemplateId) {
      setIsRendering(true);
      try {
        const renderData = receiptToRenderData(receipt);
        const html = await printTemplateService.render(receiptTemplateId, renderData);
        setRenderedHtml(html);

        // Друк зрендереного HTML
        if (inTauri) {
          setIsBrowserPrinting(true);
          try {
            printViaNewWindow(html, printerName);
            onPrintSuccess?.();
          } catch (error) {
            onPrintError?.(
              error instanceof Error ? error.message : 'Помилка друку',
            );
          } finally {
            setIsBrowserPrinting(false);
          }
        } else {
          setIsBrowserPrinting(true);
          try {
            const printWindow = window.open('', '_blank');
            if (printWindow) {
              printWindow.document.write(`
                <!DOCTYPE html>
                <html>
                <head>
                  <meta charset="UTF-8">
                  <title>Друк — Torgashka</title>
                  <style>
                    @media print {
                      body { font-family: 'Courier New', monospace; font-size: 12px; }
                      @page { margin: 5mm; }
                    }
                  </style>
                </head>
                <body>${html}</body>
                </html>
              `);
              printWindow.document.close();
              printWindow.focus();
              await new Promise((resolve) => setTimeout(resolve, 500));
              printWindow.print();
              printWindow.close();
              onPrintSuccess?.();
            }
          } catch (error) {
            onPrintError?.(
              error instanceof Error ? error.message : 'Помилка друку',
            );
          } finally {
            setIsBrowserPrinting(false);
          }
        }
      } catch (error) {
        onPrintError?.(
          error instanceof Error ? error.message : 'Помилка рендеру шаблону',
        );
      } finally {
        setIsRendering(false);
      }
      return;
    }

    // Звичайний друк без рендеру
    const htmlToPrint = renderedHtml || content;

    if (inTauri) {
      setIsBrowserPrinting(true);
      try {
        printViaNewWindow(htmlToPrint, printerName);
        onPrintSuccess?.();
      } catch (error) {
        onPrintError?.(
          error instanceof Error ? error.message : 'Помилка друку',
        );
      } finally {
        setIsBrowserPrinting(false);
      }
    } else {
      setIsBrowserPrinting(true);
      try {
        const printWindow = window.open('', '_blank');
        if (printWindow) {
          printWindow.document.write(`
            <!DOCTYPE html>
            <html>
            <head>
              <meta charset="UTF-8">
              <title>Друк — Torgashka</title>
              <style>
                @media print {
                  body { font-family: 'Courier New', monospace; font-size: 12px; }
                  @page { margin: 5mm; }
                }
              </style>
            </head>
            <body>${htmlToPrint}</body>
            </html>
          `);
          printWindow.document.close();
          printWindow.focus();
          await new Promise((resolve) => setTimeout(resolve, 500));
          printWindow.print();
          printWindow.close();
          onPrintSuccess?.();
        }
      } catch (error) {
        onPrintError?.(
          error instanceof Error ? error.message : 'Помилка друку',
        );
      } finally {
        setIsBrowserPrinting(false);
      }
    }
  }, [inTauri, content, renderedHtml, receipt, receiptTemplateId, printerName, onPrintSuccess, onPrintError]);

  const isLoading = printing || isBrowserPrinting || isRendering;

  return (
    <button
      onClick={handlePrint}
      disabled={isLoading}
      className={`
        inline-flex items-center justify-center rounded-lg
        font-medium transition-all duration-200
        focus:outline-none focus:ring-2 focus:ring-offset-2
        disabled:opacity-50 disabled:cursor-not-allowed
        ${sizeClasses[size]}
        ${variantClasses[variant]}
        ${className}
      `}
      title={
        inTauri
          ? 'Друк через Tauri'
          : receiptTemplateId
            ? 'Друк через шаблон'
            : 'Друк через браузер'
      }
    >
      {isLoading ? (
        <Loader2 className="w-4 h-4 animate-spin" />
      ) : (
        <Printer className="w-4 h-4" />
      )}
      {isLoading
        ? isRendering
          ? 'Рендер...'
          : 'Друк...'
        : label}
    </button>
  );
};

/**
 * Індикатор середовища (Tauri / Browser)
 */
export const EnvironmentBadge: React.FC = () => {
  const enabled = isTauri();
  return (
    <span
      className={`
        inline-flex items-center px-2 py-0.5 rounded text-xs font-medium
        ${enabled
          ? 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200'
          : 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200'
        }
      `}
    >
      {enabled ? '🖥️ Tauri' : '🌐 Browser'}
    </span>
  );
};
