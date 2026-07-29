import React, { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, FileText, Search, CheckCircle, ShoppingCart, Printer, Copy, Download, Filter, Save, Bookmark, X, ChevronDown, Trash2 } from 'lucide-react';
import { useDocuments, useConfirmDocument, useBatchConfirm, useCopyDocument, useDeleteDocument } from '@/hooks/useDocuments';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { Badge } from '@/components/ui/Badge';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Modal } from '@/components/ui/Modal';
import { formatCurrency, formatDateTime, formatDocumentType, formatDocumentStatus } from '@/utils/format';
import { Document, DocumentType, DocumentFilterPreset } from '@/types/document';
import { supplierService } from '@/services/supplierService';
import { documentService } from '@/services/documentService';
import toast from 'react-hot-toast';
import { useAuthStore } from '@/store/authStore';

interface Supplier {
  id: string;
  name: string;
}

const documentTypeOptions: SelectOption[] = [
  { value: '', label: 'Всі типи' },
  { value: 'invoice', label: 'Прибуткові накладні' },
  { value: 'purchase_order', label: 'Замовлення постачальнику' },
  { value: 'transfer', label: 'Переміщення' },
  { value: 'write_off', label: 'Списання' },
  { value: 'return_invoice', label: 'Повернення' },
  { value: 'inventory', label: 'Інвентаризація' },
];

const statusOptions: SelectOption[] = [
  { value: '', label: 'Всі' },
  { value: 'draft', label: 'Чернетка' },
  { value: 'confirmed', label: 'Підтверджено' },
  { value: 'cancelled', label: 'Скасовано' },
];

const statusBadgeVariant: Record<string, 'default' | 'success' | 'danger' | 'warning'> = {
  draft: 'warning',
  confirmed: 'success',
  cancelled: 'danger',
};

/** Мапа типів документів -> шлях для перегляду */
const documentViewPaths: Record<string, string> = {
  invoice: '/documents/invoice',
  purchase_order: '/documents/purchase-order',
  transfer: '/documents/transfer',
  write_off: '/documents/write-off',
  return_invoice: '/documents/return',
  inventory: '/documents/inventory',
};

const defaultFilters = {
  search: '',
  document_type: '',
  status: '',
  date_from: '',
  date_to: '',
  supplier_id: '',
  amount_from: '',
  amount_to: '',
};

