import React, { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import {
  ArrowLeft, Plus, Edit3, Trash2, Star, Eye, Printer,
  FileText, Tag, Sticker, Copy, ExternalLink, Maximize2, Minimize2,
} from 'lucide-react';
import { EditorView, type ViewUpdate } from '@codemirror/view';
import { basicSetup } from 'codemirror';
import { EditorState } from '@codemirror/state';
import { html } from '@codemirror/lang-html';
import { oneDark } from '@codemirror/theme-one-dark';
import { autocompletion, closeBrackets } from '@codemirror/autocomplete';
import { printTemplateService } from '@/services/printTemplateService';
import type { PrintTemplate, PrintTemplateFormData, PrintTemplateType } from '@/types/printTemplate';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { Modal } from '@/components/ui/Modal';
import { Spinner } from '@/components/ui/Spinner';
import { Badge } from '@/components/ui/Badge';
import { toast } from 'react-hot-toast';

// ═══════════════════════════════════════════════════════════════
// КОНФІГУРАЦІЯ
// ═══════════════════════════════════════════════════════════════

const TYPE_CONFIG: Record<string, { label: string; icon: string; color: string }> = {
  receipt_58mm: { label: 'Чек 58 мм', icon: '🧾', color: 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400' },
  receipt_80mm: { label: 'Чек 80 мм', icon: '📄', color: 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400' },
  fiscal: { label: 'Фіскальний чек', icon: '🏛️', color: 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-400' },
  custom: { label: 'Кастомний', icon: '⚙️', color: 'bg-gray-100 dark:bg-slate-700 text-gray-700 dark:text-gray-300' },
  price_tag: { label: 'Цінник', icon: '🏷️', color: 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400' },
  label: { label: 'Етикетка', icon: '📑', color: 'bg-teal-100 dark:bg-teal-900/30 text-teal-700 dark:text-teal-400' },
};

const TYPE_OPTIONS: SelectOption[] = [
  { value: 'receipt_58mm', label: 'Чек 58 мм' },
  { value: 'receipt_80mm', label: 'Чек 80 мм' },
  { value: 'fiscal', label: 'Фіскальний чек' },
  { value: 'custom', label: 'Кастомний' },
  { value: 'price_tag', label: 'Цінник' },
  { value: 'label', label: 'Етикетка' },
];

/** Фільтр-кнопки для списку шаблонів */
interface FilterOption { value: string; label: string; icon?: string; }

const FILTER_OPTIONS: FilterOption[] = [
  { value: 'all', label: 'Всі', icon: '📋' },
  { value: 'receipt_58mm', label: 'Чек 58мм', icon: '🧾' },
  { value: 'receipt_80mm', label: 'Чек 80мм', icon: '📄' },
  { value: 'fiscal', label: 'Фіскальний', icon: '🏛️' },
  { value: 'price_tag', label: 'Цінник', icon: '🏷️' },
  { value: 'label', label: 'Етикетка', icon: '📑' },
  { value: 'custom', label: 'Кастомний', icon: '⚙️' },
];

// ═══════════════════════════════════════════════════════════════
// ЗМІННІ ДЛЯ ШАБЛОНІВ
// ═══════════════════════════════════════════════════════════════

interface VariableDef {
  key: string;
  label: string;
  example: string;
}

const PRICE_TAG_VARIABLES: VariableDef[] = [
  { key: 'name', label: 'Назва товару', example: 'Хліб білий' },
  { key: 'price', label: 'Ціна', example: '25.00' },
  { key: 'barcode', label: 'Штрих-код', example: '4820012345678' },
  { key: 'barcode_image', label: 'Зображення штрих-коду', example: '<svg>...</svg>' },
  { key: 'article', label: 'Артикул', example: 'ART-001' },
  { key: 'category', label: 'Категорія', example: 'Хлібобулочні' },
  { key: 'created_date', label: 'Дата створення', example: '01.01.2025' },
  { key: 'width', label: 'Ширина (мм)', example: '60' },
  { key: 'height', label: 'Висота (мм)', example: '40' },
];

const LABEL_VARIABLES: VariableDef[] = PRICE_TAG_VARIABLES; // ті самі

const RECEIPT_VARIABLES: VariableDef[] = [
  { key: 'shop_name', label: 'Назва магазину', example: 'Магазин "Калина"' },
  { key: 'shop_address', label: 'Адреса', example: 'м. Київ, вул. Хрещатик, 1' },
  { key: 'tax_id', label: 'ЄДРПОУ', example: '12345678' },
  { key: 'receipt_number', label: 'Номер чека', example: '0001' },
  { key: 'date', label: 'Дата', example: '30.07.2026' },
  { key: 'time', label: 'Час', example: '14:30' },
  { key: 'cashier', label: 'Касир', example: 'Іван Петренко' },
  { key: 'items', label: 'Список товарів (HTML)', example: '<div>...</div>' },
  { key: 'total', label: 'Сума', example: '153.50' },
  { key: 'payment_type', label: 'Тип оплати', example: 'Готівка' },
  { key: 'payment_amount', label: 'Сплачено', example: '200.00' },
  { key: 'change', label: 'Решта', example: '46.50' },
  // ── Фіскальні змінні (Фаза 3.8: QR ДПС) ──
  { key: 'fiscal_number', label: 'Фіскальний № чека', example: '12345' },
  { key: 'fiscal_fn', label: 'ФН (фіскальний номер ПРРО)', example: '4538765845' },
  { key: 'fiscal_date_time', label: 'Дата/час фіскалізації', example: '30.07.2026 14:30' },
  { key: 'fiscal_check_url', label: 'URL перевірки чеку ДПС', example: 'https://cabinet.tax.gov.ua/cashregs/check?...' },
  { key: 'qr_code', label: 'QR-код (img data-URI)', example: '<img src="data:image/svg+xml,...">' },
  { key: 'fiscal_block', label: 'Фіскальний блок (ФН + № + дата/час + QR ДПС)', example: '<div>ФН: ...<img src="data:...">...</div>' },
];

/** Отримати список змінних для типу шаблону */
function getVariablesForType(type: string): VariableDef[] {
  if (type === 'price_tag') return PRICE_TAG_VARIABLES;
  if (type === 'label') return LABEL_VARIABLES;
  if (type === 'receipt_58mm' || type === 'receipt_80mm' || type === 'fiscal') return RECEIPT_VARIABLES;
  // custom — комбінація всіх
  return [...new Set([...PRICE_TAG_VARIABLES, ...RECEIPT_VARIABLES])];
}

/** Перевірити, чи тип є цінником/етикеткою */
const isPriceTagOrLabel = (type: string): boolean => type === 'price_tag' || type === 'label';

/** Перевірити, чи тип є чеком */
const isReceipt = (type: string): boolean =>
  type === 'receipt_58mm' || type === 'receipt_80mm' || type === 'fiscal';

// ═══════════════════════════════════════════════════════════════
// ДЕМО-ДАНІ ДЛЯ ПРЕВ'Ю
// ═══════════════════════════════════════════════════════════════

const DEMO_RENDER_DATA_RECEIPT: Record<string, string> = {
  shop_name: 'Магазин "Калина"',
  shop_address: 'м. Київ, вул. Хрещатик, 1',
  tax_id: '12345678',
  receipt_number: '0001',
  date: new Date().toLocaleDateString('uk-UA'),
  time: new Date().toLocaleTimeString('uk-UA', { hour: '2-digit', minute: '2-digit' }),
  cashier: 'Іван Петренко',
  items: [
    '<div style="margin-bottom: 4px;">',
    '  <div style="font-size: 9px; color: #666;">4820012345678</div>',
    '  <div style="display: flex; justify-content: space-between;">',
    '    <span style="flex: 1;">Хліб білий</span>',
    '    <span style="text-align: center; width: 28px;">2</span>',
    '    <span style="text-align: right; width: 48px;">25.00</span>',
    '    <span style="text-align: right; width: 48px;">50.00</span>',
    '  </div>',
    '</div>',
    '<div style="margin-bottom: 4px;">',
    '  <div style="font-size: 9px; color: #666;">4820076500123</div>',
    '  <div style="display: flex; justify-content: space-between;">',
    '    <span style="flex: 1;">Молоко 2.6%</span>',
    '    <span style="text-align: center; width: 28px;">1</span>',
    '    <span style="text-align: right; width: 48px;">38.50</span>',
    '    <span style="text-align: right; width: 48px;">38.50</span>',
    '  </div>',
    '</div>',
    '<div style="margin-bottom: 4px;">',
    '  <div style="font-size: 9px; color: #666;">4820098700543</div>',
    '  <div style="display: flex; justify-content: space-between;">',
    '    <span style="flex: 1;">Яйця курячі (10 шт)</span>',
    '    <span style="text-align: center; width: 28px;">1</span>',
    '    <span style="text-align: right; width: 48px;">65.00</span>',
    '    <span style="text-align: right; width: 48px;">65.00</span>',
    '  </div>',
    '</div>',
    '<div style="margin-bottom: 4px;">',
    '  <div style="font-size: 9px; color: #666;">4820032100987</div>',
    '  <div style="display: flex; justify-content: space-between;">',
    '    <span style="flex: 1;">Цукор білий 1кг</span>',
    '    <span style="text-align: center; width: 28px;">3</span>',
    '    <span style="text-align: right; width: 48px;">32.00</span>',
    '    <span style="text-align: right; width: 48px;">96.00</span>',
    '  </div>',
    '</div>',
  ].join('\n'),
  total: '249.50',
  payment_type: 'Готівка',
  payment_amount: '250.00',
  change: '0.50',
  // Фіскальний блок: порожній → прев'ю показує звичайний (нефіскальний) чек без QR
  fiscal_number: '',
  fiscal_fn: '',
  fiscal_date_time: '',
  fiscal_check_url: '',
  qr_code: '',
  fiscal_block: '',
};

const DEMO_RENDER_DATA_PRICE_TAG: Record<string, string> = {
  name: 'Хліб білий нарізний',
  price: '25.00',
  barcode: '4820012345678',
  article: 'ХЛ-001',
  category: 'Хлібобулочні',
  width: '40',
  height: '25',
  show_barcode: 'true',
  show_article: 'true',
  show_category: 'false',
  show_price: 'true',
  show_created_date: 'true',
  created_date: '26.07.2026',
};

const DEMO_RENDER_DATA_LABEL: Record<string, string> = {
  name: 'Молоко 2.6% "Селянське"',
  price: '38.50',
  barcode: '4820076500123',
  article: 'МЛ-002',
  category: 'Молочні продукти',
  width: '60',
  height: '40',
  show_barcode: 'true',
  show_article: 'true',
  show_category: 'true',
  show_price: 'true',
  show_created_date: 'true',
  created_date: '26.07.2026',
};

// ═══════════════════════════════════════════════════════════════
// ДОПОМІЖНІ ФУНКЦІЇ
// ═══════════════════════════════════════════════════════════════

const formatDate = (dateStr: string): string => {
  const d = new Date(dateStr);
  return d.toLocaleDateString('uk-UA', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
};

// ═══════════════════════════════════════════════════════════════
// КОМПОНЕНТ: CodeMirrorEditor
// ═══════════════════════════════════════════════════════════════

interface CodeMirrorEditorProps {
  value: string;
  onChange: (value: string) => void;
  /** Ключ для скидання редактора при зміні шаблону */
  key?: string;
}

const CodeMirrorEditor: React.FC<CodeMirrorEditorProps> = ({ value, onChange }) => {
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    if (!editorRef.current) return;

    // Визначаємо тему: темна чи світла
    const isDark = document.documentElement.classList.contains('dark');

    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          basicSetup,
          html(),
          autocompletion(),
          closeBrackets(),
          isDark ? oneDark : [],
          EditorView.updateListener.of((update: ViewUpdate) => {
            if (update.docChanged) {
              onChangeRef.current(update.state.doc.toString());
            }
          }),
          EditorView.theme({
            '&': { height: '100%' },
            '.cm-scroller': { overflow: 'auto' },
            '.cm-content': { fontFamily: '"JetBrains Mono", "Fira Code", "Consolas", monospace', fontSize: '13px', lineHeight: '1.6' },
            '.cm-gutters': { display: 'none' },
          }),
        ],
      }),
      parent: editorRef.current,
    });

    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Оновлюємо вміст редактора ззовні (наприклад при зміні шаблону)
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const currentDoc = view.state.doc.toString();
    if (currentDoc !== value) {
      view.dispatch({
        changes: {
          from: 0,
          to: currentDoc.length,
          insert: value,
        },
      });
    }
  }, [value]);

  return (
    <div
      ref={editorRef}
      className="w-full border border-gray-300 dark:border-slate-600 rounded-lg overflow-hidden"
      style={{ minHeight: '400px' }}
    />
  );
};

