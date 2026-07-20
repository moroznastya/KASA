import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Edit, Trash2, Search, Phone, Mail } from 'lucide-react';
import { useSuppliers, useDeleteSupplier } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { SearchInput } from '@/components/ui/SearchInput';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Badge } from '@/components/ui/Badge';
import { formatCurrency } from '@/utils/format';
import { Supplier } from '@/types/supplier';

const SupplierListPage: React.FC = () => {
  const navigate = useNavigate();
  const [search, setSearch] = useState('');
  const [page, setPage] = useState(1);
  const [deleteId, setDeleteId] = useState<string | null>(null);

  const { data, isLoading, error } = useSuppliers({
    page,
    size: 20,
    search: search || undefined,
  });

  const deleteMutation = useDeleteSupplier();

  const columns: Column<Supplier>[] = [
    {
      key: 'name',
      header: 'Назва',
      render: (item) => (
        <div>
          <p className="font-medium text-gray-900 dark:text-gray-100">{item.name}</p>
          <p className="text-xs text-gray-400">Код: {item.code}</p>
        </div>
      ),
    },
    {
      key: 'contact_person',
      header: 'Контактна особа',
      render: (item) => item.contact_person || '-',
    },
    {
      key: 'phone',
      header: 'Телефон',
      render: (item) =>
        item.phone ? (
          <div className="flex items-center gap-1">
            <Phone className="w-3.5 h-3.5 text-gray-400" />
            <span>{item.phone}</span>
          </div>
        ) : (
          '-'
        ),
    },
    {
      key: 'email',
      header: 'Email',
      render: (item) =>
        item.email ? (
          <div className="flex items-center gap-1">
            <Mail className="w-3.5 h-3.5 text-gray-400" />
            <span className="text-sm">{item.email}</span>
          </div>
        ) : (
          '-'
        ),
    },
    {
      key: 'balance',
      header: 'Баланс',
      render: (item) => (
        <span
          className={`font-medium ${
            parseFloat(item.balance) > 0
              ? 'text-danger-600'
              : parseFloat(item.balance) < 0
              ? 'text-success-600'
              : ''
          }`}
        >
          {formatCurrency(item.balance)}
        </span>
      ),
    },
    {
      key: 'is_active',
      header: 'Статус',
      render: (item) => (
        <Badge variant={item.is_active ? 'success' : 'default'}>
          {item.is_active ? 'Активний' : 'Неактивний'}
        </Badge>
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
              navigate(`/suppliers/${item.id}/edit`);
            }}
            className="p-1.5 rounded-lg text-gray-400 hover:text-primary-600 hover:bg-primary-50 dark:hover:bg-primary-900/20 transition-colors"
          >
            <Edit className="w-4 h-4" />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              setDeleteId(item.id);
            }}
            className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 dark:hover:bg-danger-900/20 transition-colors"
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      ),
    },
  ];

  const handleDelete = async () => {
    if (!deleteId) return;
    try {
      await deleteMutation.mutateAsync(deleteId);
      setDeleteId(null);
    } catch {
      // Error handled
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Постачальники</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Управління постачальниками
          </p>
        </div>
        <Button
          onClick={() => navigate('/suppliers/new')}
          icon={<Plus className="w-4 h-4" />}
        >
          Додати постачальника
        </Button>
      </div>

      <SearchInput
        value={search}
        onChange={(v) => {
          setSearch(v);
          setPage(1);
        }}
        placeholder="Пошук за назвою або кодом..."
        className="max-w-md"
      />

      <Table
        columns={columns}
        data={data?.items || []}
        isLoading={isLoading}
        error={error?.message}
        page={page}
        totalPages={data?.pages || 1}
        total={data?.total}
        onPageChange={setPage}
        onRowClick={(item) => navigate(`/suppliers/${item.id}/edit`)}
        keyExtractor={(item) => item.id}
        emptyMessage="Постачальників не знайдено"
        emptyIcon={<Search className="w-12 h-12" />}
      />

      <ConfirmDialog
        isOpen={deleteId !== null}
        onClose={() => setDeleteId(null)}
        onConfirm={handleDelete}
        title="Видалити постачальника?"
        message="Постачальника буде видалено з системи."
        confirmText="Видалити"
        variant="danger"
        isLoading={deleteMutation.isPending}
      />
    </div>
  );
};

export default SupplierListPage;
