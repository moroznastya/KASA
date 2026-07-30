import { useCallback, useRef, useState } from 'react';
import html2canvas from 'html2canvas';
import { printImage, saveReceiptImage } from '@/services/tauri/print';
import { isTauri } from '@/hooks/useTauri';
import toast from 'react-hot-toast';

interface UsePrintAsImageOptions {
  showErrors?: boolean;
}

interface UsePrintAsImageReturn {
  receiptRef: React.RefObject<HTMLDivElement | null>;
  captureAndPrint: (printerName?: string) => Promise<void>;
  captureToBase64: () => Promise<string>;
  captureToDataUrl: () => Promise<string>;
  isCapturing: boolean;
  error: string | null;
}

function getTailwindColorResetCSS(): string {
  return [
    '--color-gray-50: #f9fafb;',
    '--color-gray-100: #f3f4f6;',
    '--color-gray-200: #e5e7eb;',
    '--color-gray-300: #d1d5db;',
    '--color-gray-400: #9ca3af;',
    '--color-gray-500: #6b7280;',
    '--color-gray-600: #4b5563;',
    '--color-gray-700: #374151;',
    '--color-gray-800: #1f2937;',
    '--color-gray-900: #111827;',
    '--color-slate-50: #f8fafc;',
    '--color-slate-100: #f1f5f9;',
    '--color-slate-200: #e2e8f0;',
    '--color-slate-300: #cbd5e1;',
    '--color-slate-400: #94a3b8;',
    '--color-slate-500: #64748b;',
    '--color-slate-600: #475569;',
    '--color-slate-700: #334155;',
    '--color-slate-800: #1e293b;',
    '--color-slate-900: #0f172a;',
    '--color-red-50: #fef2f2;',
    '--color-red-100: #fee2e2;',
    '--color-red-200: #fecaca;',
    '--color-red-300: #fca5a5;',
    '--color-red-400: #f87171;',
    '--color-red-500: #ef4444;',
    '--color-red-600: #dc2626;',
    '--color-red-700: #b91c1c;',
    '--color-red-800: #991b1b;',
    '--color-red-900: #7f1d1d;',
    '--color-green-50: #f0fdf4;',
    '--color-green-100: #dcfce7;',
    '--color-green-200: #bbf7d0;',
    '--color-green-300: #86efac;',
    '--color-green-400: #4ade80;',
    '--color-green-500: #22c55e;',
    '--color-green-600: #16a34a;',
    '--color-green-700: #15803d;',
    '--color-green-800: #166534;',
    '--color-green-900: #14532d;',
    '--color-blue-50: #eff6ff;',
    '--color-blue-100: #dbeafe;',
    '--color-blue-200: #bfdbfe;',
    '--color-blue-300: #93c5fd;',
    '--color-blue-400: #60a5fa;',
    '--color-blue-500: #3b82f6;',
    '--color-blue-600: #2563eb;',
    '--color-blue-700: #1d4ed8;',
    '--color-blue-800: #1e40af;',
    '--color-blue-900: #1e3a8a;',
    '--color-yellow-50: #fffbeb;',
    '--color-yellow-100: #fef3c7;',
    '--color-yellow-200: #fde68a;',
    '--color-yellow-300: #fcd34d;',
    '--color-yellow-400: #fbbf24;',
    '--color-yellow-500: #f59e0b;',
    '--color-yellow-600: #d97706;',
    '--color-yellow-700: #b45309;',
    '--color-yellow-800: #92400e;',
    '--color-yellow-900: #78350f;',
    '--color-white: #ffffff;',
    '--color-black: #000000;',
  ].join('\n');
}

function getPrintResetCSS(): string {
  return `:root {\n${getTailwindColorResetCSS()}}\n`;
}

/**
 * Логування розмірів DOM-вузла перед захопленням html2canvas.
 */
function logNodeDimensions(node: HTMLElement, label: string): void {
  const rect = node.getBoundingClientRect();
  console.log(`[Print] 📐 ${label} dimensions:`, {
    offsetWidth: node.offsetWidth,
    offsetHeight: node.offsetHeight,
    clientWidth: node.clientWidth,
    clientHeight: node.clientHeight,
    scrollWidth: node.scrollWidth,
    scrollHeight: node.scrollHeight,
    boundingRect: {
      width: rect.width.toFixed(1),
      height: rect.height.toFixed(1),
      top: rect.top.toFixed(1),
      left: rect.left.toFixed(1),
    },
    children: node.children.length,
    textLength: (node.textContent || '').trim().length,
  });
}

