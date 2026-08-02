import React, { useState } from 'react';
import { Plus, Edit2, Trash2, ChevronDown, ChevronRight, FolderOpen } from 'lucide-react';
import { useCategoryTree, useCreateCategory, useUpdateCategory, useDeleteCategory } from '@/hooks/useCategories';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Spinner } from '@/components/ui/Spinner';
import { Category, CategoryCreate } from '@/types/product';

const CategoryListPage: React.FC = () => {
  const { data: categories, isLoading } = useCategoryTree();
  const createMutation = useCreateCategory();
  const updateMutation = useUpdateCategory();
  const deleteMutation = useDeleteCategory();

  const [modalOpen, setModalOpen] = useState(false);
  const [editItem, setEditItem] = useState<Category | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [form, setForm] = useState<CategoryCreate>({ name: '', parent_id: null });
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  const handleOpenCreate = (parentId?: string) => {
    setEditItem(null);
    setForm({ name: '', parent_id: parentId ?? null });
    setModalOpen(true);
  };

  const handleOpenEdit = (category: Category) => {
    setEditItem(category);
    setForm({ name: category.name, parent_id: category.parent_id });
    setModalOpen(true);
  };

  const handleSave = async () => {
    if (!form.name.trim()) return;

    try {
      if (editItem) {
        await updateMutation.mutateAsync({
          id: editItem.id,
          data: { ...form, id: editItem.id },
        });
      } else {
        await createMutation.mutateAsync(form);
      }
      setModalOpen(false);
      setForm({ name: '', parent_id: null });
      setEditItem(null);
    } catch {
      // Error handled by mutation
    }
  };

  const handleDelete = async () => {
    if (!deleteId) return;
    try {
      await deleteMutation.mutateAsync(deleteId);
      setDeleteId(null);
    } catch {
      // Error handled
    }
  };

  const toggleExpand = (id: string) => {
    setExpandedIds((prev) => {
      const newSet = new Set(prev);
      if (newSet.has(id)) {
        newSet.delete(id);
      } else {
        newSet.add(id);
      }
      return newSet;
    });
  };

  const renderCategory = (category: Category, depth: number = 0) => {
    const hasChildren = Array.isArray(category.children) && category.children.length > 0;
    const isExpanded = expandedIds.has(category.id);

    return (
      <div key={category.id}>
        <div
          className={`
            flex items-center gap-2 px-3 py-2.5 rounded-lg
            hover:bg-gray-50 dark:hover:bg-slate-700/50
            transition-colors group
          `}
          style={{ paddingLeft: `${16 + depth * 24}px` }}
        >
          {hasChildren ? (
            <button
              onClick={() => toggleExpand(category.id)}
              className="p-0.5 rounded text-gray-400 hover:text-gray-600"
            >
              {isExpanded ? (
                <ChevronDown className="w-4 h-4" />
              ) : (
                <ChevronRight className="w-4 h-4" />
              )}
            </button>
          ) : (
            <div className="w-5" />
          )}
          <FolderOpen className="w-4 h-4 text-warning-500 flex-shrink-0" />
          <span className="flex-1 text-sm font-medium text-gray-700 dark:text-gray-300">
            {category.name}
          </span>
          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
            <button
              onClick={() => handleOpenCreate(category.id)}
              className="p-1 rounded text-gray-400 hover:text-primary-600 hover:bg-primary-50 dark:hover:bg-primary-900/20"
              title="Додати підкатегорію"
            >
              <Plus className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => handleOpenEdit(category)}
              className="p-1 rounded text-gray-400 hover:text-primary-600 hover:bg-primary-50 dark:hover:bg-primary-900/20"
              title="Редагувати"
            >
              <Edit2 className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => setDeleteId(category.id)}
              className="p-1 rounded text-gray-400 hover:text-danger-600 hover:bg-danger-50 dark:hover:bg-danger-900/20"
              title="Видалити"
            >
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
        {hasChildren && isExpanded && (
          <div>
            {Array.isArray(category.children) && category.children.map((child) => renderCategory(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  const parentOptions = [
    { value: '', label: 'Коренева категорія' },
    ...(categories?.map((cat) => ({
      value: String(cat.id),
      label: cat.name,
    })) || []),
  ];

  if (isLoading) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="lg" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Категорії</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Управління категоріями товарів
          </p>
        </div>
        <Button onClick={() => handleOpenCreate()} icon={<Plus className="w-4 h-4" />}>
          Додати категорію
        </Button>
      </div>

      <div className="card divide-y divide-gray-200 dark:divide-slate-700">
        {!Array.isArray(categories) || categories.length === 0 ? (
          <div className="text-center py-12 text-gray-400">
            <FolderOpen className="w-12 h-12 mx-auto mb-2 opacity-50" />
            <p>Категорії не створені</p>
          </div>
        ) : (
          categories.map((cat) => renderCategory(cat))
        )}
      </div>

      {/* Create/Edit Modal */}
      <Modal
        isOpen={modalOpen}
        onClose={() => {
          setModalOpen(false);
          setEditItem(null);
          setForm({ name: '', parent_id: null });
        }}
        title={editItem ? 'Редагувати категорію' : 'Нова категорія'}
        size="sm"
      >
        <div className="space-y-4">
          <Input
            label="Назва категорії"
            value={form.name}
            onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))}
            placeholder="Введіть назву"
            autoFocus
          />
          <Select
            label="Батьківська категорія"
            options={parentOptions}
            value={String(form.parent_id || '')}
            onChange={(e) =>
              setForm((prev) => ({
                ...prev,
                parent_id: e.target.value || null,
              }))
            }
          />
          <div className="flex justify-end gap-3 pt-2">
            <Button
              variant="secondary"
              onClick={() => {
                setModalOpen(false);
                setEditItem(null);
              }}
            >
              Скасувати
            </Button>
            <Button
              onClick={handleSave}
              isLoading={createMutation.isPending || updateMutation.isPending}
              disabled={!form.name.trim()}
            >
              {editItem ? 'Зберегти' : 'Створити'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Delete confirmation */}
      <ConfirmDialog
        isOpen={deleteId !== null}
        onClose={() => setDeleteId(null)}
        onConfirm={handleDelete}
        title="Видалити категорію?"
        message="Категорію буде видалено. Якщо категорія має підкатегорії, вони також будуть видалені."
        confirmText="Видалити"
        variant="danger"
        isLoading={deleteMutation.isPending}
      />
    </div>
  );
};

export default CategoryListPage;