// ═══════════════════════════════════════════════════════════════
// ОСНОВНИЙ КОМПОНЕНТ
// ═══════════════════════════════════════════════════════════════

const PrintTemplatesPage: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  // ── Стани ──────────────────────────────────────
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingTemplate, setEditingTemplate] = useState<PrintTemplate | null>(null);
  const [formData, setFormData] = useState<PrintTemplateFormData>({
    name: '',
    type: 'receipt_58mm',
    content: '',
    is_active: true,
  });
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);
  const [activeFilter, setActiveFilter] = useState('all');
  const [previewScale, setPreviewScale] = useState<'small' | 'medium' | 'large'>('medium');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Варто використовувати CodeMirror? (перевірка наявності)
  const useCodeMirror = true;

  // ── Завантаження шаблонів ──────────────────
  const { data: templates, isLoading, isError } = useQuery<PrintTemplate[]>({
    queryKey: ['print-templates', 'all'],
    queryFn: () => printTemplateService.getAllIncludingInactive(),
  });

  // ── Мутації ─────────────────────────────────
  const deleteMutation = useMutation({
    mutationFn: (id: string) => printTemplateService.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['print-templates'] });
      toast.success('Шаблон видалено');
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка при видаленні');
    },
  });

  const setDefaultMutation = useMutation({
    mutationFn: (id: string) => printTemplateService.setDefault(id),
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: ['print-templates'] });
      toast.success(`Шаблон "${data.name}" встановлено як основний`);
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка при встановленні основного');
    },
  });

  const saveMutation = useMutation({
    mutationFn: async (data: { id?: string; form: PrintTemplateFormData }) => {
      if (data.id) {
        return printTemplateService.update(data.id, data.form);
      } else {
        return printTemplateService.create(data.form);
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['print-templates'] });
      handleCloseModal();
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка при збереженні');
    },
  });

  const duplicateMutation = useMutation({
    mutationFn: async (template: PrintTemplate) => {
      const dupData: PrintTemplateFormData = {
        name: template.name + ' (копія)',
        type: template.type,
        content: template.content,
        is_active: template.is_active,
      };
      return printTemplateService.create(dupData);
    },
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: ['print-templates'] });
      toast.success(`Створено копію: "${data.name}"`);
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка при створенні копії');
    },
  });

  // ── Відкриття модалки ──────────────────────
  const handleOpenCreate = useCallback(() => {
    setEditingTemplate(null);
    setFormData({ name: '', type: 'receipt_58mm', content: '', is_active: true });
    setPreviewHtml(null);
    setIsModalOpen(true);
  }, []);

  const handleOpenEdit = useCallback((template: PrintTemplate) => {
    setEditingTemplate(template);
    setFormData({
      name: template.name,
      type: template.type,
      content: template.content,
      is_active: template.is_active,
    });
    setPreviewHtml(null);
    setIsModalOpen(true);
  }, []);

  const handleCloseModal = useCallback(() => {
    setIsModalOpen(false);
    setEditingTemplate(null);
    setPreviewHtml(null);
    setPreviewScale('medium');
  }, []);

  // ── Збереження ──────────────────────────────
  const handleSave = useCallback(() => {
    if (!formData.name.trim()) {
      toast.error('Введіть назву шаблону');
      return;
    }
    if (!formData.content.trim()) {
      toast.error('Введіть HTML-вміст шаблону');
      return;
    }
    saveMutation.mutate({ id: editingTemplate?.id, form: formData });
  }, [formData, editingTemplate, saveMutation]);

  // ── Видалення ───────────────────────────────
  const handleDelete = useCallback((template: PrintTemplate) => {
    if (window.confirm(`Видалити шаблон "${template.name}"?`)) {
      deleteMutation.mutate(template.id);
    }
  }, [deleteMutation]);

  // ── Дублювання ──────────────────────────────
  const handleDuplicate = useCallback((template: PrintTemplate) => {
    duplicateMutation.mutate(template);
  }, [duplicateMutation]);

  // ── Прев'ю ──────────────────────────────────
  const handlePreview = useCallback(async () => {
    if (!editingTemplate?.id) {
      toast.error('Збережіть шаблон перед переглядом');
      return;
    }
    setIsPreviewLoading(true);
    try {
      const demoData = formData.type === 'price_tag'
        ? DEMO_RENDER_DATA_PRICE_TAG
        : formData.type === 'label'
        ? DEMO_RENDER_DATA_LABEL
        : DEMO_RENDER_DATA_RECEIPT;
      const html = await printTemplateService.render(editingTemplate.id, demoData);
      setPreviewHtml(html);
    } catch {
      toast.error('Помилка генерації прев\'ю');
    } finally {
      setIsPreviewLoading(false);
    }
  }, [editingTemplate, formData.type]);

  // ── Авто-оновлення прев'ю з debounce ────────
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleContentChange = useCallback((content: string) => {
    setFormData((prev) => ({ ...prev, content }));

    // Авто-оновлення прев'ю через 500ms після зупинки вводу
    if (editingTemplate?.id) {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        handlePreview();
      }, 500);
    }
  }, [editingTemplate, handlePreview]);

  // Очищення таймера при розмонтуванні
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, []);

  // ── Вставка змінної в редактор ──────────────
  const insertVariable = useCallback((variableKey: string) => {
    const insertion = `{{${variableKey}}}`;

    // Пробуємо CodeMirror editor
    if (useCodeMirror) {
      // Якщо використовується CodeMirror, просто додаємо в formData через content
      setFormData((prev) => ({ ...prev, content: prev.content + insertion }));
      return;
    }

    // Фолбек: textarea — вставка в позицію курсора
    const textarea = textareaRef.current;
    if (textarea) {
      const start = textarea.selectionStart;
      const end = textarea.selectionEnd;
      const currentContent = formData.content;
      const newContent = currentContent.substring(0, start) + insertion + currentContent.substring(end);
      setFormData((prev) => ({ ...prev, content: newContent }));

      // Відновлюємо позицію курсора після вставленого тексту
      requestAnimationFrame(() => {
        textarea.focus();
        const newPos = start + insertion.length;
        textarea.setSelectionRange(newPos, newPos);
      });
    } else {
      setFormData((prev) => ({ ...prev, content: prev.content + insertion }));
    }
  }, [formData.content, useCodeMirror]);

  // ── Оновлення назви при зміні типу ─────────
  const handleTypeChange = useCallback((type: string) => {
    setFormData((prev) => ({ ...prev, type: type as PrintTemplateType }));
    if (!editingTemplate) {
      const typeLabels: Record<string, string> = {
        receipt_58mm: 'Новий шаблон чеку 58 мм',
        receipt_80mm: 'Новий шаблон чеку 80 мм',
        fiscal: 'Новий шаблон фіскального чеку',
        custom: 'Новий кастомний шаблон',
        price_tag: 'Новий шаблон цінника',
        label: 'Новий шаблон етикетки',
      };
      setFormData((prev) => ({
        ...prev,
        name: typeLabels[type] || prev.name,
      }));
    }
  }, [editingTemplate]);

  // ── Відкрити прев'ю в новому вікні ─────────
  const handleOpenPreviewInNewWindow = useCallback(() => {
    if (!previewHtml) return;
    const win = window.open('', '_blank');
    if (!win) {
      toast.error('Блокувальник спливних вікон. Дозвольте спливні вікна для цього сайту.');
      return;
    }
    win.document.write(previewHtml);
    win.document.close();
    win.focus();
  }, [previewHtml]);

  // ── Форматування типу для відображення ──────
  const renderTypeIcon = (type: string) => {
    const config = TYPE_CONFIG[type];
    if (!config) return null;
    return (
      <span className={`inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-xs font-medium ${config.color}`}>
        <span>{config.icon}</span>
        <span>{config.label}</span>
      </span>
    );
  };

  // ── Фільтрація списку ───────────────────────
  const filteredTemplates = useMemo(() => {
    const list = templates || [];
    if (activeFilter === 'all') return list;
    return list.filter((t) => t.type === activeFilter);
  }, [templates, activeFilter]);

  // ── Групування за типом (для відображення) ──
  const groupedTemplates = useMemo(() => {
    const list = filteredTemplates;
    const groups: Array<{ key: string; label: string; icon: React.ReactNode; templates: PrintTemplate[] }> = [];

    const receipts = list.filter((t) => isReceipt(t.type));
    const priceTags = list.filter((t) => t.type === 'price_tag');
    const labels = list.filter((t) => t.type === 'label');
    const custom = list.filter((t) => t.type === 'custom');
    const others = list.filter((t) => !isReceipt(t.type) && !isPriceTagOrLabel(t.type) && t.type !== 'custom');

    if (receipts.length > 0) {
      groups.push({ key: 'receipts', label: 'Шаблони чеків', icon: <Printer className="w-5 h-5 text-gray-500" />, templates: receipts });
    }
    if (priceTags.length > 0) {
      groups.push({ key: 'price_tags', label: 'Шаблони цінників', icon: <Tag className="w-5 h-5 text-amber-500" />, templates: priceTags });
    }
    if (labels.length > 0) {
      groups.push({ key: 'labels', label: 'Шаблони етикеток', icon: <Sticker className="w-5 h-5 text-teal-500" />, templates: labels });
    }
    if (custom.length > 0) {
      groups.push({ key: 'custom', label: 'Кастомні шаблони', icon: <FileText className="w-5 h-5 text-gray-500" />, templates: custom });
    }
    if (others.length > 0) {
      groups.push({ key: 'others', label: 'Інші шаблони', icon: <FileText className="w-5 h-5 text-gray-500" />, templates: others });
    }

    return groups;
  }, [filteredTemplates]);

  // ── Змінні для поточного типу ────────────────
  const currentVariables = useMemo(() => {
    return getVariablesForType(formData.type);
  }, [formData.type]);

  // ── Розмір прев'ю ────────────────────────────
  const previewHeight = useMemo(() => {
    switch (previewScale) {
      case 'small': return '250px';
      case 'large': return '600px';
      default: return '400px';
    }
  }, [previewScale]);

  // ── Стан завантаження ───────────────────────
  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <Spinner size="lg" />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="flex items-center justify-center min-h-[60vh]">
        <div className="text-center">
          <p className="text-red-500 font-medium">Помилка завантаження шаблонів</p>
          <p className="text-sm text-gray-500 mt-1">Спробуйте пізніше</p>
        </div>
      </div>
    );
  }

  const templateList = templates || [];

  // ── Рендер картки шаблону ────────────────────
  const renderTemplateCard = (template: PrintTemplate) => {
    const config = TYPE_CONFIG[template.type];
    return (
      <div
        key={template.id}
        className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden hover:shadow-md transition-shadow group"
      >
        <div className="p-5">
          <div className="flex items-start justify-between mb-3">
            <div className="flex items-center gap-2 flex-1 min-w-0">
              <div className={`w-10 h-10 rounded-lg flex items-center justify-center text-lg flex-shrink-0 ${config?.color || 'bg-gray-100'}`}>
                <span>{config?.icon || '📄'}</span>
              </div>
              <div className="min-w-0 flex-1">
                <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
                  {template.name}
                </h3>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                  {formatDate(template.updated_at)}
                </p>
              </div>
            </div>
            {template.is_default && (
              <Badge variant="primary" size="sm" className="flex-shrink-0 ml-2">
                <Star className="w-3 h-3 mr-1" />
                Основний
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            {renderTypeIcon(template.type)}
            {!template.is_active && (
              <Badge variant="warning" size="sm">Неактивний</Badge>
            )}
          </div>
        </div>
        <div className="px-5 py-3 bg-gray-50 dark:bg-slate-700/50 border-t border-gray-100 dark:border-slate-700 flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => handleOpenEdit(template)}
            icon={<Edit3 className="w-3.5 h-3.5" />}
            className="text-gray-600 dark:text-gray-400"
          >
            Редагувати
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => handleDuplicate(template)}
            icon={<Copy className="w-3.5 h-3.5" />}
            disabled={duplicateMutation.isPending}
            className="text-gray-600 dark:text-gray-400"
          >
            Копія
          </Button>
          <div className="flex-1" />
          {!template.is_default && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setDefaultMutation.mutate(template.id)}
              icon={<Star className="w-3.5 h-3.5" />}
              disabled={setDefaultMutation.isPending}
              className="text-amber-600 dark:text-amber-400"
            >
              Основний
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => handleDelete(template)}
            icon={<Trash2 className="w-3.5 h-3.5" />}
            disabled={deleteMutation.isPending}
            className="text-danger-600 dark:text-danger-400"
          >
            Видалити
          </Button>
        </div>
      </div>
    );
  };

  return (
    <div className="max-w-5xl mx-auto px-4 py-6 space-y-6">
      {/* ── Заголовок ─────────────────────── */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={() => navigate('/settings')}
            className="p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <ArrowLeft className="w-5 h-5 text-gray-500" />
          </button>
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
              Шаблони друку
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Керування шаблонами для чеків, цінників та етикеток
            </p>
          </div>
        </div>
        <Button onClick={handleOpenCreate} icon={<Plus className="w-4 h-4" />}>
          Створити новий
        </Button>
      </div>

      {/* ── Фільтри за типом ───────────────── */}
      <div className="flex items-center gap-2 flex-wrap">
        {FILTER_OPTIONS.map((opt) => (
          <button
            key={opt.value}
            onClick={() => setActiveFilter(opt.value)}
            className={`
              px-3 py-1.5 rounded-lg text-sm font-medium transition-all duration-150
              ${activeFilter === opt.value
                ? 'bg-primary-600 text-white shadow-sm ring-1 ring-primary-700'
                : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600'
              }
            `}
          >
            <span className="flex items-center gap-1.5">
              <span>{opt.icon}</span>
              <span>{opt.label}</span>
              {opt.value !== 'all' && (
                <span className="text-xs opacity-70">
                  ({templateList.filter((t) => opt.value === 'all' || t.type === opt.value).length})
                </span>
              )}
            </span>
          </button>
        ))}
      </div>

      {/* ── Групований список шаблонів ────── */}
      {groupedTemplates.length === 0 ? (
        <div className="text-center py-16">
          <div className="w-20 h-20 mx-auto mb-4 rounded-full bg-gray-100 dark:bg-slate-700 flex items-center justify-center">
            <Printer className="w-10 h-10 text-gray-400" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            {activeFilter === 'all' ? 'Ще немає шаблонів' : 'Немає шаблонів цього типу'}
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-6 max-w-md mx-auto">
            {activeFilter === 'all'
              ? 'Створіть перший шаблон для друку чеків, цінників або етикеток.'
              : 'Спробуйте змінити фільтр або створіть новий шаблон цього типу.'}
          </p>
          <Button onClick={handleOpenCreate} icon={<Plus className="w-4 h-4" />}>
            Створити шаблон
          </Button>
        </div>
      ) : (
        groupedTemplates.map((group) => (
          <div key={group.key}>
            <div className="flex items-center gap-2 mb-4">
              {group.icon}
              <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                {group.label}
              </h2>
              <span className="text-sm text-gray-400">({group.templates.length})</span>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {group.templates.map(renderTemplateCard)}
            </div>
          </div>
        ))
      )}

      {/* ── Модальне вікно редагування ──── */}
      <Modal
        isOpen={isModalOpen}
        onClose={handleCloseModal}
        title={editingTemplate ? 'Редагувати шаблон' : 'Створити шаблон'}
        size="4xl"
      >
        <div className="flex flex-col lg:flex-row gap-5 min-h-[600px]">
          {/* ═══ ЛІВА ЧАСТИНА: редактор ═══ */}
          <div className="flex-1 flex flex-col gap-4 min-w-0">
            <Input
              label="Назва шаблону"
              value={formData.name}
              onChange={(e) => setFormData((prev) => ({ ...prev, name: e.target.value }))}
              placeholder='Наприклад: "Чек 58 мм основний"'
            />

            <Select
              label="Тип шаблону"
              options={TYPE_OPTIONS}
              value={formData.type}
              onChange={(e) => handleTypeChange(e.target.value)}
            />

            {/* Панель вставки змінних */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
                Вставка змінних
              </label>
              <div className="flex flex-wrap gap-1.5 p-2 bg-gray-50 dark:bg-slate-700/50 border border-gray-200 dark:border-slate-600 rounded-lg max-h-[120px] overflow-y-auto">
                {currentVariables.map((v) => (
                  <button
                    key={v.key}
                    onClick={() => insertVariable(v.key)}
                    className="
                      px-2 py-1 text-xs font-mono rounded
                      bg-white dark:bg-slate-800
                      border border-gray-200 dark:border-slate-600
                      text-primary-700 dark:text-primary-400
                      hover:bg-primary-50 dark:hover:bg-primary-900/20
                      hover:border-primary-300 dark:hover:border-primary-700
                      transition-all duration-100
                      cursor-pointer
                    "
                    title={`${v.label}: ${v.example}`}
                  >
                    {'{{'}{v.key}{'}}'}
                  </button>
                ))}
              </div>
            </div>

            {/* Редактор HTML */}
            <div className="flex-1 flex flex-col min-h-0">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5 flex-shrink-0">
                HTML-вміст шаблону
              </label>
              <div className="flex-1 min-h-[300px]">
                {useCodeMirror ? (
                  <CodeMirrorEditor
                    key={editingTemplate?.id || 'new'}
                    value={formData.content}
                    onChange={handleContentChange}
                  />
                ) : (
                  <textarea
                    ref={textareaRef}
                    value={formData.content}
                    onChange={(e) => handleContentChange(e.target.value)}
                    className="
                      w-full h-full min-h-[300px] rounded-lg border border-gray-300 dark:border-slate-600
                      bg-white dark:bg-slate-800
                      text-gray-900 dark:text-gray-100
                      placeholder-gray-400 dark:placeholder-gray-500
                      focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-transparent
                      transition-all duration-150
                      font-mono text-sm leading-relaxed
                      px-4 py-3
                      resize-none
                    "
                    placeholder="<html><body>...</body></html>"
                    spellCheck={false}
                  />
                )}
              </div>
            </div>
          </div>

          {/* ═══ ПРАВА ЧАСТИНА: змінні + прев'ю ═══ */}
          <div className="w-full lg:w-80 xl:w-96 flex flex-col gap-4 flex-shrink-0">
            {/* Доступні змінні */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
                Доступні змінні
              </label>
              <div className="border border-gray-200 dark:border-slate-600 rounded-lg divide-y divide-gray-100 dark:divide-slate-700 max-h-[300px] overflow-y-auto bg-white dark:bg-slate-800">
                {currentVariables.map((v) => (
                  <button
                    key={v.key}
                    onClick={() => insertVariable(v.key)}
                    className="
                      w-full text-left px-3 py-2 hover:bg-gray-50 dark:hover:bg-slate-700
                      transition-colors cursor-pointer flex flex-col gap-0.5
                    "
                  >
                    <span className="text-xs font-mono text-primary-600 dark:text-primary-400">
                      {'{{'}{v.key}{'}}'}
                    </span>
                    <span className="text-xs text-gray-500 dark:text-gray-400">
                      {v.label}
                    </span>
                    <span className="text-[10px] text-gray-400 dark:text-gray-500 italic">
                      {v.example}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            {/* Прев'ю */}
            {editingTemplate?.id && (
              <div className="flex-1 flex flex-col min-h-0">
                <div className="flex items-center justify-between mb-2 flex-shrink-0">
                  <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                    Попередній перегляд
                  </label>
                  <div className="flex items-center gap-1">
                    {/* Перемикач розміру */}
                    <div className="flex items-center border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden">
                      <button
                        onClick={() => setPreviewScale('small')}
                        className={`p-1.5 text-xs transition-colors ${previewScale === 'small' ? 'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-400' : 'text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700'}`}
                        title="Малий"
                      >
                        <Minimize2 className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => setPreviewScale('medium')}
                        className={`p-1.5 text-xs transition-colors ${previewScale === 'medium' ? 'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-400' : 'text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700'}`}
                        title="Середній"
                      >
                        <Maximize2 className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => setPreviewScale('large')}
                        className={`p-1.5 text-xs transition-colors ${previewScale === 'large' ? 'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-400' : 'text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700'}`}
                        title="Великий"
                      >
                        <ExternalLink className="w-3.5 h-3.5" />
                      </button>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={handlePreview}
                      disabled={isPreviewLoading}
                      icon={isPreviewLoading ? <Spinner size="sm" /> : <Eye className="w-3.5 h-3.5" />}
                      className="!p-1.5"
                      title="Оновити прев'ю"
                    />
                    {previewHtml && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={handleOpenPreviewInNewWindow}
                        icon={<ExternalLink className="w-3.5 h-3.5" />}
                        className="!p-1.5"
                        title="Відкрити в новому вікні"
                      />
                    )}
                  </div>
                </div>
                <div
                  className="flex-1 border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden bg-white dark:bg-slate-800 min-h-[200px] relative"
                  style={{ height: previewHeight }}
                >
                  {previewHtml ? (
                    <iframe
                      title="Прев'ю шаблону"
                      className="absolute inset-0 w-full h-full border-0"
                      sandbox="allow-same-origin"
                      srcDoc={previewHtml}
                    />
                  ) : isPreviewLoading ? (
                    <div className="absolute inset-0 flex items-center justify-center">
                      <div className="text-center">
                        <Spinner size="md" />
                        <p className="mt-2 text-sm text-gray-500">Генерація прев'ю...</p>
                      </div>
                    </div>
                  ) : (
                    <div className="absolute inset-0 flex items-center justify-center text-gray-400 dark:text-gray-500">
                      <div className="text-center">
                        <FileText className="w-8 h-8 mx-auto mb-2 opacity-50" />
                        <p className="text-sm">Натисніть оновити, щоб побачити результат</p>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Кнопки дій */}
        <div className="flex items-center justify-between pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <div className="flex items-center gap-2">
            {editingTemplate && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => handleDuplicate(editingTemplate)}
                disabled={duplicateMutation.isPending}
                icon={<Copy className="w-4 h-4" />}
                className="text-gray-600 dark:text-gray-400"
              >
                Створити копію
              </Button>
            )}
            {editingTemplate && !editingTemplate.is_default && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setDefaultMutation.mutate(editingTemplate.id)}
                disabled={setDefaultMutation.isPending}
                icon={<Star className="w-4 h-4" />}
                className="text-amber-600 dark:text-amber-400"
              >
                Зробити основним
              </Button>
            )}
            {editingTemplate?.is_default && (
              <Badge variant="primary" size="md">
                <Star className="w-3.5 h-3.5 mr-1.5" />
                Основний шаблон
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-3">
            <Button variant="secondary" onClick={handleCloseModal}>
              Скасувати
            </Button>
            <Button
              onClick={handleSave}
              disabled={saveMutation.isPending}
              icon={saveMutation.isPending ? <Spinner size="sm" /> : undefined}
            >
              {saveMutation.isPending ? 'Збереження...' : 'Зберегти'}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};

export default PrintTemplatesPage;