export function usePrintAsImage(
  options: UsePrintAsImageOptions = {},
): UsePrintAsImageReturn {
  const receiptRef = useRef<HTMLDivElement | null>(null);
  const [isCapturing, setIsCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const waitForDomUpdate = useCallback(async (): Promise<void> => {
    return new Promise((resolve) => {
      requestAnimationFrame(() => {
        setTimeout(resolve, 50);
      });
    });
  }, []);

  const ensureRefHasContent = useCallback((): HTMLDivElement => {
    const node = receiptRef.current;
    if (!node) {
      throw new Error(
        'receiptRef не прикріплено до DOM-вузла. ' +
        'Переконайтеся, що <div ref={receiptRef}> є в JSX.',
      );
    }
    const hasContent =
      node.children.length > 0 &&
      node.textContent !== null &&
      node.textContent.trim().length > 0;
    if (!hasContent) {
      throw new Error(
        'receiptRef порожній. ' +
        'Переконайтеся, що HTML чека вже згенеровано ' +
        'і знаходиться всередині <div ref={receiptRef}>.',
      );
    }
    // 📐 Логування розмірів перед захопленням
    logNodeDimensions(node, 'receiptRef');
    return node;
  }, []);

  const captureToBase64 = useCallback(async (): Promise<string> => {
    await waitForDomUpdate();
    ensureRefHasContent();
    const node = receiptRef.current!;
    try {
      // 📐 Логування розмірів безпосередньо перед html2canvas
      logNodeDimensions(node, 'PRE-html2canvas');

      const canvas = await html2canvas(node, {
        scale: 2,
        useCORS: true,
        backgroundColor: '#ffffff',
        logging: true,
        width: node.scrollWidth,
        height: node.scrollHeight,
        windowWidth: node.scrollWidth,
        windowHeight: node.scrollHeight,
        onclone: (clonedDoc) => {
          try {
            // 📐 Логування розмірів у клонованому документі
            const clonedReceipt = clonedDoc.querySelector('[data-print-receipt]');
            if (clonedReceipt) {
              const el = clonedReceipt as HTMLElement;
              console.log('[Print] 📐 cloned receipt dimensions:', {
                offsetWidth: el.offsetWidth,
                offsetHeight: el.offsetHeight,
                scrollWidth: el.scrollWidth,
                scrollHeight: el.scrollHeight,
              });
            }

            const resetStyle = clonedDoc.createElement('style');
            resetStyle.textContent = getPrintResetCSS();
            clonedDoc.head.appendChild(resetStyle);

            const receiptContainer = clonedDoc.querySelector('[data-print-receipt]');
            if (receiptContainer) {
              (receiptContainer as HTMLElement).style.color = '#000';
              (receiptContainer as HTMLElement).style.backgroundColor = '#fff';
              const allElements = receiptContainer.querySelectorAll('*');
              allElements.forEach((el) => {
                const htmlEl = el as HTMLElement;
                if (!htmlEl.style.color || htmlEl.style.color === '') {
                  htmlEl.style.color = '#000';
                }
                if (!htmlEl.style.backgroundColor || htmlEl.style.backgroundColor === '') {
                  htmlEl.style.backgroundColor = 'transparent';
                }
              });
            }
          } catch (err) {
            console.error('[Print] ❌ onclone: помилка:', err);
          }
        },
      });

      // 📐 Логування розмірів отриманого Canvas
      console.log('[Print] 📐 canvas dimensions:', {
        width: canvas.width,
        height: canvas.height,
        scale: 2,
        expectedWidth: node.scrollWidth * 2,
        expectedHeight: node.scrollHeight * 2,
      });

      const base64 = canvas
        .toDataURL('image/png')
        .replace(/^data:image\/png;base64,/, '');

      // 📐 Логування розміру Base64
      console.log('[Print] 📐 base64 PNG size:', {
        length: base64.length,
        estimatedBytes: Math.round(base64.length * 0.75),
        estimatedKB: (base64.length * 0.75 / 1024).toFixed(1),
      });

      return base64;
    } catch (err) {
      console.error('[Print] ❌ html2canvas помилка:', err);
      throw err;
    }
  }, [waitForDomUpdate, ensureRefHasContent]);

  const captureToDataUrl = useCallback(async (): Promise<string> => {
    const base64 = await captureToBase64();
    return `data:image/png;base64,${base64}`;
  }, [captureToBase64]);

  const captureAndPrint = useCallback(
    async (printerName?: string) => {
      if (!isTauri()) {
        window.print();
        return;
      }

      setIsCapturing(true);
      setError(null);

      try {
        const base64 = await captureToBase64();

        // 🖼️ Збереження PNG на диск
        try {
          const savedPath = await saveReceiptImage(base64);
          console.log('[Print] ✅ PNG збережено:', savedPath);
          toast.success(`📸 Чек збережено: ~/Downloads/${savedPath.split('/').pop()}`);
        } catch (saveErr) {
          const msg = saveErr instanceof Error ? saveErr.message : String(saveErr);
          console.error('[Print] ❌ ПОМИЛКА збереження PNG:', msg);
          toast.error(`❌ Не вдалося зберегти PNG: ${msg}`);
        }

        // 🖨️ Друк
        const result = await printImage(base64, printerName);
        if (!result.success) {
          throw new Error(result.message);
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : 'Невідома помилка';
        setError(msg);
        if (options.showErrors) {
          console.error('[Print-as-Image error]:', msg);
        }
        throw err;
      } finally {
        setIsCapturing(false);
      }
    },
    [captureToBase64, options.showErrors],
  );

  return {
    receiptRef,
    captureAndPrint,
    captureToBase64,
    captureToDataUrl,
    isCapturing,
    error,
  };
}
