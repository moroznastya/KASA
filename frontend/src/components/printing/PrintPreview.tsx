import React, { useEffect, useRef } from 'react';
import { FileText, Loader2 } from 'lucide-react';
import { Spinner } from '@/components/ui/Spinner';

// ── Пропси ───────────────────────────────────────
interface PrintPreviewProps {
  html: string | null;
  isLoading: boolean;
  totalPages?: number;
  totalLabels?: number;
  type: 'price_tag' | 'label';
}

// ── Компонент ────────────────────────────────────
const PrintPreview: React.FC<PrintPreviewProps> = ({
  html,
  isLoading,
  totalPages,
  totalLabels,
  type,
}) => {
  const iframeRef = useRef<HTMLIFrameElement>(null);

  // Оновлюємо iframe при зміні html
  useEffect(() => {
    if (html && iframeRef.current) {
      iframeRef.current.srcdoc = html;
    }
  }, [html]);

  return (
    <div className="flex flex-col h-full">
      {/* Мета-інформація */}
      {(totalPages !== undefined || totalLabels !== undefined) && html && !isLoading && (
        <div className="flex items-center gap-3 mb-3 px-1">
          {type === 'price_tag' && totalPages !== undefined && (
            <div className="flex items-center gap-1.5 px-3 py-1.5 bg-primary-50 dark:bg-primary-900/20 rounded-lg">
              <span className="text-xs font-medium text-primary-700 dark:text-primary-400">
                Сторінок: {totalPages}
              </span>
            </div>
          )}
          {totalLabels !== undefined && (
            <div className="flex items-center gap-1.5 px-3 py-1.5 bg-primary-50 dark:bg-primary-900/20 rounded-lg">
              <span className="text-xs font-medium text-primary-700 dark:text-primary-400">
                {type === 'price_tag' ? 'Цінників' : 'Етикеток'}: {totalLabels}
              </span>
            </div>
          )}
        </div>
      )}

      {/* Прев'ю */}
      <div className="flex-1 border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden bg-white dark:bg-slate-800 min-h-[300px]">
        {isLoading ? (
          <div className="flex items-center justify-center h-full min-h-[300px]">
            <div className="text-center p-6">
              <Spinner size="md" />
              <p className="mt-3 text-sm text-gray-500 dark:text-gray-400">
                Генерація {type === 'price_tag' ? 'цінників' : 'етикеток'}...
              </p>
            </div>
          </div>
        ) : html ? (
          <iframe
            ref={iframeRef}
            title={`Прев'ю ${type === 'price_tag' ? 'цінників' : 'етикеток'}`}
            className="w-full h-full min-h-[300px]"
            sandbox="allow-same-origin"
          />
        ) : (
          <div className="flex items-center justify-center h-full min-h-[300px] text-gray-400 dark:text-gray-500">
            <div className="text-center p-6">
              <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-gray-100 dark:bg-slate-700 flex items-center justify-center">
                <FileText className="w-8 h-8 text-gray-400" />
              </div>
              <p className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-1">
                Попередній перегляд
              </p>
              <p className="text-xs text-gray-400 dark:text-gray-500">
                Додайте товари, налаштуйте параметри та натисніть «Оновити прев'ю»
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

export default PrintPreview;
