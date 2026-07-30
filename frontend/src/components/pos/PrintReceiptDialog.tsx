import React, { useEffect, useRef, useState } from 'react';
import {
  Printer,
  X,
  Loader2,
  FileText,
  Banknote,
  CreditCard,
  ShoppingCart,
  CheckCircle2,
  AlertTriangle,
  RotateCcw,
} from 'lucide-react';
import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { useReceiptPrinter, receiptToRenderData } from '@/hooks/useReceiptPrinter';
import type { Receipt } from '@/types/receipt';
import { formatCurrency } from '@/utils/format';
import { printTemplateService } from '@/services/printTemplateService';
import { toast } from 'react-hot-toast';
import { isTauri } from '@/hooks/useTauri';

// ── Пропси ───────────────────────────────────
interface PrintReceiptDialogProps {
  isOpen: boolean;
  onClose: () => void;
  receipt: Receipt;
  autoPrint?: boolean;       // якщо true — друкувати одразу при відкритті
  onPrinted?: () => void;    // колбек після успішного друку
}

// ── Компонент ────────────────────────────────
const PrintReceiptDialog: React.FC<PrintReceiptDialogProps> = ({
  isOpen,
  onClose,
  receipt,
  autoPrint = false,
  onPrinted,
}) => {
  const {
    selectedTemplate,
    previewHtml,
    isPreviewLoading,
    isPrinting,
    generatePreview,
    printReceipt,
    loadDefaultTemplate,
    receiptRef,               // ✅ Додано: ref для html2canvas
  } = useReceiptPrinter({ receipt });

  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [autoPrintDone, setAutoPrintDone] = useState(false);
  const [printError, setPrintError] = useState<string | null>(null);

  // Визначаємо чи це чек повернення
  const isReturnReceipt = receipt.receipt_type === 'return';

  // Скидаємо стан при відкритті
  useEffect(() => {
    if (isOpen) {
      setAutoPrintDone(false);
      setPrintError(null);
    }
  }, [isOpen]);

  // Оновлюємо iframe при зміні previewHtml
  useEffect(() => {
    if (previewHtml && iframeRef.current) {
      iframeRef.current.srcdoc = previewHtml;
    }
  }, [previewHtml]);

  // Автоматично завантажуємо дефолтний шаблон
  useEffect(() => {
    if (isOpen) {
      loadDefaultTemplate();
    }
  }, [isOpen, loadDefaultTemplate]);

  // Генеруємо прев'ю після завантаження шаблону
  useEffect(() => {
    if (isOpen && selectedTemplate && !previewHtml && !isPreviewLoading) {
      generatePreview();
    }
  }, [isOpen, selectedTemplate, previewHtml, isPreviewLoading, generatePreview]);

  // Автоматичний друк — тільки для звичайних чеків (НЕ для повернення)
  useEffect(() => {
    if (isOpen && autoPrint && !isReturnReceipt && selectedTemplate && previewHtml && !autoPrintDone && !isPrinting) {
      setAutoPrintDone(true);
      const doPrint = async () => {
        try {
          await printReceipt();
          toast.success('Чек надруковано');
          onPrinted?.();
          onClose();
        } catch (err) {
          const msg = err instanceof Error ? err.message : 'Невідома помилка друку';
          setPrintError(msg);
          toast.error(msg);
        }
      };
      doPrint();
    }
  }, [isOpen, autoPrint, isReturnReceipt, selectedTemplate, previewHtml, autoPrintDone, isPrinting, printReceipt, onPrinted, onClose]);

  // Друк (ручний)
  const handlePrint = async () => {
    setPrintError(null);
    try {
      await printReceipt();
      toast.success(isReturnReceipt ? 'Чек повернення відправлено на друк' : 'Чек відправлено на друк');
      onPrinted?.();
      onClose();
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Невідома помилка друку';
      setPrintError(msg);
      toast.error(msg);
    }
  };

  // Інформація про оплату
  const paymentLabel =
    receipt.payment_method === 'cash'
      ? 'Готівка'
      : receipt.payment_method === 'card'
      ? 'Картка'
      : receipt.payment_method === 'mixed'
      ? 'Змішаний'
      : 'Готівка';

  const paymentIcon =
    receipt.payment_method === 'card' ? (
      <CreditCard className="w-4 h-4" />
    ) : (
      <Banknote className="w-4 h-4" />
    );

  // Якщо авто-друк і процес триває — показуємо мінімальний індикатор
  // (для звичайних чеків, не для повернення)
  if (!isReturnReceipt && autoPrint && (isPrinting || isPreviewLoading || !previewHtml) && !printError) {
    return (
      <Modal isOpen={isOpen} onClose={onClose} title="" size="sm" showCloseButton={false}>
        {/* ✅ Прихований контейнер для html2canvas */}
        <div ref={receiptRef} style={{ position: 'absolute', left: '-9999px', top: 0, width: '58mm' }}>
          <div dangerouslySetInnerHTML={{ __html: previewHtml || '' }} />
        </div>
        <div className="flex flex-col items-center justify-center py-8 gap-3">
          <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
          <p className="text-sm text-gray-500">
            {isPreviewLoading ? 'Генерація чеку...' : 'Друк...'}
          </p>
          <p className="text-xs text-gray-400">
            Чек №{receipt.receipt_number} — {formatCurrency(Number(receipt.total_amount))} грн
          </p>
        </div>
      </Modal>
    );
  }

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="" size="lg" showCloseButton={false}>
      <div className="flex flex-col min-h-[300px]">
        {/* ── Заголовок ─────────────────────── */}
        <div className={`flex items-center justify-between pb-4 border-b ${
          isReturnReceipt
            ? 'border-danger-200 dark:border-danger-700'
            : 'border-gray-200 dark:border-slate-700'
        }`}>
          <div className="flex items-center gap-3">
            <div className={`w-10 h-10 rounded-xl flex items-center justify-center ${
              isReturnReceipt
                ? 'bg-danger-100 dark:bg-danger-900/30'
                : 'bg-success-100 dark:bg-success-900/30'
            }`}>
              {isReturnReceipt ? (
                <RotateCcw className="w-5 h-5 text-danger-600 dark:text-danger-400" />
              ) : (
                <CheckCircle2 className="w-5 h-5 text-success-600 dark:text-success-400" />
              )}
            </div>
            <div>
              <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                {isReturnReceipt
                  ? `ЧЕК ПОВЕРНЕННЯ №${receipt.receipt_number}`
                  : `Чек №${receipt.receipt_number} створено`
                }
              </h2>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                {isReturnReceipt ? 'Повернення оформлено' : `${formatCurrency(Number(receipt.total_amount))} грн — ${paymentLabel}`}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* ── Основний контент ──────────────── */}
        <div className="py-4 space-y-4">
          {/* ✅ Прихований контейнер для html2canvas (для Print-as-Image) */}
          <div ref={receiptRef} style={{ position: 'absolute', left: '-9999px', top: 0, width: '58mm' }}>
            <div dangerouslySetInnerHTML={{ __html: previewHtml || '' }} />
          </div>

          {/* Інформація про чек */}
          <div className={`rounded-lg p-4 ${
            isReturnReceipt
              ? 'bg-danger-50 dark:bg-danger-900/20 border border-danger-200 dark:border-danger-800'
              : 'bg-gray-50 dark:bg-slate-700/50'
          }`}>
            <div className="space-y-1.5 text-sm">
              <div className="flex justify-between">
                <span className="text-gray-500 dark:text-gray-400">Номер:</span>
                <span className="font-medium text-gray-900 dark:text-gray-100">
                  {receipt.receipt_number}
                </span>
              </div>

              {/* Номер оригінального чеку (для повернення) */}
              {isReturnReceipt && receipt.original_receipt_number && (
                <div className="flex justify-between">
                  <span className="text-gray-500 dark:text-gray-400">Оригінальний чек:</span>
                  <span className="font-medium text-gray-900 dark:text-gray-100">
                    №{receipt.original_receipt_number}
                  </span>
                </div>
              )}

              {/* Причина повернення */}
              {isReturnReceipt && receipt.return_reason && (
                <div className="flex justify-between">
                  <span className="text-gray-500 dark:text-gray-400">Причина:</span>
                  <span className="font-medium text-gray-900 dark:text-gray-100">
                    {receipt.return_reason}
                  </span>
                </div>
              )}

              <div className="flex justify-between">
                <span className="text-gray-500 dark:text-gray-400">Позицій:</span>
                <span className="font-medium text-gray-900 dark:text-gray-100">
                  {receipt.items.length}
                </span>
              </div>

              {/* Сума — для повернення червоним кольором */}
              <div className={`flex justify-between text-base font-semibold pt-1.5 border-t ${
                isReturnReceipt
                  ? 'border-danger-200 dark:border-danger-700'
                  : 'border-gray-200 dark:border-slate-600'
              }`}>
                <span className="text-gray-700 dark:text-gray-300">Сума:</span>
                <span className={
                  isReturnReceipt
                    ? 'text-danger-600 dark:text-danger-400'
                    : 'text-primary-600 dark:text-primary-400'
                }>
                  {isReturnReceipt
                    ? `-${formatCurrency(Number(receipt.total_amount))}`
                    : formatCurrency(Number(receipt.total_amount))
                  }
                </span>
              </div>

              <div className="flex justify-between items-center">
                <span className="text-gray-500 dark:text-gray-400">Оплата:</span>
                <div className="flex items-center gap-1.5">
                  {paymentIcon}
                  <span className="font-medium text-gray-900 dark:text-gray-100">
                    {paymentLabel}
                  </span>
                </div>
              </div>
              {Number(receipt.paid_amount) > 0 && (
                <div className="flex justify-between">
                  <span className="text-gray-500 dark:text-gray-400">Сплачено:</span>
                  <span className={`font-medium ${
                    isReturnReceipt
                      ? 'text-danger-600 dark:text-danger-400'
                      : 'text-success-600 dark:text-success-400'
                  }`}>
                    {formatCurrency(Number(receipt.paid_amount))}
                  </span>
                </div>
              )}
              {Number(receipt.change_amount) > 0 && (
                <div className="flex justify-between">
                  <span className="text-gray-500 dark:text-gray-400">Решта:</span>
                  <span className="font-medium text-gray-900 dark:text-gray-100">
                    {formatCurrency(Number(receipt.change_amount))}
                  </span>
                </div>
              )}
              {receipt.cashier_name && (
                <div className={`flex justify-between pt-1.5 border-t ${
                  isReturnReceipt
                    ? 'border-danger-200 dark:border-danger-700'
                    : 'border-gray-200 dark:border-slate-600'
                }`}>
                  <span className="text-gray-500 dark:text-gray-400">Касир:</span>
                  <span className="font-medium text-gray-900 dark:text-gray-100">
                    {receipt.cashier_name}
                  </span>
                </div>
              )}
            </div>
          </div>

          {/* Назва шаблону (інформаційно) */}
          {selectedTemplate && (
            <div className="flex items-center gap-2 text-xs text-gray-400">
              <FileText className="w-3.5 h-3.5" />
              <span>Шаблон: {selectedTemplate.name}</span>
              {selectedTemplate.is_default && (
                <Badge variant="primary" size="sm">Основний</Badge>
              )}
            </div>
          )}

          {/* Прогрес генерації/друку */}
          {(isPreviewLoading || isPrinting) && !printError && (
            <div className="flex items-center justify-center gap-2 py-3 text-sm text-gray-500">
              <Loader2 className="w-4 h-4 animate-spin" />
              <span>
                {isPreviewLoading ? 'Генерація чеку...' : 'Друк...'}
              </span>
            </div>
          )}

          {/* Помилка друку */}
          {printError && (
            <div className="flex items-start gap-2 p-3 rounded-lg bg-danger-50 dark:bg-danger-900/20 border border-danger-200 dark:border-danger-800">
              <AlertTriangle className="w-4 h-4 text-danger-500 mt-0.5 shrink-0" />
              <div className="text-sm text-danger-700 dark:text-danger-300">
                <p className="font-medium">Помилка друку:</p>
                <p className="mt-0.5 text-xs opacity-80">{printError}</p>
              </div>
            </div>
          )}
        </div>

        {/* ── Нижня частина — кнопки ────────── */}
        <div className={`flex items-center justify-between pt-4 border-t ${
          isReturnReceipt
            ? 'border-danger-200 dark:border-danger-700'
            : 'border-gray-200 dark:border-slate-700'
        }`}>
          <div className="flex items-center gap-2 text-xs text-gray-400">
            {isTauri() ? (
              <Badge variant="success" size="sm">🖥️ Tauri</Badge>
            ) : (
              <Badge variant="warning" size="sm">🌐 Browser</Badge>
            )}
          </div>
          <div className="flex items-center gap-3">
            <Button variant="secondary" onClick={onClose} disabled={isPrinting}>
              {printError ? 'Закрити' : isReturnReceipt ? 'Не друкувати' : 'Не друкувати'}
            </Button>
            {!printError && (
              <Button
                onClick={handlePrint}
                disabled={isPrinting || isPreviewLoading || !selectedTemplate}
                size="lg"
                variant={isReturnReceipt ? 'danger' : 'primary'}
                icon={
                  isPrinting ? (
                    <Loader2 className="w-5 h-5 animate-spin" />
                  ) : (
                    <Printer className="w-5 h-5" />
                  )
                }
              >
                {isPrinting
                  ? 'Друк...'
                  : isReturnReceipt
                    ? 'Друкувати чек повернення'
                    : 'Друкувати чек'
                }
              </Button>
            )}
            {printError && (
              <Button
                variant="primary"
                onClick={handlePrint}
                size="lg"
                icon={<Printer className="w-5 h-5" />}
              >
                Спробувати знову
              </Button>
            )}
          </div>
        </div>
      </div>
    </Modal>
  );
};

export default PrintReceiptDialog;
