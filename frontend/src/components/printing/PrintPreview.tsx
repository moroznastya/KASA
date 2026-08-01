import React, { useState, useCallback, useEffect, useLayoutEffect, useRef, useMemo } from 'react';
import { FileText, Maximize2, ZoomIn, ZoomOut } from 'lucide-react';
import { Spinner } from '@/components/ui/Spinner';

// ── Пропси ───────────────────────────────────────
interface PrintPreviewProps {
  html: string | null;
  isLoading: boolean;
  totalPages?: number;
  totalLabels?: number;
  type: 'price_tag' | 'label';
}

// ── Константи ────────────────────────────────────
// A4 при 96dpi: 210×297 мм ≈ 794×1123 px
const A4_WIDTH_PX = 794;
const A4_HEIGHT_PX = 1123;
const ZOOM_MIN = 0.1;
const ZOOM_MAX = 2.0;
const ZOOM_STEP = 0.25;

/** Швидкий хеш вмісту HTML — щоб не перезавантажувати iframe,
 *  якщо згенерований HTML ідентичний попередньому (md5 стабільний). */
function quickHash(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return h.toString(36) + ':' + s.length;
}

// ── Компонент ────────────────────────────────────
/**
 * WYSIWYG прев'ю (srcDoc = html як є) БЕЗ «плинності» при оновленні.
 *
 * АНТИ-ФЛІКЕР:
 *  - `displayHtml` — останній повністю завантажений html. Поки триває
 *    генерація (isLoading=true), показуємо СТАРИЙ html → прев'ю не зникає,
 *    інтерфейс не стрибає.
 *  - Оверлей зі спінером тримається, поки iframe реально не завантажив новий
 *    вміст (onLoad) → білий спалах при заміні srcDoc прихований.
 *
 * Стабільність висоти: контейнер `flex-1 min-h-0` у фіксованій за висотою
 * flex-колонці — висота НЕ залежить від вмісту/стану.
 */
