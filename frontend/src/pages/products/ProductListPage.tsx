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

import { usePageState } from '@/hooks/usePageState';
const ProductListPage: React.FC = () => {
  const navigate = useNavigate();
  const [pageState, setPageState] = usePageState('product_list', {
    search: '',
    categoryFilter: '',
    page: 1,
  });
  const { search, categoryFilter, page } = pageState;
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
      key: 'title',
      header: 'Назва',
      render: (item) => (
        <div>
          <p className="font-medium text-gray-900 dark:text-gray-100">{item.title}</p>
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
      render: (item) => {
        const stockNum = parseFloat(item.stock);
        return (
          <span className={stockNum <= 0 ? 'text-danger-600 font-medium' : ''}>
            {item.stock} {formatUnit(item.unit)}
          </span>
        );
      },
    },
    {
      key: 'sku',
      header: 'Артикул',
      render: (item) => item.sku || '-',
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

      {/* Filters — пошук = Назва + Штрих-код, категорії = Залишок + Артикул + Дії */}
      <div className="flex flex-col sm:flex-row gap-4">
        <div className="flex-[2]">
          <SearchInput
            value={search}
            onChange={(v) => {
              setPageState({ search: v, page: 1 });
            }}
            placeholder="Пошук за назвою або штрих-кодом..."
            className="w-full"
          />
        </div>
        <div className="flex-[3]">
          <Select
            options={categoryOptions}
            value={categoryFilter}
            onChange={(e) => {
              setPageState({ categoryFilter: e.target.value, page: 1 });
            }}
            className="w-full"
          />
        </div>
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
        onPageChange={(n) => setPageState({ page: n })}
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
