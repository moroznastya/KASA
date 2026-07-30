import { useCallback, useRef, useState } from 'react';
import html2canvas from 'html2canvas';
import { printImage } from '@/services/tauri/print';
import { isTauri } from '@/hooks/useTauri';

interface UsePrintAsImageOptions {
  /** Відображати повідомлення про помилки */
  showErrors?: boolean;
}

interface UsePrintAsImageReturn {
  /** Ref, який потрібно прикріпити до контейнера з чеком */
  receiptRef: React.RefObject<HTMLDivElement | null>;
  /** Захопити та надрукувати вміст receiptRef */
  captureAndPrint: (printerName?: string) => Promise<void>;
  /** Захопити та повернути Base64 (для прев'ю) */
  captureToBase64: () => Promise<string>;
  /** Чи йде захоплення зараз */
  isCapturing: boolean;
  /** Остання помилка */
  error: string | null;
}

/**
 * Хук для друку чека як зображення (Print-as-Image).
 *
 * Як працює:
 * 1. Компонент рендерить прихований div з ref={receiptRef}
 * 2. HTML чека знаходиться всередині цього div (dangerouslySetInnerHTML)
 * 3. html2canvas знімає скріншот div → Canvas
 * 4. Canvas конвертується в Base64 PNG
 * 5. Base64 надсилається в Tauri → Rust → ESC/POS растр → принтер
 *
 * Важливо: html2canvas потребує, щоб DOM-вузол БУВ у документі
 * і мав актуальний HTML-вміст. Переконайтеся, що receiptRef
 * прикріплено до div з контентом ПЕРЕД викликом captureAndPrint().
 */
export function usePrintAsImage(
  options: UsePrintAsImageOptions = {},
): UsePrintAsImageReturn {
  const receiptRef = useRef<HTMLDivElement | null>(null);
  const [isCapturing, setIsCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /**
   * Очікує, поки DOM оновиться після зміни стану React.
   * Використовує requestAnimationFrame + setTimeout для гарантії.
   */
  const waitForDomUpdate = useCallback(async (): Promise<void> => {
    return new Promise((resolve) => {
      requestAnimationFrame(() => {
        setTimeout(resolve, 50); // 50ms — достатньо для React batch update
      });
    });
  }, []);

  /**
   * Перевіряє, чи receiptRef має актуальний вміст.
   * Якщо div порожній або null — кидає помилку.
   */
  const ensureRefHasContent = useCallback((): HTMLDivElement => {
    const node = receiptRef.current;
    if (!node) {
      throw new Error(
        'receiptRef не прикріплено до DOM-вузла. ' +
        'Переконайтеся, що <div ref={receiptRef}> є в JSX.',
      );
    }

    // Перевіряємо, чи є хоч якийсь вміст
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

    return node;
  }, []);

  /** Захопити HTML в Base64 PNG */
  const captureToBase64 = useCallback(async (): Promise<string> => {
    // Чекаємо, поки DOM оновиться
    await waitForDomUpdate();

    // Перевіряємо, що ref має контент
    ensureRefHasContent();

    const node = receiptRef.current!;

    const canvas = await html2canvas(node, {
      scale: 2, // 2x для чіткості (краще для термодруку)
      useCORS: true,
      backgroundColor: '#ffffff',
      logging: false,
      onclone: () => {
        // Можна додати стилі для друку
      },
    });

    return canvas
      .toDataURL('image/png')
      .replace(/^data:image\/png;base64,/, '');
  }, [waitForDomUpdate, ensureRefHasContent]);

  /** Захопити та надрукувати */
  const captureAndPrint = useCallback(
    async (printerName?: string) => {
      if (!isTauri()) {
        // У браузері — друкуємо через window.print()
        window.print();
        return;
      }

      setIsCapturing(true);
      setError(null);

      try {
        const base64 = await captureToBase64();
        const result = await printImage(base64, printerName);

        if (!result.success) {
          throw new Error(result.message);
        }
      } catch (err) {
        const msg =
          err instanceof Error ? err.message : 'Невідома помилка';
        setError(msg);
        if (options.showErrors) {
          console.error('Print-as-Image error:', msg);
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
    isCapturing,
    error,
  };
}
