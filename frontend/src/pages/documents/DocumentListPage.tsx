import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, FileText, Search, CheckCircle, XCircle, Eye } from 'lucide-react';
import { useDocuments, useConfirmDocument, useCancelDocument } from '@/hooks/useDocuments';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { SearchInput } from '@/components/ui/SearchInput';
import { Select } from '@/components/ui/Select';
import { Badge } from '@/components/ui/Badge';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { formatCurrency, formatDateTime, formatDocumentType, formatDocumentStatus } from '@/utils/format';
import { Document, DocumentType } from '@/types/document';

const documentTypeOptions = [
  { value: '', label: 'Всі типи' },
  { value: 'invoice', label: 'Прибуткові накладні' },
  { value: 'transfer', label: 'Переміщення' },
  { value: 'write_off', label: 'Списання' },
  { value: 'return_invoice', label: 'Повернення' },
];

const statusBadgeVariant: Record<string, 'default' | 'success' | 'danger' | 'warning'> = {
  draft: 'warning',
  confirmed: 'success',
  cancelled: 'danger',
};

/** Мапа типів документів -> шлях для перегляду */
const documentViewPaths: Record<string, string> = {
  invoice: '/documents/invoice',
  transfer: '/documents/transfer',
  write_off: '/documents/write-off',
  return_invoice: '/documents/return',
};

const DocumentListPage: React.FC = () => {
  const navigate = useNavigate();
  const [search, setSearch] = useState('');
  const [typeFilter, setTypeFilter] = useState('');
  const [page, setPage] = useState(1);
  const [confirmItem, setConfirmItem] = useState<{ id: string; type?: DocumentType } | null>(null);
  const [cancelItem, setCancelItem] = useState<{ id: string; type?: DocumentType } | null>(null);

  const { data, isLoading, error } = useDocuments({
    page,
    size: 20,
    search: search || undefined,
    document_type: (typeFilter as DocumentType) || undefined,
  });

  const confirmMutation = useConfirmDocument();
  const cancelMutation = useCancelDocument();

  /** Навігація до перегляду документа */
  const handleRowClick = (item: Document) => {
    const basePath = documentViewPaths[item.document_type];
    if (basePath) {
      navigate(`${basePath}/${item.id}`);
    }
  };

  const columns: Column<Document>[] = [
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
      render: (item) => (
        <span className="font-medium">{formatCurrency(item.total_amount)}</span>
      ),
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
      key: 'actions',
      header: 'Дії',
      render: (item) => (
        <div className="flex items-center gap-2">
          <button
            onClick={(e) => {
              e.stopPropagation();
              handleRowClick(item);
            }}
            className="p-1.5 rounded-lg text-gray-400 hover:text-primary-600 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors"
            title="Переглянути"
          >
            <Eye className="w-4 h-4" />
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
                  setCancelItem({ id: item.id, type: item.document_type });
                }}
                className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 dark:hover:bg-danger-900/20 transition-colors"
                title="Скасувати"
              >
                <XCircle className="w-4 h-4" />
              </button>
            </>
          )}
        </div>
      ),
    },
  ];

  const handleConfirm = async () => {
    if (!confirmItem) return;
    try {
      await confirmMutation.mutateAsync({ id: confirmItem.id, documentType: confirmItem.type });
      setConfirmItem(null);
    } catch {
      // Error handled
    }
  };

  const handleCancel = async () => {
    if (!cancelItem) return;
    try {
      await cancelMutation.mutateAsync({ id: cancelItem.id, documentType: cancelItem.type });
      setCancelItem(null);
    } catch {
      // Error handled
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Документи</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Управління документами
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="secondary"
            onClick={() => navigate('/documents/invoice/new')}
            icon={<Plus className="w-4 h-4" />}
          >
            Накладна
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
        </div>
      </div>

      {/* Пошук і фільтр — однакової ширини */}
      <div className="flex flex-col sm:flex-row gap-4">
        <SearchInput
          value={search}
          onChange={(v) => {
            setSearch(v);
            setPage(1);
          }}
          placeholder="Пошук за номером..."
          className="flex-1"
        />
        <Select
          options={documentTypeOptions}
          value={typeFilter}
          onChange={(e) => {
            setTypeFilter(e.target.value);
            setPage(1);
          }}
          containerClassName="flex-1"
        />
      </div>

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
        emptyMessage="Документів не знайдено"
        emptyIcon={<FileText className="w-12 h-12" />}
      />

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

      <ConfirmDialog
        isOpen={cancelItem !== null}
        onClose={() => setCancelItem(null)}
        onConfirm={handleCancel}
        title="Скасувати документ?"
        message="Скасований документ не можна буде підтвердити."
        confirmText="Скасувати"
        variant="danger"
        isLoading={cancelMutation.isPending}
      />
    </div>
  );
};

export default DocumentListPage;
