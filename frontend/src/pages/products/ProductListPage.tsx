import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Plus, Edit, Trash2, Search } from 'lucide-react';
import { useProducts, useDeleteProduct } from '@/hooks/useProducts';
import { useCategories } from '@/hooks/useCategories';
import { Button } from '@/components/ui/Button';
import { Table, Column } from '@/components/ui/Table';
import { SearchInput } from '@/components/ui/SearchInput';
import { Select } from '@/components/ui/Select';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Badge } from '@/components/ui/Badge';
import { formatCurrency, formatUnit } from '@/utils/format';
import { Product } from '@/types/product';
import toast from 'react-hot-toast';

const ProductListPage: React.FC = () => {
  const navigate = useNavigate();
  const [search, setSearch] = useState('');
  const [categoryFilter, setCategoryFilter] = useState('');
  const [page, setPage] = useState(1);
  const [deleteId, setDeleteId] = useState<string | null>(null);

  const { data, isLoading, error } = useProducts({
    page,
    size: 20,
    search: search || undefined,
    category_id: categoryFilter || undefined,
  });

  const { data: categories } = useCategories();
  const deleteMutation = useDeleteProduct();

  const columns: Column<Product>[] = [
    {
      key: 'name',
      header: 'Назва',
      render: (item) => (
        <div>
          <p className="font-medium text-gray-900 dark:text-gray-100">{item.name}</p>
          {item.barcode && (
            <p className="text-xs text-gray-400">ШК: {item.barcode}</p>
          )}
        </div>
      ),
    },
    {
      key: 'barcode',
      header: 'Штрих-код',
      render: (item) => item.barcode || '-',
    },
    {
      key: 'price',
      header: 'Ціна',
      render: (item) => (
        <span className="font-medium">{formatCurrency(item.price)}</span>
      ),
    },
    {
      key: 'stock',
      header: 'Залишок',
      render: (item) => (
        <span className={item.stock <= 0 ? 'text-danger-600 font-medium' : ''}>
          {item.stock} {formatUnit(item.unit)}
        </span>
      ),
    },
    {
      key: 'category_name',
      header: 'Категорія',
      render: (item) => item.category_name || '-',
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
              navigate(`/products/${item.id}/edit`);
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
      // Error handled by mutation
    }
  };

  const categoryOptions = [
    { value: '', label: 'Всі категорії' },
    ...(categories?.map((cat) => ({
      value: String(cat.id),
      label: cat.name,
    })) || []),
  ];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Товари</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Управління товарами та послугами
          </p>
        </div>
        <Button
          onClick={() => navigate('/products/new')}
          icon={<Plus className="w-4 h-4" />}
        >
          Додати товар
        </Button>
      </div>

      {/* Filters */}
      <div className="flex flex-col sm:flex-row gap-4">
        <SearchInput
          value={search}
          onChange={(v) => {
            setSearch(v);
            setPage(1);
          }}
          placeholder="Пошук за назвою або штрих-кодом..."
          className="flex-1"
        />
        <Select
          options={categoryOptions}
          value={categoryFilter}
          onChange={(e) => {
            setCategoryFilter(e.target.value);
            setPage(1);
          }}
          className="w-full sm:w-48"
        />
      </div>

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
        onRowClick={(item) => navigate(`/products/${item.id}/edit`)}
        keyExtractor={(item) => item.id}
        emptyMessage="Товари не знайдено"
        emptyIcon={<Search className="w-12 h-12" />}
      />

      {/* Delete confirmation */}
      <ConfirmDialog
        isOpen={deleteId !== null}
        onClose={() => setDeleteId(null)}
        onConfirm={handleDelete}
        title="Видалити товар?"
        message="Ця дія незворотна. Товар буде видалено з системи."
        confirmText="Видалити"
        variant="danger"
        isLoading={deleteMutation.isPending}
      />
    </div>
  );
};

export default ProductListPage;
