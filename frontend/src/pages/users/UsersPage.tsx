import React, { useState, useEffect } from 'react';
import {
  Users,
  Search,
  Loader2,
  UserPlus,
  Shield,
  ShieldOff,
  Pencil,
  Trash2,
  Key,
  CheckCircle,
  XCircle,
  Lock,
} from 'lucide-react';
import { userService, UserCreate, UserUpdate } from '@/services/userService';
import { User } from '@/types/auth';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { UserPermissionsModal } from './UserPermissionsModal';
import toast from 'react-hot-toast';

const UsersPage: React.FC = () => {
  const [users, setUsers] = useState<User[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');

  // Create / Edit modal
  const [showFormModal, setShowFormModal] = useState(false);
  const [editingUser, setEditingUser] = useState<User | null>(null);
  const [formData, setFormData] = useState<UserCreate>({
    name: '',
    login: '',
    password: '',
    pin_code: '',
    role: 'cashier',
    is_active: true,
  });
  const [isSaving, setIsSaving] = useState(false);

  // Delete confirm
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  // Permissions modal
  const [permissionsTarget, setPermissionsTarget] = useState<User | null>(null);

  const loadUsers = async () => {
    setIsLoading(true);
    try {
      const data = await userService.list();
      setUsers(data);
    } catch {
      toast.error('Помилка завантаження списку користувачів');
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadUsers();
  }, []);

  const openCreateModal = () => {
    setEditingUser(null);
    setFormData({
      name: '',
      password: '',
      pin_code: '',
      role: 'cashier',
      is_active: true,
    });
    setShowFormModal(true);
  };

  const openEditModal = (user: User) => {
    setEditingUser(user);
    setFormData({
      name: user.name,
      password: '',
      pin_code: '',
      role: user.role as 'admin' | 'cashier',
      is_active: user.is_active,
    });
    setShowFormModal(true);
  };

  const handleSave = async () => {
    if (!formData.name.trim()) {
      toast.error("Введіть ім'я користувача");
      return;
    }
    if (!editingUser && !formData.password) {
      toast.error('Введіть пароль');
      return;
    }

    setIsSaving(true);
    try {
      if (editingUser) {
        const updateData: UserUpdate = {
          name: formData.name,
          role: formData.role,
          is_active: formData.is_active,
        };
        if (formData.password) updateData.password = formData.password;
        if (formData.pin_code) updateData.pin_code = formData.pin_code;

        await userService.update(editingUser.id, updateData);
        toast.success(`Користувача "${formData.name}" оновлено`);
      } else {
        await userService.create(formData);
        toast.success(`Користувача "${formData.name}" створено`);
      }
      setShowFormModal(false);
      loadUsers();
    } catch (err: any) {
      const detail = err?.response?.data?.detail;
      if (Array.isArray(detail)) {
        const messages = detail.map((d: any) => {
          const field = d.loc?.slice(1).join('.') || '';
          return field ? `${field}: ${d.msg}` : d.msg;
        });
        toast.error(messages.join('\n') || 'Помилка валідації даних');
      } else if (typeof detail === 'string') {
        toast.error(detail);
      } else {
        toast.error('Помилка збереження користувача');
      }
    } finally {
      setIsSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setIsDeleting(true);
    try {
      await userService.delete(deleteTarget.id);
      toast.success(`Користувача "${deleteTarget.name}" видалено`);
      setDeleteTarget(null);
      loadUsers();
    } catch {
      toast.error('Помилка видалення користувача');
    } finally {
      setIsDeleting(false);
    }
  };

  const filteredUsers = users.filter(
    (u) =>
      u.name.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const roleBadge = (role: string) => {
    switch (role) {
      case 'admin':
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-purple-100 text-purple-800 dark:bg-purple-900/30 dark:text-purple-400">
            <Shield className="w-3 h-3" />
            Адміністратор
          </span>
        );
      case 'cashier':
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-400">
            <ShoppingCartIcon className="w-3 h-3" />
            Касир
          </span>
        );
      default:
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-300">
            {role}
          </span>
        );
    }
  };

  return (
    <div className="p-6 max-w-7xl mx-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Користувачі
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Управління користувачами системи та їх правами доступу
          </p>
        </div>
        <Button onClick={openCreateModal}>
          <UserPlus className="w-4 h-4 mr-2" />
          Новий користувач
        </Button>
      </div>

      {/* Search */}
      <div className="relative mb-6">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-gray-400" />
        <input
          type="text"
          placeholder="Пошук за ім'ям..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="w-full pl-10 pr-4 py-2.5 rounded-lg border border-gray-300 dark:border-slate-600 
                     bg-white dark:bg-slate-800 text-gray-900 dark:text-gray-100
                     placeholder-gray-400 dark:placeholder-gray-500
                     focus:ring-2 focus:ring-primary-500 focus:border-transparent
                     transition-all duration-150"
        />
      </div>

      {/* Loading */}
      {isLoading ? (
        <div className="flex items-center justify-center py-20">
          <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
        </div>
      ) : filteredUsers.length === 0 ? (
        <div className="text-center py-20">
          <Users className="w-16 h-16 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
          <p className="text-gray-500 dark:text-gray-400">
            {searchQuery ? 'Нічого не знайдено' : 'Ще немає користувачів'}
          </p>
          {!searchQuery && (
            <Button variant="secondary" className="mt-4" onClick={openCreateModal}>
              <UserPlus className="w-4 h-4 mr-2" />
              Створити першого користувача
            </Button>
          )}
        </div>
      ) : (
        <div className="grid gap-4">
          {filteredUsers.map((user) => (
            <div
              key={user.id}
              className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 
                         p-4 flex items-center gap-4 hover:shadow-md transition-shadow duration-200"
            >
              {/* Avatar */}
              <div className="w-12 h-12 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center flex-shrink-0">
                <span className="text-xl font-medium text-primary-700 dark:text-primary-400">
                  {user.name.charAt(0).toUpperCase()}
                </span>
              </div>

              {/* Info */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <h3 className="font-medium text-gray-900 dark:text-gray-100 truncate">
                    {user.name}
                  </h3>
                  {user.is_active ? (
                    <CheckCircle className="w-4 h-4 text-green-500 flex-shrink-0" />
                  ) : (
                    <XCircle className="w-4 h-4 text-red-500 flex-shrink-0" />
                  )}
                </div>
                <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
                  <span>{user.login}</span>
                  <span>·</span>
                  {roleBadge(user.role)}
                  {user.permissions && user.permissions.length > 0 && (
                    <>
                      <span>·</span>
                      <span className="inline-flex items-center gap-1 text-xs text-gray-400">
                        <Lock className="w-3 h-3" />
                        {user.permissions.length} прав
                      </span>
                    </>
                  )}
                </div>
              </div>

              {/* Actions */}
              <div className="flex items-center gap-2 flex-shrink-0">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => setPermissionsTarget(user)}
                  title="Редагувати права доступу"
                >
                  <Lock className="w-4 h-4 mr-1" />
                  Права
                </Button>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => openEditModal(user)}
                  title="Редагувати користувача"
                >
                  <Pencil className="w-4 h-4" />
                </Button>
                <Button
                  variant="danger"
                  size="sm"
                  onClick={() => setDeleteTarget(user)}
                  title="Видалити користувача"
                >
                  <Trash2 className="w-4 h-4" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Create / Edit Modal */}
      <Modal
        isOpen={showFormModal}
        onClose={() => setShowFormModal(false)}
        title={editingUser ? 'Редагувати користувача' : 'Новий користувач'}
        size="md"
      >
        <div className="space-y-4">
          <Input
            label="Ім'я"
            value={formData.name}
            onChange={(e) => setFormData({ ...formData, name: e.target.value })}
            placeholder="Повне ім'я користувача"
            autoFocus
            id="user-name"
            name="user-name"
          />

          <Input
            label={editingUser ? 'Новий пароль (залиште порожнім, щоб не змінювати)' : 'Пароль'}
            type="password"
            value={formData.password}
            onChange={(e) => setFormData({ ...formData, password: e.target.value })}
            placeholder={editingUser ? 'Новий пароль' : 'Мінімум 4 символи'}
            icon={<Key className="w-4 h-4" />}
            id="user-password"
            name="user-password"
          />

          <Input
            label="PIN-код для каси (необов'язково)"
            type="password"
            value={formData.pin_code || ''}
            onChange={(e) => setFormData({ ...formData, pin_code: e.target.value })}
            placeholder="4-10 цифр"
            id="user-pin"
            name="user-pin"
          />

          <Select
            label="Роль"
            value={formData.role}
            onChange={(e) => setFormData({ ...formData, role: e.target.value as 'admin' | 'cashier' })}
            options={[
              { value: 'cashier', label: 'Касир' },
              { value: 'admin', label: 'Адміністратор' },
            ]}
            id="user-role"
            name="user-role"
          />

          <div className="flex items-center gap-3 pt-2">
            <label className="relative inline-flex items-center cursor-pointer">
              <input
                type="checkbox"
                checked={formData.is_active ?? true}
                onChange={(e) => setFormData({ ...formData, is_active: e.target.checked })}
                className="sr-only peer"
              />
              <div className="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary-300 dark:peer-focus:ring-primary-800 rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary-600"></div>
              <span className="ms-3 text-sm font-medium text-gray-700 dark:text-gray-300">
                Активний користувач
              </span>
            </label>
          </div>

          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={() => setShowFormModal(false)}>
              Скасувати
            </Button>
            <Button onClick={handleSave} isLoading={isSaving}>
              {editingUser ? 'Зберегти' : 'Створити'}
            </Button>
          </div>
        </div>
      </Modal>

      {/* Delete confirm */}
      <ConfirmDialog
        isOpen={!!deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDelete}
        title="Видалити користувача"
        message={
          deleteTarget
            ? `Ви впевнені, що хочете видалити користувача "${deleteTarget.name}"?`
            : ''
        }
        confirmText="Видалити"
        isLoading={isDeleting}
        variant="danger"
      />

      {/* Permissions modal */}
      {permissionsTarget && (
        <UserPermissionsModal
          user={permissionsTarget}
          isOpen={!!permissionsTarget}
          onClose={() => setPermissionsTarget(null)}
          onSaved={loadUsers}
        />
      )}
    </div>
  );
};

// Додатковий компонент для іконки ShoppingCart в roleBadge
const ShoppingCartIcon: React.FC<{ className?: string }> = ({ className }) => (
  <svg
    className={className}
    fill="none"
    viewBox="0 0 24 24"
    strokeWidth={1.5}
    stroke="currentColor"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      d="M2.25 3h1.386c.51 0 .955.343 1.087.835l.383 1.437M7.5 14.25a3 3 0 00-3 3h15.75m-12.75-3h11.218c1.121-2.3 2.1-4.684 2.924-7.138a60.114 60.114 0 00-16.536-1.84M7.5 14.25L5.106 5.272M6 20.25a.75.75 0 11-1.5 0 .75.75 0 011.5 0zm12.75 0a.75.75 0 11-1.5 0 .75.75 0 011.5 0z"
    />
  </svg>
);

export default UsersPage;
