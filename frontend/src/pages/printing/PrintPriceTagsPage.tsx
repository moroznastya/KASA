import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Printer, ArrowLeft, Eye, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { printService } from '@/services/printService';
import { isTauri } from '@/hooks/useTauri';
import PrintProductSelector from '@/components/printing/PrintProductSelector';
import PrintPreview from '@/components/printing/PrintPreview';
import PrintSettingsPanel from '@/components/printing/PrintSettingsPanel';
import type { Product } from '@/types/product';
import type { SelectedProduct } from '@/types/print';
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
const PrintPriceTagsPage: React.FC = () => {
  const navigate = useNavigate();

  // Стан вибраних товарів
  const [selected, setSelected] = useState<SelectedProduct[]>([]);

  // Налаштування друку
  const [settings, setSettings] = useState<PrintSettings>({
    templateId: '',
    widthMm: 40,
    heightMm: 25,
    gapMm: 3,
    marginMm: 10,
  });

  // Стан прев'ю та друку
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [isPrinting, setIsPrinting] = useState(false);
  const [renderResult, setRenderResult] = useState<{
    totalPages: number;
    totalLabels: number;
  } | null>(null);

  // Додати товар
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

  // Видалити товар
  const handleRemove = useCallback((id: string) => {
    setSelected((prev) => prev.filter((item) => item.id !== id));
  }, []);

  // Оновити кількість копій
  const handleUpdateCopies = useCallback((id: string, copies: number) => {
    setSelected((prev) =>
      prev.map((item) => (item.id === id ? { ...item, copies } : item))
    );
  }, []);

  // Зміна налаштувань
  const handleSettingsChange = useCallback(
    (field: string, value: string | number) => {
      setSettings((prev) => ({ ...prev, [field]: value }));
    },
    []
  );

  // Генерація прев'ю
  const handleGeneratePreview = useCallback(async () => {
    if (!settings.templateId) {
      toast.error('Виберіть шаблон цінника');
      return;
    }
    if (selected.length === 0) {
      toast.error('Додайте хоча б один товар');
      return;
    }

    setIsPreviewLoading(true);
    setPreviewHtml(null);
    setRenderResult(null);

    try {
      const products = selected.map((item) => ({
        id: item.id,
        title: item.title,
        price: item.price,
        barcode: item.barcode,
        article: item.sku || undefined,
        copies: item.copies,
      }));

      const result = await printService.renderPriceTags({
        template_id: settings.templateId,
        products,
        width_mm: settings.widthMm,
        height_mm: settings.heightMm,
        gap_mm: settings.gapMm,
        margin_mm: settings.marginMm,
      });

      setPreviewHtml(result.html);
      setRenderResult({
        totalPages: result.total_pages,
        totalLabels: result.total_labels,
      });
      toast.success(`Згенеровано ${result.total_labels} цінників на ${result.total_pages} стор.`);
    } catch (err: any) {
      const msg = err?.response?.data?.detail || 'Помилка генерації цінників';
      toast.error(msg);
    } finally {
      setIsPreviewLoading(false);
    }
  }, [settings, selected]);

  // Друк
  const handlePrint = useCallback(async () => {
    if (!previewHtml) {
      toast.error('Спочатку згенеруйте прев\'ю');
      return;
    }

    setIsPrinting(true);
    try {
      if (isTauri()) {
        // Tauri — друкуємо через нове вікно (як у браузері)
        const printWindow = window.open('', '_blank');
        if (printWindow) {
          printWindow.document.write(`
            <!DOCTYPE html>
            <html>
            <head>
              <meta charset="UTF-8">
              <title>Друк цінників — Kasa POS</title>
              <style>
                @media print {
                  @page { margin: 0; size: A4; }
                  body { margin: 0; padding: 0; }
                }
              </style>
            </head>
            <body>${previewHtml}</body>
            </html>
          `);
          printWindow.document.close();
          printWindow.focus();
          setTimeout(() => {
            printWindow.print();
          }, 500);
          toast.success('Цінники відправлено на друк');
        } else {
          toast.error('Блокувальник спливних вікон. Дозвольте спливні вікна для цього сайту.');
        }
      } else {
        // Браузер — відкриваємо в новому вікні
        const printWindow = window.open('', '_blank');
        if (printWindow) {
          printWindow.document.write(`
            <!DOCTYPE html>
            <html>
            <head>
              <meta charset="UTF-8">
              <title>Друк цінників — Kasa POS</title>
              <style>
                @media print {
                  @page { margin: 0; size: A4; }
                  body { margin: 0; padding: 0; }
                }
              </style>
            </head>
            <body>${previewHtml}</body>
            </html>
          `);
          printWindow.document.close();
          printWindow.focus();
          setTimeout(() => {
            printWindow.print();
          }, 500);
          toast.success('Друк розпочато');
        } else {
          toast.error('Блокувальник спливних вікон. Дозвольте спливні вікна для цього сайту.');
        }
      }
    } catch (err: any) {
      toast.error(err?.message || 'Помилка друку');
    } finally {
      setIsPrinting(false);
    }
  }, [previewHtml]);

  return (
    <div className="flex flex-col h-[calc(100vh-4rem)]">
      {/* Заголовок */}
      <div className="flex items-center justify-between mb-4 flex-shrink-0">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate('/products')}
            className="p-2 rounded-lg text-gray-500 hover:text-gray-700 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
            title="Назад до товарів"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100">
              Друк цінників
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Виберіть товари, налаштуйте розміри та надрукуйте цінники на A4
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
            {isPreviewLoading ? 'Генерація...' : 'Оновити прев\'ю'}
          </Button>
          <Button
            onClick={handlePrint}
            disabled={isPrinting || !previewHtml}
            size="lg"
            icon={
              isPrinting ? (
                <Loader2 className="w-5 h-5 animate-spin" />
              ) : (
                <Printer className="w-5 h-5" />
              )
            }
          >
            {isPrinting ? 'Друк...' : '🖨️ Друк'}
          </Button>
        </div>
      </div>

      {/* Три колонки */}
      <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-4 min-h-0">
        {/* Колонка 1 — вибір товарів (4/12) */}
        <div className="lg:col-span-4 card p-4 overflow-hidden flex flex-col">
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

        {/* Колонка 2 — налаштування (3/12) */}
        <div className="lg:col-span-3 card p-4">
          <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Налаштування
          </h2>
          <PrintSettingsPanel
            templateId={settings.templateId}
            widthMm={settings.widthMm}
            heightMm={settings.heightMm}
            gapMm={settings.gapMm}
            marginMm={settings.marginMm}
            onChange={handleSettingsChange}
            type="price_tag"
          />
        </div>

        {/* Колонка 3 — прев'ю (5/12) */}
        <div className="lg:col-span-5 card p-4 overflow-hidden flex flex-col">
          <h2 className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3 flex-shrink-0">
            Попередній перегляд
          </h2>
          <div className="flex-1 min-h-0">
            <PrintPreview
              html={previewHtml}
              isLoading={isPreviewLoading}
              totalPages={renderResult?.totalPages}
              totalLabels={renderResult?.totalLabels}
              type="price_tag"
            />
          </div>
        </div>
      </div>
    </div>
  );
};

export default PrintPriceTagsPage;
