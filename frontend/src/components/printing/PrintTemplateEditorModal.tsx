import React, { useState, useEffect, useCallback, useRef } from 'react';
import {
  Eye,
  Save,
  X,
  Loader2,
  FileText,
  AlertTriangle,
  Braces,
  Code2,
  ListChecks,
} from 'lucide-react';
import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { printTemplateService } from '@/services/printTemplateService';
import type { PrintTemplate } from '@/types/printTemplate';
import toast from 'react-hot-toast';

// ═══════════════════════════════════════════════════════════════
// ПРОПСИ
// ═══════════════════════════════════════════════════════════════

interface PrintTemplateEditorModalProps {
  isOpen: boolean;
  onClose: () => void;
  /** Поточний шаблон для редагування (null — нічого не редагуємо) */
  template: PrintTemplate | null;
  /** Викликається після успішного збереження (для оновлення списку в батьківському компоненті) */
  onSaved?: (template: PrintTemplate) => void;
}

// ═══════════════════════════════════════════════════════════════
// СПИСОК ЗМІННИХ ДЛЯ ЦІННИКІВ/ЕТИКЕТОК
// ═══════════════════════════════════════════════════════════════

interface VariableDef {
  key: string;
  label: string;
  example?: string;
  /** true — умовна змінна ({{#if key}}...{{/if}}) */
  conditional?: boolean;
}

const SIMPLE_VARIABLES: VariableDef[] = [
  { key: 'name', label: 'Назва товару', example: 'Хліб білий нарізний' },
  { key: 'price', label: 'Ціна', example: '25.00' },
  { key: 'barcode', label: 'Штрих-код', example: '4820012345678' },
  { key: 'barcode_image', label: 'Зображення штрих-коду', example: 'генерується сервером' },
  { key: 'article', label: 'Артикул', example: 'ХЛ-001' },
  { key: 'category', label: 'Категорія', example: 'Хлібобулочні' },
  { key: 'created_date', label: 'Дата створення', example: '26.07.2026' },
  { key: 'width', label: 'Ширина (мм)', example: '40' },
  { key: 'height', label: 'Висота (мм)', example: '25' },
  { key: 'barcode_type', label: 'Тип коду', example: 'code128 / qr' },
  { key: 'barcode_height_mm', label: 'Висота коду (мм)', example: '7' },
];

const CONDITIONAL_VARIABLES: VariableDef[] = [
  { key: 'show_barcode', label: 'Показувати штрих-код', conditional: true },
  { key: 'show_price', label: 'Показувати ціну', conditional: true },
  { key: 'show_article', label: 'Показувати артикул', conditional: true },
  { key: 'show_created_date', label: 'Показувати дату', conditional: true },
  { key: 'show_category', label: 'Показувати категорію', conditional: true },
];

// ═══════════════════════════════════════════════════════════════
// ДЕМО-ДАНІ ДЛЯ ПОПЕРЕДНЬОГО ПЕРЕГЛЯДУ
// ═══════════════════════════════════════════════════════════════

const DEMO_RENDER_DATA: Record<string, Record<string, string>> = {
  price_tag: {
    name: 'Хліб білий нарізний',
    price: '25.00',
    barcode: '4820012345678',
    article: 'ХЛ-001',
    category: 'Хлібобулочні',
    created_date: '26.07.2026',
    width: '40',
    height: '25',
    barcode_type: 'code128',
    barcode_height_mm: '7',
    show_barcode: 'true',
    show_price: 'true',
    show_article: 'true',
    show_created_date: 'true',
    show_category: 'true',
  },
  label: {
    name: 'Молоко 2.6% "Селянське"',
    price: '38.50',
    barcode: '4820076500123',
    article: 'МЛ-002',
    category: 'Молочні продукти',
    created_date: '26.07.2026',
    width: '60',
    height: '40',
    barcode_type: 'code128',
    barcode_height_mm: '7',
    show_barcode: 'true',
    show_price: 'true',
    show_article: 'true',
    show_created_date: 'true',
    show_category: 'true',
  },
};

