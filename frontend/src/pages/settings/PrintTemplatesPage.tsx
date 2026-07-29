import React, { useState, useCallback, useRef, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { ArrowLeft, Plus, Edit3, Trash2, Star, Eye, Printer, FileText, Tag, Sticker } from 'lucide-react';
import { printTemplateService } from '@/services/printTemplateService';
import type { PrintTemplate, PrintTemplateFormData, PrintTemplateType } from '@/types/printTemplate';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { Modal } from '@/components/ui/Modal';
import { Spinner } from '@/components/ui/Spinner';
import { Badge } from '@/components/ui/Badge';
import { toast } from 'react-hot-toast';

// ── Конфігурація типів ──────────────────────────
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

// ── Дані для прев'ю ──────────────────────────
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
  payment_method: 'Готівка',
  paid: '250.00',
  change: '0.50',
  footer: 'Дякуємо за покупку!',
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

// ── Допоміжні функції ────────────────────────
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

const isPriceTagOrLabel = (type: string): boolean => type === 'price_tag' || type === 'label';

// ── Основний компонент ────────────────────────
const PrintTemplatesPage: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  // Стани
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
  const iframeRef = useRef<HTMLIFrameElement>(null);

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
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: ['print-templates'] });
      toast.success(data.id ? 'Шаблон оновлено' : 'Шаблон створено');
      handleCloseModal();
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка при збереженні');
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

  // ── Оновлення iframe при зміні прев'ю ───────
  useEffect(() => {
    if (previewHtml && iframeRef.current) {
      const iframe = iframeRef.current;
      iframe.srcdoc = previewHtml;
    }
  }, [previewHtml]);

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

  // ── Фільтрація за типом ──────────────────────
  const receiptTemplates = templateList.filter(t => !isPriceTagOrLabel(t.type));
  const priceTagTemplates = templateList.filter(t => t.type === 'price_tag');
  const labelTemplates = templateList.filter(t => t.type === 'label');

  // ── Рендер списку шаблонів ──────────────────
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
    <div className="max-w-5xl mx-auto px-4 py-6 space-y-8">
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

      {/* ── Секція: Шаблони чеків ────────── */}
      {receiptTemplates.length > 0 && (
        <div>
          <div className="flex items-center gap-2 mb-4">
            <Printer className="w-5 h-5 text-gray-500" />
            <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Шаблони чеків
            </h2>
            <span className="text-sm text-gray-400">({receiptTemplates.length})</span>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {receiptTemplates.map(renderTemplateCard)}
          </div>
        </div>
      )}

      {/* ── Секція: Шаблони цінників ─────── */}
      <div>
        <div className="flex items-center gap-2 mb-4">
          <Tag className="w-5 h-5 text-amber-500" />
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Шаблони цінників
          </h2>
          <span className="text-sm text-gray-400">({priceTagTemplates.length})</span>
        </div>
        {priceTagTemplates.length === 0 ? (
          <div className="bg-white dark:bg-slate-800 rounded-xl border border-dashed border-gray-300 dark:border-slate-600 p-8 text-center">
            <Tag className="w-10 h-10 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
              Ще немає шаблонів цінників
            </p>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                setEditingTemplate(null);
                setFormData({
                  name: 'Новий шаблон цінника',
                  type: 'price_tag',
                  content: '',
                  is_active: true,
                });
                setPreviewHtml(null);
                setIsModalOpen(true);
              }}
              icon={<Plus className="w-3.5 h-3.5" />}
            >
              Створити шаблон цінника
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {priceTagTemplates.map(renderTemplateCard)}
          </div>
        )}
      </div>

      {/* ── Секція: Шаблони етикеток ─────── */}
      <div>
        <div className="flex items-center gap-2 mb-4">
          <Sticker className="w-5 h-5 text-teal-500" />
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Шаблони етикеток
          </h2>
          <span className="text-sm text-gray-400">({labelTemplates.length})</span>
        </div>
        {labelTemplates.length === 0 ? (
          <div className="bg-white dark:bg-slate-800 rounded-xl border border-dashed border-gray-300 dark:border-slate-600 p-8 text-center">
            <Sticker className="w-10 h-10 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
              Ще немає шаблонів етикеток
            </p>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                setEditingTemplate(null);
                setFormData({
                  name: 'Новий шаблон етикетки',
                  type: 'label',
                  content: '',
                  is_active: true,
                });
                setPreviewHtml(null);
                setIsModalOpen(true);
              }}
              icon={<Plus className="w-3.5 h-3.5" />}
            >
              Створити шаблон етикетки
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {labelTemplates.map(renderTemplateCard)}
          </div>
        )}
      </div>

      {/* ── Пустий стан ──────────────────── */}
      {templateList.length === 0 && (
        <div className="text-center py-16">
          <div className="w-20 h-20 mx-auto mb-4 rounded-full bg-gray-100 dark:bg-slate-700 flex items-center justify-center">
            <Printer className="w-10 h-10 text-gray-400" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            Ще немає шаблонів
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-6 max-w-md mx-auto">
            Створіть перший шаблон для друку чеків, цінників або етикеток.
          </p>
          <Button onClick={handleOpenCreate} icon={<Plus className="w-4 h-4" />}>
            Створити перший шаблон
          </Button>
        </div>
      )}

      {/* ── Модальне вікно редагування ──── */}
      <Modal
        isOpen={isModalOpen}
        onClose={handleCloseModal}
        title={editingTemplate ? 'Редагувати шаблон' : 'Створити шаблон'}
        size="4xl"
      >
        <div className="space-y-5">
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

          {/* Підказка для цінників/етикеток */}
          {isPriceTagOrLabel(formData.type) && (
            <div className="bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-lg p-3 text-sm text-blue-700 dark:text-blue-300">
              <p className="font-medium mb-1">Доступні змінні для {formData.type === 'price_tag' ? 'цінника' : 'етикетки'}:</p>
              <code className="text-xs">
                {'{{name}}'} — назва товару<br />
                {'{{price}}'} — ціна<br />
                {'{{barcode}}'} — штрих-код<br />
                {'{{article}}'} — артикул<br />
                {'{{category}}'} — категорія<br />
                {'{{created_date}}'} — дата створення цінника<br />
                {'{{width}}'} — ширина (мм)<br />
                {'{{height}}'} — висота (мм)<br />
                {'{{show_barcode}}'} — показувати штрих-код<br />
                {'{{show_price}}'} — показувати ціну<br />
                {'{{show_article}}'} — показувати артикул<br />
                {'{{show_category}}'} — показувати категорію<br />
                {'{{show_created_date}}'} — показувати дату створення
              </code>
            </div>
          )}

          {/* HTML-вміст */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
              HTML-вміст шаблону
            </label>
            <textarea
              value={formData.content}
              onChange={(e) => setFormData((prev) => ({ ...prev, content: e.target.value }))}
              rows={16}
              className="
                w-full rounded-lg border border-gray-300 dark:border-slate-600
                bg-white dark:bg-slate-800
                text-gray-900 dark:text-gray-100
                placeholder-gray-400 dark:placeholder-gray-500
                focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-transparent
                transition-all duration-150
                font-mono text-sm leading-relaxed
                px-4 py-3
                resize-vertical
              "
              placeholder='<html><body>...</body></html>'
              spellCheck={false}
            />
          </div>

          {/* Прев'ю */}
          {editingTemplate?.id && (
            <div>
              <div className="flex items-center justify-between mb-2">
                <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  Попередній перегляд
                </label>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handlePreview}
                  disabled={isPreviewLoading}
                  icon={isPreviewLoading ? <Spinner size="sm" /> : <Eye className="w-3.5 h-3.5" />}
                >
                  {isPreviewLoading ? 'Генерація...' : 'Оновити прев\'ю'}
                </Button>
              </div>
              <div className="border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden bg-white">
                {previewHtml ? (
                  <iframe
                    ref={iframeRef}
                    title="Прев'ю шаблону"
                    className="w-full h-[400px]"
                    sandbox="allow-same-origin"
                  />
                ) : (
                  <div className="flex items-center justify-center h-[200px] text-gray-400 dark:text-gray-500">
                    <div className="text-center">
                      <FileText className="w-8 h-8 mx-auto mb-2 opacity-50" />
                      <p className="text-sm">Натисніть "Оновити прев'ю", щоб побачити результат</p>
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Кнопки дій */}
          <div className="flex items-center justify-between pt-4 border-t border-gray-200 dark:border-slate-700">
            <div>
              {editingTemplate && !editingTemplate.is_default && (
                <Button
                  variant="ghost"
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
        </div>
      </Modal>
    </div>
  );
};

export default PrintTemplatesPage;
