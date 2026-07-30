import React, { useMemo } from 'react';
import { FileText } from 'lucide-react';
import { Spinner } from '@/components/ui/Spinner';

// ── Пропси ───────────────────────────────────────
interface PrintPreviewProps {
  html: string | null;
  isLoading: boolean;
  totalPages?: number;
  totalLabels?: number;
  type: 'price_tag' | 'label';
}

// ── Функція: прибрати @page з HTML для прев'ю ──
function stripAtPageRules(html: string): string {
  return html.replace(/@page\s*\{[^}]*\}/gs, '');
}

// ── Функція: додати скрипт масштабування ───────
function injectScaleScript(html: string): string {
  return html.replace(
    '</body>',
    `<script>
      (function() {
        function fitContent() {
          var body = document.body;
          if (!body) return;
          var scaleX = window.innerWidth / (body.scrollWidth || 1);
          var scaleY = window.innerHeight / (body.scrollHeight || 1);
          var scale = Math.min(scaleX, scaleY, 1);
          if (scale < 1) {
            body.style.transform = 'scale(' + scale + ')';
            body.style.transformOrigin = 'top left';
            body.style.overflow = 'hidden';
          }
        }
        if (document.readyState === 'complete') {
          fitContent();
        } else {
          window.addEventListener('load', fitContent);
        }
      })();
    </script></body>`
  );
}

// ── Компонент ────────────────────────────────────
const PrintPreview: React.FC<PrintPreviewProps> = ({
  html,
  isLoading,
  totalPages,
  totalLabels,
  type,
}) => {
  // Обробка HTML: видаляємо @page, додаємо скрипт
  const processedHtml = useMemo(() => {
    if (!html) return undefined;
    const withoutPage = stripAtPageRules(html);
    return injectScaleScript(withoutPage);
  }, [html]);

  const showMeta = html && (totalPages !== undefined || totalLabels !== undefined);

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Мета-інформація */}
      {showMeta && (
        <div className="flex items-center gap-3 mb-3 px-1 flex-shrink-0">
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

      {/* Прев'ю — relative контейнер з absolute iframe */}
      <div
        className="relative flex-1 border border-gray-200 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 overflow-hidden min-h-0"
        data-print-preview-container="true"
      >
        {html ? (
          <iframe
            title={`Прев'ю ${type === 'price_tag' ? 'цінників' : 'етикеток'}`}
            className="absolute inset-0 w-full h-full border-0"
            sandbox="allow-scripts allow-same-origin"
            srcDoc={processedHtml}
          />
        ) : isLoading ? (
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="text-center p-6">
              <Spinner size="md" />
              <p className="mt-3 text-sm text-gray-500 dark:text-gray-400">
                Генерація {type === 'price_tag' ? 'цінників' : 'етикеток'}...
              </p>
            </div>
          </div>
        ) : (
          <div className="absolute inset-0 flex items-center justify-center text-gray-400 dark:text-gray-500">
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