const DocumentListPage: React.FC = () => {
  const navigate = useNavigate();
  const [page, setPage] = useState(1);

  // --- Advanced filters ---
  const [showAdvancedFilters, setShowAdvancedFilters] = useState(false);
  const [filters, setFilters] = useState(defaultFilters);
  const [suppliers, setSuppliers] = useState<Supplier[]>([]);

  // --- Presets ---
  const [savedPresets, setSavedPresets] = useState<{ name: string; filters: typeof filters }[]>(() => {
    try {
      const saved = localStorage.getItem('document_filter_presets');
      return saved ? JSON.parse(saved) : [];
    } catch { return []; }
  });
  const [presetName, setPresetName] = useState('');
  const [showSavePreset, setShowSavePreset] = useState(false);

  // --- Batch selection ---
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [selectAll, setSelectAll] = useState(false);

  // --- Confirm/Cancel dialogs ---
  const [confirmItem, setConfirmItem] = useState<{ id: string; type?: DocumentType } | null>(null);
  const [batchConfirmOpen, setBatchConfirmOpen] = useState(false);
  const [deleteItem, setDeleteItem] = useState<{ id: string; type: DocumentType } | null>(null);

  // --- Load suppliers for filter ---
  useEffect(() => {
    supplierService.getAllSuppliers().then((data) => {
      setSuppliers(data);
    }).catch(() => {
      // Ignore errors loading suppliers
    });
  }, []);

  // --- Query params for useDocuments ---
  const queryParams = {
    page,
    size: 20,
    ...(filters.search ? { search: filters.search } : {}),
    ...(filters.document_type ? { document_type: filters.document_type as DocumentType } : {}),
    ...(filters.status ? { status: filters.status } : {}),
    ...(filters.date_from ? { date_from: filters.date_from } : {}),
    ...(filters.date_to ? { date_to: filters.date_to } : {}),
    ...(filters.supplier_id ? { supplier_id: filters.supplier_id } : {}),
    ...(filters.amount_from ? { amount_from: filters.amount_from } : {}),
    ...(filters.amount_to ? { amount_to: filters.amount_to } : {}),
  };

  const { data, isLoading, error } = useDocuments(queryParams);

  const confirmMutation = useConfirmDocument();
  const batchConfirmMutation = useBatchConfirm();
  const copyMutation = useCopyDocument();
  const deleteMutation = useDeleteDocument();

  // --- Supplier Select options ---
  const supplierFilterOptions: SelectOption[] = [
    { value: '', label: 'Всі постачальники' },
    ...suppliers.map((s) => ({ value: s.id, label: s.name })),
  ];

  // --- Reset selection on page change ---
  useEffect(() => {
    setSelectedIds(new Set());
    setSelectAll(false);
  }, [page]);

  // --- Sync selectAll ---
  useEffect(() => {
    if (data?.items && data.items.length > 0) {
      const allSelected = data.items.every((item) => selectedIds.has(item.id));
      setSelectAll(allSelected);
    } else {
      setSelectAll(false);
    }
  }, [selectedIds, data?.items]);

  // --- Keyboard handler ---
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && selectedIds.size > 0) {
        setSelectedIds(new Set());
        setSelectAll(false);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedIds]);

  // --- Handlers ---

  const toggleSelectAll = () => {
    if (!data?.items) return;
    if (selectAll) {
      setSelectedIds(new Set());
      setSelectAll(false);
    } else {
      const ids = new Set(data.items.map((item) => item.id));
      setSelectedIds(ids);
      setSelectAll(true);
    }
  };

  const toggleSelectItem = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleRowClick = (item: Document) => {
    const basePath = documentViewPaths[item.document_type];
    if (basePath) {
      navigate(`${basePath}/${item.id}`);
    }
  };

  const handleConfirm = async () => {
    if (!confirmItem) return;
    try {
      await confirmMutation.mutateAsync({ id: confirmItem.id, documentType: confirmItem.type });
      setConfirmItem(null);
    } catch {
      // Error handled
    }
  };

  const handlePrint = (item: Document) => {
    // Отримуємо токен з authStore (він також зберігається в localStorage)
    const token = useAuthStore.getState().accessToken || localStorage.getItem('accessToken');
    // Відкриваємо в новій вкладці з токеном в URL (бекенд підтримує ?token=)
    const printUrl = `/api/v1/documents/${item.id}/print?document_type=${item.document_type}` +
      (token ? `&token=${encodeURIComponent(token)}` : '');
    window.open(printUrl, '_blank');
  };

  const handleDelete = async () => {
    if (!deleteItem) return;
    try {
      await deleteMutation.mutateAsync({ id: deleteItem.id, documentType: deleteItem.type });
      setDeleteItem(null);
    } catch {
      // Error handled
    }
  };

  const handleCopy = async (item: Document) => {
    try {
      await copyMutation.mutateAsync({ id: item.id, documentType: item.document_type });
    } catch {
      // Error handled
    }
  };

  const handleBatchConfirm = async () => {
    if (selectedIds.size === 0) return;
    const items = data?.items
      ?.filter((item) => selectedIds.has(item.id) && item.status === 'draft')
      .map((item) => ({
        id: item.id,
        document_type: item.document_type as DocumentType,
      })) || [];

    if (items.length === 0) {
      toast.error('Немає чернеток для підтвердження');
      return;
    }

    try {
      await batchConfirmMutation.mutateAsync(items);
      setSelectedIds(new Set());
      setSelectAll(false);
      setBatchConfirmOpen(false);
    } catch {
      // Error handled
    }
  };

  const handleExport = async () => {
    try {
      const params: any = { format: 'excel', detailed: true };
      if (filters.search) params.search = filters.search;
      if (filters.document_type) params.document_type = filters.document_type;
      if (filters.status) params.status = filters.status;
      if (filters.date_from) params.date_from = filters.date_from;
      if (filters.date_to) params.date_to = filters.date_to;
      if (filters.supplier_id) params.supplier_id = filters.supplier_id;
      if (filters.amount_from) params.amount_from = filters.amount_from;
      if (filters.amount_to) params.amount_to = filters.amount_to;
      if (selectedIds.size > 0) params.ids = Array.from(selectedIds).join(',');

      const blob = await documentService.exportDocuments(params);
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `nakladni_${new Date().toISOString().slice(0, 10)}.xlsx`;
      a.click();
      window.URL.revokeObjectURL(url);
      toast.success('Експорт виконано');
    } catch {
      toast.error('Помилка експорту');
    }
  };

  // --- Preset handlers ---

  const applyPreset = (preset: { name: string; filters: typeof filters }) => {
    setFilters(preset.filters);
    setPage(1);
    setShowAdvancedFilters(true);
  };

  const savePreset = () => {
    if (!presetName.trim()) {
      toast.error('Введіть назву пресета');
      return;
    }
    const newPreset = {
      name: presetName.trim(),
      filters: { ...filters },
      created_at: new Date().toISOString(),
    };
    const updated = [...savedPresets, newPreset];
    setSavedPresets(updated);
    localStorage.setItem('document_filter_presets', JSON.stringify(updated));
    setPresetName('');
    setShowSavePreset(false);
    toast.success('Пресет збережено');
  };

  const deletePreset = (index: number) => {
    const updated = savedPresets.filter((_, i) => i !== index);
    setSavedPresets(updated);
    localStorage.setItem('document_filter_presets', JSON.stringify(updated));
  };

  const resetFilters = () => {
    setFilters(defaultFilters);
    setPage(1);
  };

  const getDraftCount = (): number => {
    return data?.items?.filter((item) => item.status === 'draft' && selectedIds.has(item.id)).length || 0;
  };

  // --- Columns ---
  const columns: Column<Document>[] = [
    {
      key: 'select',
      header: '',
      render: (item) => {
        const isSelected = selectedIds.has(item.id);
        return (
          <input
            type="checkbox"
            checked={isSelected}
            onChange={() => toggleSelectItem(item.id)}
            onClick={(e) => e.stopPropagation()}
            className="w-4 h-4 rounded border-gray-300 dark:border-slate-600 text-primary-600 focus:ring-primary-500 cursor-pointer"
            aria-label={`Вибрати ${item.document_number}`}
          />
        );
      },
      width: '48px',
    },
    {
      key: 'document_number',
      header: '№',
      render: (item) => (
        <span className="font-medium text-primary-600 dark:text-primary-400 hover:underline cursor-pointer">
          {item.document_number}
        </span>
      ),
    },
    {
      key: 'document_type',
      header: 'Тип',
      render: (item) => (
        <Badge variant="primary">{formatDocumentType(item.document_type)}</Badge>
      ),
    },
    {
      key: 'supplier_name',
      header: 'Постачальник',
      render: (item) => item.supplier_name || '-',
    },
    {
      key: 'total_amount',
      header: 'Сума',
      render: (item) => {
        // Для прибуткових накладних — закупівельна сума
        // Для інвентаризації — сума відхилення
        // Для решти — загальна сума
        let amount: number;
        const extItem = item as any;
        if (item.document_type === 'inventory' && extItem.deviation_total != null) {
          amount = Number(extItem.deviation_total);
        } else if (item.document_type === 'invoice' && item.purchase_total != null) {
          amount = item.purchase_total;
        } else {
          amount = Number(item.total_amount);
        }
        return (
          <span className={`font-medium ${
            item.document_type === 'inventory'
              ? (amount > 0 ? 'text-green-600 dark:text-green-400' : amount < 0 ? 'text-red-600 dark:text-red-400' : '')
              : ''
          }`}>
            {formatCurrency(amount)}
          </span>
        );
      },
    },
    {
      key: 'status',
      header: 'Статус',
      render: (item) => (
        <Badge variant={statusBadgeVariant[item.status] || 'default'}>
          {formatDocumentStatus(item.status)}
        </Badge>
      ),
    },
    {
      key: 'created_at',
      header: 'Створено',
      render: (item) => (
        <span className="text-gray-500">{formatDateTime(item.created_at)}</span>
      ),
    },
    {
      key: 'created_by_name',
      header: 'Користувач',
      render: (item) => (
        <span className="text-gray-700 dark:text-gray-300">
          {item.created_by_name || '-'}
        </span>
      ),
    },
    {
      key: 'actions',
      header: 'Дії',
      render: (item) => (
        <div className="flex items-center gap-1">

          <button
            onClick={(e) => {
              e.stopPropagation();
              handlePrint(item);
            }}
            className="p-1.5 rounded-lg text-gray-400 hover:text-primary-600 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors"
            title="Друк накладної"
          >
            <Printer className="w-4 h-4" />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              handleCopy(item);
            }}
            className="p-1.5 rounded-lg text-gray-400 hover:text-primary-600 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors"
            title="Копіювати документ"
          >
            <Copy className="w-4 h-4" />
          </button>
          {item.status === 'draft' && (
            <>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setConfirmItem({ id: item.id, type: item.document_type });
                }}
                className="p-1.5 rounded-lg text-gray-400 hover:text-success-600 hover:bg-success-50 dark:hover:bg-success-900/20 transition-colors"
                title="Підтвердити"
              >
                <CheckCircle className="w-4 h-4" />
              </button>

              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setDeleteItem({ id: item.id, type: item.document_type });
                }}
                className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 dark:hover:bg-danger-900/20 transition-colors"
                title="Видалити"
              >
                <Trash2 className="w-4 h-4" />
              </button>
            </>
          )}
        </div>
      ),
    },
  ];

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Накладні</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Управління накладними та документами
          </p>
        </div>
        <div className="flex gap-2 flex-wrap">
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/invoice/new')}
            icon={<Plus className="w-4 h-4" />}
          >
            Накладна
          </Button>
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/purchase-order/new')}
            icon={<ShoppingCart className="w-4 h-4" />}
          >
            Замовлення
          </Button>
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/transfer/new')}
            icon={<Plus className="w-4 h-4" />}
          >
            Переміщення
          </Button>
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/write-off/new')}
            icon={<Plus className="w-4 h-4" />}
          >
            Списання
          </Button>
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/return/new')}
            icon={<Plus className="w-4 h-4" />}
          >
            Повернення
          </Button>
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/inventory/new')}
            icon={<Plus className="w-4 h-4" />}
          >
            Інвентаризація
          </Button>
        </div>
      </div>

      {/* Search and Filters */}
      <div className="card p-4 space-y-4">
        {/* Search row */}
        <div className="flex items-center gap-3">
          <div className="flex-1 relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 pointer-events-none" />
            <input
              type="text"
              value={filters.search}
              onChange={(e) => {
                setFilters((prev) => ({ ...prev, search: e.target.value }));
                setPage(1);
              }}
              placeholder="Пошук за номером..."
              className="input-field pl-10 pr-4 w-full"
              id="document-search"
              name="document-search"
              autoComplete="off"
            />
          </div>
          <Button
            variant="secondary"
            onClick={() => setShowAdvancedFilters((prev) => !prev)}
            icon={<Filter className="w-4 h-4" />}
          >
            Фільтри
            <ChevronDown className={`w-3 h-3 ml-1 transition-transform ${showAdvancedFilters ? 'rotate-180' : ''}`} />
          </Button>
          <Button
            variant="secondary"
            onClick={handleExport}
            icon={<Download className="w-4 h-4" />}
          >
            Експорт
          </Button>
        </div>

        {/* Advanced filters panel */}
        {showAdvancedFilters && (
          <div className="border-t border-gray-200 dark:border-slate-700 pt-4 space-y-4">
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
              {/* Date filter */}
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  📅 Дата від
                </label>
                <input
                  type="date"
                  value={filters.date_from}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, date_from: e.target.value }));
                    setPage(1);
                  }}
                  className="input-field w-full"
                  id="filter-date-from"
                  name="filter-date-from"
                />
              </div>
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  📅 Дата до
                </label>
                <input
                  type="date"
                  value={filters.date_to}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, date_to: e.target.value }));
                    setPage(1);
                  }}
                  className="input-field w-full"
                  id="filter-date-to"
                  name="filter-date-to"
                />
              </div>

              {/* Supplier filter */}
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  🏭 Постачальник
                </label>
                <Select
                  options={supplierFilterOptions}
                  value={filters.supplier_id}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, supplier_id: e.target.value }));
                    setPage(1);
                  }}
                />
              </div>

              {/* Status filter */}
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  📊 Статус
                </label>
                <Select
                  options={statusOptions}
                  value={filters.status}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, status: e.target.value }));
                    setPage(1);
                  }}
                />
              </div>

              {/* Amount filter */}
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  💰 Сума від
                </label>
                <input
                  type="number"
                  step="0.01"
                  min="0"
                  value={filters.amount_from}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, amount_from: e.target.value }));
                    setPage(1);
                  }}
                  placeholder="0.00"
                  className="input-field w-full"
                  id="filter-amount-from"
                  name="filter-amount-from"
                />
              </div>
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  💰 Сума до
                </label>
                <input
                  type="number"
                  step="0.01"
                  min="0"
                  value={filters.amount_to}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, amount_to: e.target.value }));
                    setPage(1);
                  }}
                  placeholder="0.00"
                  className="input-field w-full"
                  id="filter-amount-to"
                  name="filter-amount-to"
                />
              </div>

              {/* Document type filter */}
              <div className="space-y-1">
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400">
                  📄 Тип документа
                </label>
                <Select
                  options={documentTypeOptions}
                  value={filters.document_type}
                  onChange={(e) => {
                    setFilters((prev) => ({ ...prev, document_type: e.target.value }));
                    setPage(1);
                  }}
                />
              </div>
            </div>

            {/* Presets and actions */}
            <div className="flex flex-wrap items-center gap-2 pt-2 border-t border-gray-100 dark:border-slate-700">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setShowSavePreset(true)}
                icon={<Save className="w-3.5 h-3.5" />}
              >
                Зберегти пресет
              </Button>

              {/* Presets dropdown */}
              {savedPresets.length > 0 && (
                <div className="relative group">
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<Bookmark className="w-3.5 h-3.5" />}
                  >
                    Пресети
                    <ChevronDown className="w-3 h-3 ml-1" />
                  </Button>
                  <div className="absolute top-full left-0 z-50 mt-1 w-64 bg-white dark:bg-slate-700 border border-gray-200 dark:border-slate-600 rounded-lg shadow-lg hidden group-hover:block">
                    <div className="py-1 max-h-48 overflow-y-auto">
                      {savedPresets.map((preset, index) => (
                        <div
                          key={index}
                          className="flex items-center justify-between px-3 py-2 hover:bg-gray-50 dark:hover:bg-slate-600 cursor-pointer"
                          onClick={() => applyPreset(preset)}
                        >
                          <span className="text-sm text-gray-700 dark:text-gray-300">{preset.name}</span>
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              deletePreset(index);
                            }}
                            className="p-1 text-gray-400 hover:text-danger-500 transition-colors"
                            title="Видалити пресет"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              )}

              <Button
                variant="secondary"
                size="sm"
                onClick={resetFilters}
                icon={<X className="w-3.5 h-3.5" />}
              >
                Скинути всі фільтри
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* Select all checkbox */}
      {data && data.items.length > 0 && (
        <div className="flex items-center gap-2 px-1 mb-2">
          <input
            type="checkbox"
            checked={selectAll}
            onChange={toggleSelectAll}
            className="w-4 h-4 rounded border-gray-300 dark:border-slate-600 text-primary-600 focus:ring-primary-500 cursor-pointer"
          />
          <span className="text-xs text-gray-500">
            {selectAll ? 'Зняти всі' : 'Вибрати всі'}
          </span>
        </div>
      )}

      {/* Table */}
      <Table
        columns={columns}
        data={data?.items || []}
        isLoading={isLoading}
        error={error?.message}
        page={page}
        totalPages={data?.pages || 1}
        total={data?.total}
        onPageChange={setPage}
        onRowClick={handleRowClick}
        keyExtractor={(item) => item.id}
        emptyMessage="Накладних не знайдено"
        emptyIcon={<FileText className="w-12 h-12" />}
      />

      {/* Batch actions panel */}
      {selectedIds.size > 0 && (
        <div className="fixed bottom-0 left-0 right-0 z-50 p-4 bg-white dark:bg-slate-800 border-t border-gray-200 dark:border-slate-700 shadow-2xl">
          <div className="max-w-7xl mx-auto flex items-center justify-between">
            <div className="flex items-center gap-3">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                ✅ Вибрано: {selectedIds.size} {selectedIds.size === 1 ? 'документ' : 'документів'}
              </span>
              <span className="text-xs text-gray-400">
                (натисніть Escape щоб зняти вибір)
              </span>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="primary"
                size="sm"
                onClick={() => setBatchConfirmOpen(true)}
                icon={<CheckCircle className="w-4 h-4" />}
              >
                Підтвердити всі ({getDraftCount()})
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={handleExport}
                icon={<Download className="w-4 h-4" />}
              >
                Експорт вибраних
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  setSelectedIds(new Set());
                  setSelectAll(false);
                }}
                icon={<X className="w-4 h-4" />}
              >
                Скасувати
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm dialog for single document */}
      <ConfirmDialog
        isOpen={confirmItem !== null}
        onClose={() => setConfirmItem(null)}
        onConfirm={handleConfirm}
        title="Підтвердити документ?"
        message="Після підтвердження документ вплине на залишки товарів."
        confirmText="Підтвердити"
        variant="primary"
        isLoading={confirmMutation.isPending}
      />

      {/* Delete dialog */}
      <ConfirmDialog
        isOpen={deleteItem !== null}
        onClose={() => setDeleteItem(null)}
        onConfirm={handleDelete}
        title="Видалити документ?"
        message="Цю дію не можна скасувати. Документ буде видалено назавжди."
        confirmText="Видалити"
        cancelText="Скасувати"
        variant="danger"
        isLoading={deleteMutation.isPending}
      />

      {/* Batch confirm dialog */}
      <ConfirmDialog
        isOpen={batchConfirmOpen}
        onClose={() => setBatchConfirmOpen(false)}
        onConfirm={handleBatchConfirm}
        title="Масове підтвердження"
        message={`Підтвердити ${getDraftCount()} чернеток?`}
        confirmText="Підтвердити всі"
        variant="primary"
        isLoading={batchConfirmMutation.isPending}
      />

      {/* Save preset modal */}
      <Modal
        isOpen={showSavePreset}
        onClose={() => setShowSavePreset(false)}
        title="Зберегти пресет фільтрів"
        size="sm"
      >
        <div className="space-y-4">
          <Input
            label="Назва пресета"
            value={presetName}
            onChange={(e) => setPresetName(e.target.value)}
            placeholder="Наприклад: Мої накладні за тиждень"
            autoFocus
            id="preset-name"
            name="preset-name"
          />
          <div className="flex justify-end gap-3 pt-2 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => setShowSavePreset(false)}>
              Скасувати
            </Button>
            <Button onClick={savePreset}>
              Зберегти
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};

export default DocumentListPage;
