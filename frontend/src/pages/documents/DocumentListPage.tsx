import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, FileText, Search, CheckCircle, XCircle } from 'lucide-react';
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

export const DocumentListPage: React.FC = () => {
  const navigate = useNavigate();
  const [search, setSearch] = useState('');
  const [typeFilter, setTypeFilter] = useState('');
  const [page, setPage] = useState(1);
  const [confirmId, setConfirmId] = useState<number | null>(null);
  const [cancelId, setCancelId] = useState<number | null>(null);

  const { data, isLoading, error } = useDocuments({
    page,
    size: 20,
    search: search || undefined,
    document_type: (typeFilter as DocumentType) || undefined,
  });

  const confirmMutation = useConfirmDocument();
  const cancelMutation = useCancelDocument();

  const columns: Column<Document>[] = [
    {
      key: 'document_number',
      header: '№',
      render: (item) => (
        <span className="font-medium text-gray-900 dark:text-gray-100">
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
          {item.status === 'draft' && (
            <>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setConfirmId(item.id);
                }}
                className="p-1.5 rounded-lg text-gray-400 hover:text-success-600 hover:bg-success-50 dark:hover:bg-success-900/20 transition-colors"
                title="Підтвердити"
              >
                <CheckCircle className="w-4 h-4" />
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setCancelId(item.id);
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
    if (!confirmId) return;
    try {
      await confirmMutation.mutateAsync(confirmId);
      setConfirmId(null);
    } catch {
      // Error handled
    }
  };

  const handleCancel = async () => {
    if (!cancelId) return;
    try {
      await cancelMutation.mutateAsync(cancelId);
      setCancelId(null);
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
          className="w-full sm:w-56"
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
        keyExtractor={(item) => item.id}
        emptyMessage="Документів не знайдено"
        emptyIcon={<FileText className="w-12 h-12" />}
      />

      <ConfirmDialog
        isOpen={confirmId !== null}
        onClose={() => setConfirmId(null)}
        onConfirm={handleConfirm}
        title="Підтвердити документ?"
        message="Після підтвердження документ вплине на залишки товарів."
        confirmText="Підтвердити"
        variant="primary"
        isLoading={confirmMutation.isPending}
      />

      <ConfirmDialog
        isOpen={cancelId !== null}
        onClose={() => setCancelId(null)}
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