/** Отримати демо-дані за типом шаблону */
function getDemoDataForType(type: string): Record<string, string> {
  if (type === 'label') return DEMO_RENDER_DATA.label;
  return DEMO_RENDER_DATA.price_tag;
}

// ═══════════════════════════════════════════════════════════════
// КОМПОНЕНТ
// ═══════════════════════════════════════════════════════════════

const PrintTemplateEditorModal: React.FC<PrintTemplateEditorModalProps> = ({
  isOpen,
  onClose,
  template,
  onSaved,
}) => {
  // ── Стан вмісту ───────────────────────────────
  const [content, setContent] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Синхронізація вмісту при відкритті/зміні шаблону
  useEffect(() => {
    if (isOpen && template) {
      setContent(template.content);
      setPreviewHtml(null);
      setError(null);
    }
  }, [isOpen, template]);

  // Скидання при закритті
  const handleClose = useCallback(() => {
    if (isSaving) return; // не закриваємо під час збереження
    setError(null);
    setPreviewHtml(null);
    onClose();
  }, [isSaving, onClose]);

  // ── Вставка змінної в позицію курсора ─────────
  const insertVariable = useCallback((variable: VariableDef) => {
    const textarea = textareaRef.current;
    const current = content;

    if (variable.conditional) {
      // Умовна змінна: {{#if show_barcode}}...{{/if}}
      const insertion = `{{#if ${variable.key}}}...{{/if}}`;
      if (textarea) {
        const start = textarea.selectionStart;
        const end = textarea.selectionEnd;
        const next = current.substring(0, start) + insertion + current.substring(end);
        setContent(next);
        // Виділяємо "..." — щоб користувач одразу міг замінити
        requestAnimationFrame(() => {
          textarea.focus();
          const placeholderStart = start + `{{#if ${variable.key}}}`.length;
          textarea.setSelectionRange(placeholderStart, placeholderStart + 3);
        });
      } else {
        setContent(current + insertion);
      }
      return;
    }

    // Проста змінна: {{name}}
    const insertion = `{{${variable.key}}}`;
    if (textarea) {
      const start = textarea.selectionStart;
      const end = textarea.selectionEnd;
      const next = current.substring(0, start) + insertion + current.substring(end);
      setContent(next);
      // Відновлюємо позицію курсора після вставленого тексту
      requestAnimationFrame(() => {
        textarea.focus();
        const newPos = start + insertion.length;
        textarea.setSelectionRange(newPos, newPos);
      });
    } else {
      setContent(current + insertion);
    }
  }, [content]);

  // ── Попередній перегляд (render з demo-товаром) ──
  const handlePreview = useCallback(async () => {
    if (!template) return;
    if (!content.trim()) {
      setError('Вміст шаблону порожній — додайте HTML, щоб згенерувати прев\'ю');
      return;
    }
    setIsPreviewLoading(true);
    setError(null);
    try {
      const demoData = getDemoDataForType(template.type);
      const html = await printTemplateService.render(template.id, demoData);
      setPreviewHtml(html);
    } catch (err: any) {
      setError(
        err?.response?.data?.detail ||
        err?.message ||
        'Не вдалося згенерувати попередній перегляд. Перевірте вміст шаблону.',
      );
    } finally {
      setIsPreviewLoading(false);
    }
  }, [template, content]);

  // ── Збереження ────────────────────────────────
  const handleSave = useCallback(async () => {
    if (!template) return;

    // Валідація: вміст не порожній
    if (!content.trim()) {
      setError('Вміст шаблону не може бути порожнім');
      return;
    }

    setIsSaving(true);
    setError(null);
    try {
      const updated = await printTemplateService.update(template.id, {
        name: template.name,
        type: template.type,
        content,
        is_active: template.is_active,
      });
      toast.success(`Шаблон «${updated.name}» збережено`);
      onSaved?.(updated);
      onClose();
    } catch (err: any) {
      const msg =
        err?.response?.data?.detail ||
        (Array.isArray(err?.response?.data?.detail)
          ? err.response.data.detail.map((d: any) => d.msg || d).join('; ')
          : null) ||
        err?.message ||
        'Помилка при збереженні шаблону';
      setError(msg);
      toast.error(msg);
    } finally {
      setIsSaving(false);
    }
  }, [template, content, onSaved, onClose]);

  if (!template) return null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={handleClose}
      title={`Редагування шаблону: ${template.name}`}
      size="4xl"
    >
      <div className="flex flex-col gap-4">
        {/* ── Помилка ─────────────────────────── */}
        {error && (
          <div className="flex items-start gap-2.5 p-3.5 rounded-xl bg-danger-50 dark:bg-danger-900/20 border border-danger-200 dark:border-danger-800">
            <AlertTriangle className="w-4 h-4 text-danger-500 mt-0.5 shrink-0" />
            <div className="text-sm text-danger-700 dark:text-danger-300">
              <p className="font-medium mb-0.5">Помилка</p>
              <p className="text-xs opacity-90 break-words">{error}</p>
            </div>
            <button
              onClick={() => setError(null)}
              className="ml-auto p-1 rounded-md text-danger-400 hover:bg-danger-100 dark:hover:bg-danger-900/30 transition-colors"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        )}

        <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4 min-h-0">
          {/* ═══ ЛІВА КОЛОНКА: редактор + змінні ═══ */}
          <div className="flex flex-col gap-3 min-w-0">
            {/* Редактор HTML */}
            <div>
              <div className="flex items-center gap-1.5 mb-1.5">
                <Code2 className="w-4 h-4 text-gray-400" />
                <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  HTML-вміст шаблону
                </label>
              </div>
              <textarea
                ref={textareaRef}
                value={content}
                onChange={(e) => setContent(e.target.value)}
                spellCheck={false}
                className="
                  w-full h-[320px] lg:h-[380px] rounded-xl border border-gray-300 dark:border-slate-600
                  bg-white dark:bg-slate-900
                  text-gray-900 dark:text-gray-100
                  placeholder-gray-400 dark:placeholder-gray-500
                  focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-transparent
                  transition-all duration-150
                  font-mono text-[13px] leading-relaxed
                  px-4 py-3
                  resize-none
                "
                placeholder="<div style=&quot;width: 40mm;&quot;>...</div>"
              />
              <div className="flex items-center justify-between mt-1">
                <p className="text-xs text-gray-400 dark:text-gray-500">
                  {content.length} символів
                </p>
                {content.trim() === '' && (
                  <p className="text-xs text-amber-600 dark:text-amber-400">
                    Вміст не може бути порожнім
                  </p>
                )}
              </div>
            </div>

            {/* Змінні (підказка) */}
            <div className="space-y-2.5">
              <div>
                <div className="flex items-center gap-1.5 mb-1.5">
                  <Braces className="w-4 h-4 text-gray-400" />
                  <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    Змінні
                  </span>
                  <span className="text-xs text-gray-400">— клік вставляє в позицію курсора</span>
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {SIMPLE_VARIABLES.map((v) => (
                    <button
                      key={v.key}
                      onClick={() => insertVariable(v)}
                      title={`${v.label}${v.example ? ` — приклад: ${v.example}` : ''}`}
                      className="
                        px-2 py-1 text-xs font-mono rounded-lg
                        bg-white dark:bg-slate-800
                        border border-gray-200 dark:border-slate-600
                        text-primary-700 dark:text-primary-400
                        hover:bg-primary-50 dark:hover:bg-primary-900/20
                        hover:border-primary-300 dark:hover:border-primary-700
                        transition-all duration-100
                        cursor-pointer
                      "
                    >
                      {'{{'}{v.key}{'}}'}
                    </button>
                  ))}
                </div>
              </div>

              <div>
                <div className="flex items-center gap-1.5 mb-1.5">
                  <ListChecks className="w-4 h-4 text-gray-400" />
                  <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    Умовні блоки
                  </span>
                  <span className="text-xs text-gray-400">— показують вміст, якщо поле увімкнено</span>
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {CONDITIONAL_VARIABLES.map((v) => (
                    <button
                      key={v.key}
                      onClick={() => insertVariable(v)}
                      title={`{{#if ${v.key}}}...{{/if}} — ${v.label}`}
                      className="
                        px-2 py-1 text-xs font-mono rounded-lg
                        bg-amber-50 dark:bg-amber-900/20
                        border border-amber-200 dark:border-amber-800
                        text-amber-700 dark:text-amber-400
                        hover:bg-amber-100 dark:hover:bg-amber-900/30
                        transition-all duration-100
                        cursor-pointer
                      "
                    >
                      {'{{#if '}{v.key}{'}}…{{/if}}'}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </div>

          {/* ═══ ПРАВА КОЛОНКА: прев'ю ═══ */}
          <div className="flex flex-col min-w-0">
            <div className="flex items-center justify-between mb-1.5">
              <div className="flex items-center gap-1.5">
                <Eye className="w-4 h-4 text-gray-400" />
                <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  Попередній перегляд
                </label>
              </div>
              <Button
                variant="secondary"
                size="sm"
                onClick={handlePreview}
                disabled={isPreviewLoading || content.trim() === ''}
                icon={isPreviewLoading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Eye className="w-3.5 h-3.5" />}
              >
                {isPreviewLoading ? 'Генерація...' : 'Попередній перегляд'}
              </Button>
            </div>
            <div className="flex-1 min-h-[320px] lg:min-h-[380px] border border-gray-200 dark:border-slate-600 rounded-xl overflow-hidden bg-white dark:bg-slate-900 relative">
              {previewHtml ? (
                <iframe
                  title={`Прев'ю шаблону «${template.name}»`}
                  className="absolute inset-0 w-full h-full border-0"
                  sandbox="allow-same-origin allow-scripts"
                  srcDoc={previewHtml}
                />
              ) : isPreviewLoading ? (
                <div className="absolute inset-0 flex items-center justify-center">
                  <div className="text-center">
                    <Loader2 className="w-6 h-6 animate-spin text-primary-500 mx-auto mb-2" />
                    <p className="text-sm text-gray-500 dark:text-gray-400">
                      Генерація прев'ю...
                    </p>
                  </div>
                </div>
              ) : (
                <div className="absolute inset-0 flex items-center justify-center text-gray-400 dark:text-gray-500">
                  <div className="text-center px-4">
                    <FileText className="w-10 h-10 mx-auto mb-3 opacity-40" />
                    <p className="text-sm font-medium mb-1">Попередній перегляд</p>
                    <p className="text-xs opacity-70 max-w-[220px]">
                      Натисніть «Попередній перегляд», щоб побачити шаблон з демо-товаром
                    </p>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>

        {/* ── Кнопки дій ─────────────────────── */}
        <div className="flex items-center justify-between pt-4 mt-1 border-t border-gray-200 dark:border-slate-700">
          <p className="text-xs text-gray-400 dark:text-gray-500 max-w-[50%]">
            Тип: <span className="font-mono">{template.type}</span>
            {template.is_default && (
              <span className="ml-2 text-amber-600 dark:text-amber-400 font-medium">★ Основний</span>
            )}
          </p>
          <div className="flex items-center gap-3">
            <Button variant="secondary" onClick={handleClose} disabled={isSaving}>
              Скасувати
            </Button>
            <Button
              onClick={handleSave}
              disabled={isSaving || content.trim() === ''}
              icon={isSaving ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
            >
              {isSaving ? 'Збереження...' : 'Зберегти'}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
};

export default PrintTemplateEditorModal;