const PrintPreview: React.FC<PrintPreviewProps> = ({
  html,
  isLoading,
  totalPages,
  totalLabels,
  type,
}) => {
  // ── Zoom (лише для відображення; НЕ скидається при оновленні html) ──
  const [zoom, setZoom] = useState(1);

  const zoomIn = useCallback(() => {
    setZoom((z) => Math.min(ZOOM_MAX, Math.round((z + ZOOM_STEP) * 100) / 100));
  }, []);

  const zoomOut = useCallback(() => {
    setZoom((z) => Math.max(ZOOM_MIN, Math.round((z - ZOOM_STEP) * 100) / 100));
  }, []);

  const resetZoom = useCallback(() => setZoom(1), []);

  // ── «По ширині»: підігнати масштаб під ширину контейнера (округлення до 0.05) ──
  const fitWidth = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = el.clientWidth;
    const f = Math.max(ZOOM_MIN, Math.min(1, Math.floor((w / 794) * 20) / 20));
    setZoom(f);
  }, []);

  // ── АНТИ-ФЛІКЕР: комітимо новий html ЛИШЕ після завершення генерації ──
  // displayHtml = «previousHtml»: старе прев'ю залишається видимим, поки
  // нове генерується (навіть якщо батько тимчасово передасть html=null).
  const [displayHtml, setDisplayHtml] = useState<string | null>(html);

  // ── Оверлей тримається, поки iframe не завантажив новий вміст ──
  const [iframeLoading, setIframeLoading] = useState(false);

  // ── Refs: контейнер прев'ю, iframe, збереження scroll при оновленні ──
  const containerRef = useRef<HTMLDivElement>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const savedScrollRef = useRef<{ top: number; left: number } | null>(null);
  const prevSizeRef = useRef<{ w: number; h: number } | null>(null);
  const hasFittedRef = useRef(false);

  useEffect(() => {
    // ДІАГНОСТИКА: зміни стану прев'ю (послідовність оновлень)
    console.log('[PREVIEW] state |', {
      isLoading,
      contentChanged: html !== displayHtml,
      htmlLen: html?.length,
      displayHtmlLen: displayHtml?.length,
      showOverlay: displayHtml !== null && iframeLoading,
      zoom,
    });
    if (isLoading) return; // генерація триває → НЕ чіпаємо (старе видно)
    // Завершено: комітимо новий html (або null → плейсхолдер при зміні типу)
    // ПЕРЕД заміною srcDoc зберігаємо позицію скролу контейнера — iframe
    // перезавантажиться і скрол інакше скинеться нагору.
    const el = containerRef.current;
    if (el && savedScrollRef.current === null && html !== displayHtml) {
      savedScrollRef.current = { top: el.scrollTop, left: el.scrollLeft };
    }
    // КОРІНЬ «картинальної зміни дизайну»: при першому прев'ю iframe
    // з'являвся з zoom=1 (A4 794px обрізаний), а fitWidth спрацьовував
    // ЛИШЕ ПІСЛЯ рендеру (окремий useEffect) → стрибок 794px → 278px.
    // Виправлення: fitWidth викликається ОДНОЧАСНО з setDisplayHtml —
    // React батчить setZoom + setDisplayHtml в один рендер, тому iframe
    // з'являється одразу з правильним масштабом (без стрибка).
    if (html && !hasFittedRef.current) {
      hasFittedRef.current = true;
      fitWidth();
    }
    setDisplayHtml((prev) => {
      if (prev === html) return prev;
      if (prev && html && quickHash(prev) === quickHash(html)) return prev;
      return html;
    });
  }, [html, isLoading, displayHtml, iframeLoading, zoom, fitWidth]);

  // useLayoutEffect — до paint: оверлей встигає накрити перезавантаження iframe
  useLayoutEffect(() => {
    if (displayHtml !== null) {
      setIframeLoading(true);
    }
  }, [displayHtml]);


  // ── ДІАГНОСТИКА: вимірюємо DOM усередині iframe (чи обрізається підпис QR) ──
  const measurePreview = useCallback(() => {
    const doc = iframeRef.current?.contentDocument;
    if (!doc) return;
    const cells = Array.from(doc.querySelectorAll('.tag-cell, .label-item'));
    const cellInfo = cells.slice(0, 3).map((el, i) => {
      const r = el.getBoundingClientRect();
      // Знайти підпис: span monospace всередині комірки
      const captions = Array.from(el.querySelectorAll('span[style*="monospace"]'));
      const capInfo = captions.map((c) => {
        const cr = c.getBoundingClientRect();
        return {
          top: Math.round(cr.top - r.top),
          bottom: Math.round(cr.bottom - r.top),
          h: Math.round(cr.height),
          visible: cr.bottom <= r.bottom,
        };
      });
      return {
        cell: i,
        h: Math.round(r.height),
        scrollH: el.scrollHeight,
        overflowHidden: getComputedStyle(el).overflow,
        captions: capInfo,
      };
    });
    console.log('[PREVIEW] measure |', {
      cells: cellInfo,
      containerH: containerRef.current?.clientHeight,
      iframeH: iframeRef.current?.clientHeight,
      scrollY: window.scrollY,
    });
  }, []);

  const handleIframeLoad = useCallback(() => {
    setIframeLoading(false);
    // Відновити позицію скролу контейнера після перезавантаження iframe
    requestAnimationFrame(() => {
      if (savedScrollRef.current && containerRef.current) {
        containerRef.current.scrollTop = savedScrollRef.current.top;
        containerRef.current.scrollLeft = savedScrollRef.current.left;
        savedScrollRef.current = null;
      }
    });
    // ДІАГНОСТИКА: після рендеру DOM усередині iframe — виміряти комірки
    requestAnimationFrame(() => {
      requestAnimationFrame(() => measurePreview());
    });
  }, [measurePreview]);

  // ── ДІАГНОСТИКА: зміни розміру контейнера прев'ю («плинність» layout) ──
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth;
      const h = el.clientHeight;
      const prev = prevSizeRef.current;
      prevSizeRef.current = { w, h };
      // Логуємо ТІЛЬКИ реальні зміни (пропускаємо початковий виклик observe)
      if (prev && (prev.w !== w || prev.h !== h)) {
        // Діагностика «плинності» прев'ю: увімкни через localStorage.setItem('kasa.debug.preview', '1')
        if (localStorage.getItem('kasa.debug.preview') === '1') {
          console.log('[PREVIEW] resize |', {
            w, h, st: el.scrollTop, sl: el.scrollLeft,
            scrollY: window.scrollY,
            winW: window.innerWidth, winH: window.innerHeight,
            lg: window.matchMedia('(min-width: 1024px)').matches,
          });
        }
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Перше прев'ю: автоматично підігнати A4 по ширині контейнера,
  // щоб користувач одразу бачив повний дизайн (а не величезний шматок при zoom=1).
  // Ручний zoom користувача при наступних оновленнях НЕ скидається (hasFittedRef).
  useEffect(() => {
    if (displayHtml && !hasFittedRef.current) {
      hasFittedRef.current = true;
      fitWidth();
    }
  }, [displayHtml, fitWidth]);

  // Оверлей: старий вміст видно крізь напівпрозорий шар
  const showOverlay = displayHtml !== null && iframeLoading;
  const showMeta = displayHtml && (totalPages !== undefined || totalLabels !== undefined);

  // Масштабований документ: transform scale ВСЕРЕДИНІ iframe (на body),
  // а не на iframe — інакше layout iframe = 794×1123 розширює scroll-область
  // контейнера (overflow-auto) → порожній простір 476px×674px при прокрутці.
  const scaledDoc = useMemo(() => {
    if (!displayHtml) return displayHtml;
    return displayHtml.replace(
      '</head>',
      `<style>
        html { overflow: hidden !important; }
        body {
          margin: 0 !important;
          padding: 0 !important;
          overflow: hidden !important;
          transform: scale(${zoom}) !important;
          transform-origin: top left !important;
          width: ${A4_WIDTH_PX}px !important;
          height: ${A4_HEIGHT_PX}px !important;
        }
      </style></head>`
    );
  }, [displayHtml, zoom]);

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Мета-інформація: обгортка ЗАВЖДИ присутня (min-h-8 + mb-3) — висота
          колонки стабільна, контент рендериться умовно (не стрибає 773→733) */}
      <div className="flex items-center gap-3 mb-3 px-1 flex-shrink-0 min-h-8">
        {showMeta && (
          <>
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
          </>
        )}
      </div>

      {/* Zoom-контрол: впливає лише на відображення, не на друк */}
      <div
        className="flex items-center gap-1 mb-2 flex-shrink-0"
        data-print-preview-zoom="true"
      >
        <button
          type="button"
          onClick={zoomOut}
          disabled={!displayHtml || zoom <= ZOOM_MIN}
          title="Зменшити"
          className="
            p-1.5 rounded-lg border border-gray-200 dark:border-slate-600
            bg-white dark:bg-slate-800
            text-gray-600 dark:text-gray-300
            hover:bg-gray-50 dark:hover:bg-slate-700
            disabled:opacity-40 disabled:cursor-not-allowed
            transition-colors
          "
        >
          <ZoomOut className="w-4 h-4" />
        </button>
        <button
          type="button"
          onClick={resetZoom}
          disabled={!displayHtml}
          title="Скинути масштаб (100%)"
          className="
            w-14 px-2 py-1.5 rounded-lg border border-gray-200 dark:border-slate-600
            bg-white dark:bg-slate-800
            text-xs font-semibold text-gray-700 dark:text-gray-300 tabular-nums
            hover:bg-gray-50 dark:hover:bg-slate-700
            disabled:opacity-40 disabled:cursor-not-allowed
            transition-colors
          "
        >
          {Math.round(zoom * 100)}%
        </button>
        <button
          type="button"
          onClick={fitWidth}
          disabled={!displayHtml}
          title="По ширині сторінки"
          className="
            p-1.5 rounded-lg border border-gray-200 dark:border-slate-600
            bg-white dark:bg-slate-800
            text-gray-600 dark:text-gray-300
            hover:bg-gray-50 dark:hover:bg-slate-700
            disabled:opacity-40 disabled:cursor-not-allowed
            transition-colors
          "
        >
          <Maximize2 className="w-4 h-4" />
        </button>
        <button
          type="button"
          onClick={zoomIn}
          disabled={!displayHtml || zoom >= ZOOM_MAX}
          title="Збільшити"
          className="
            p-1.5 rounded-lg border border-gray-200 dark:border-slate-600
            bg-white dark:bg-slate-800
            text-gray-600 dark:text-gray-300
            hover:bg-gray-50 dark:hover:bg-slate-700
            disabled:opacity-40 disabled:cursor-not-allowed
            transition-colors
          "
        >
          <ZoomIn className="w-4 h-4" />
        </button>
      </div>

      {/*
        Прев'ю — scrollable контейнер.
        Висота СТАБІЛЬНА: flex-1 у фіксованій flex-колонці (min-h-0),
        внутрішній контент (A4 794×1123 з transform scale) НЕ розширює
        grid-рядок — overflow-auto + min-h-0.
      */}
      <div
        className="relative flex-1 border border-gray-200 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 overflow-auto min-h-0"
        ref={containerRef}
        data-print-preview-container="true"
      >
        {displayHtml ? (
          <>
            {/* Масштабування ВСЕРЕДИНІ iframe: transform scale на body документа,
                а не на iframe. Layout iframe = A4×zoom (обгортка) → scrollWidth
                контейнера = точно A4×zoom, без порожнього простору 476px×674px. */}
            <div
              className="relative overflow-hidden"
              style={{ width: A4_WIDTH_PX * zoom, height: A4_HEIGHT_PX * zoom }}
            >
              <iframe
                ref={iframeRef}
                title={`Прев'ю ${type === 'price_tag' ? 'цінників' : 'етикеток'}`}
                className="absolute top-0 left-0 border-0 bg-white"
                style={{ width: A4_WIDTH_PX * zoom, height: A4_HEIGHT_PX * zoom }}
                sandbox="allow-scripts allow-same-origin"
                srcDoc={scaledDoc ?? undefined}
                onLoad={handleIframeLoad}
              />
            </div>

            {/* Оверлей під час оновлення: НЕПРОЗОРИЙ фон повністю ховає
                перезавантаження iframe (білий спалах). Тримається, поки iframe
                не завантажив новий вміст (onLoad). */}
            {showOverlay && (
              <div className="absolute inset-0 z-10 flex items-center justify-center bg-white dark:bg-slate-900">
                <div className="flex items-center gap-2 px-3 py-1.5 rounded-full bg-white/90 dark:bg-slate-800/90 shadow-md border border-gray-200 dark:border-slate-600">
                  <Spinner size="sm" />
                  <span className="text-xs font-medium text-gray-600 dark:text-gray-300">
                    Оновлення...
                  </span>
                </div>
              </div>
            )}
          </>
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
