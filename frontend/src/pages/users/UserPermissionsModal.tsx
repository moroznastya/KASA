import React, { useState, useEffect } from 'react';
import {Shield, Loader2, Check, Package, Tags, Truck, FileText, ShoppingCart, Users, BookOpen, BarChart3, UserCog, ArrowRightLeft, Trash2, Undo2, } from 'lucide-react';
import { userService } from '@/services/userService';
import {User, PermissionGroup} from '@/types/auth';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import toast from 'react-hot-toast';

const ICON_MAP: Record<string, React.ReactNode> = {
  Package: <Package className="w-5 h-5" />,
  Tags: <Tags className="w-5 h-5" />,
  Truck: <Truck className="w-5 h-5" />,
  FileText: <FileText className="w-5 h-5" />,
  ShoppingCart: <ShoppingCart className="w-5 h-5" />,
  Users: <Users className="w-5 h-5" />,
  BookOpen: <BookOpen className="w-5 h-5" />,
  BarChart3: <BarChart3 className="w-5 h-5" />,
  UserCog: <UserCog className="w-5 h-5" />,
  ArrowRightLeft: <ArrowRightLeft className="w-5 h-5" />,
  Trash2: <Trash2 className="w-5 h-5" />,
  Undo2: <Undo2 className="w-5 h-5" />,
};

interface UserPermissionsModalProps {
  user: User;
  isOpen: boolean;
  onClose: () => void;
  onSaved: () => void;
}

export const UserPermissionsModal: React.FC<UserPermissionsModalProps> = ({
  user,
  isOpen,
  onClose,
  onSaved,
}) => {
  const [groups, setGroups] = useState<PermissionGroup[]>([]);
  const [selectedPermissions, setSelectedPermissions] = useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    if (isOpen) {
      loadPermissions();
    }
  }, [isOpen, user.id]);

  const loadPermissions = async () => {
    setIsLoading(true);
    try {
      const data = await userService.getPermissionsList();
      setGroups(data.groups);

      // Встановлюємо поточні права користувача
      const userPerms = user.permissions || [];
      setSelectedPermissions(new Set(userPerms));
    } catch {
      toast.error('Помилка завантаження списку прав');
    } finally {
      setIsLoading(false);
    }
  };

  const togglePermission = (permKey: string) => {
    setSelectedPermissions((prev) => {
      const next = new Set(prev);
      if (next.has(permKey)) {
        next.delete(permKey);
      } else {
        next.add(permKey);
      }
      return next;
    });
  };

  const toggleGroup = (group: PermissionGroup) => {
    const groupKeys = group.permissions.map((p) => p.key);
    const allSelected = groupKeys.every((k) => selectedPermissions.has(k));

    setSelectedPermissions((prev) => {
      const next = new Set(prev);
      if (allSelected) {
        // Вимикаємо всі в групі
        groupKeys.forEach((k) => next.delete(k));
      } else {
        // Вмикаємо всі в групі
        groupKeys.forEach((k) => next.add(k));
      }
      return next;
    });
  };

  const selectAll = () => {
    const allKeys = groups.flatMap((g) => g.permissions.map((p) => p.key));
    setSelectedPermissions(new Set(allKeys));
  };

  const clearAll = () => {
    setSelectedPermissions(new Set());
  };

  const handleSave = async () => {
    setIsSaving(true);
    try {
      const permissionsArray = Array.from(selectedPermissions);
      await userService.updatePermissions(user.id, permissionsArray);
      toast.success(`Права доступу для "${user.name}" оновлено`);
      onSaved();
      onClose();
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
        toast.error('Помилка збереження прав');
      }
    } finally {
      setIsSaving(false);
    }
  };

  const getGroupState = (group: PermissionGroup): 'all' | 'some' | 'none' => {
    const groupKeys = group.permissions.map((p) => p.key);
    const selectedCount = groupKeys.filter((k) => selectedPermissions.has(k)).length;

    if (selectedCount === groupKeys.length) return 'all';
    if (selectedCount > 0) return 'some';
    return 'none';
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={`Права доступу: ${user.name}`}
      size="lg"
    >
      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <Loader2 className="w-8 h-8 animate-spin text-primary-600" />
        </div>
      ) : (
        <div className="space-y-4">
          {/* Інформація про користувача */}
          <div className="flex items-center gap-3 p-3 bg-gray-50 dark:bg-slate-700/50 rounded-lg">
            <div className="w-10 h-10 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center">
              <span className="text-lg font-medium text-primary-700 dark:text-primary-400">
                {user.name.charAt(0).toUpperCase()}
              </span>
            </div>
            <div>
              <p className="font-medium text-gray-900 dark:text-gray-100">{user.name}</p>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                {user.login} · {user.role === 'admin' ? 'Адміністратор' : 'Касир'}
              </p>
            </div>
          </div>

          {/* Кнопки "Виділити всі" / "Очистити" */}
          <div className="flex items-center justify-between">
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Обрано {selectedPermissions.size} з{' '}
              {groups.reduce((acc, g) => acc + g.permissions.length, 0)} прав
            </p>
            <div className="flex gap-2">
              <Button variant="secondary" size="sm" onClick={selectAll}>
                Виділити всі
              </Button>
              <Button variant="secondary" size="sm" onClick={clearAll}>
                Очистити
              </Button>
            </div>
          </div>

          {/* Список груп прав */}
          <div className="space-y-3 max-h-[60vh] overflow-y-auto pr-1">
            {groups.map((group) => {
              const groupState = getGroupState(group);
              return (
                <div
                  key={group.name}
                  className="border border-gray-200 dark:border-slate-600 rounded-lg overflow-hidden"
                >
                  {/* Заголовок групи */}
                  <button
                    onClick={() => toggleGroup(group)}
                    className={`
                      w-full flex items-center gap-3 px-4 py-3 text-left
                      transition-colors duration-150
                      ${groupState === 'all'
                        ? 'bg-primary-50 dark:bg-primary-900/20'
                        : groupState === 'some'
                        ? 'bg-blue-50 dark:bg-blue-900/10'
                        : 'bg-gray-50 dark:bg-slate-700/30 hover:bg-gray-100 dark:hover:bg-slate-700/50'
                      }
                    `}
                  >
                    <div className={`
                      w-5 h-5 rounded border-2 flex items-center justify-center flex-shrink-0
                      transition-colors duration-150
                      ${groupState === 'all'
                        ? 'bg-primary-600 border-primary-600'
                        : groupState === 'some'
                        ? 'bg-blue-500 border-blue-500'
                        : 'border-gray-300 dark:border-gray-500'
                      }
                    `}>
                      {groupState === 'all' && <Check className="w-3.5 h-3.5 text-white" />}
                      {groupState === 'some' && <div className="w-2 h-2 bg-white rounded-sm" />}
                    </div>
                    <div className="text-gray-500 dark:text-gray-400">
                      {ICON_MAP[group.icon] || <Shield className="w-5 h-5" />}
                    </div>
                    <span className="font-medium text-gray-900 dark:text-gray-100">
                      {group.name}
                    </span>
                    <span className="ml-auto text-xs text-gray-400 dark:text-gray-500">
                      {group.permissions.filter((p) => selectedPermissions.has(p.key)).length}/{group.permissions.length}
                    </span>
                  </button>

                  {/* Список прав в групі */}
                  <div className="divide-y divide-gray-100 dark:divide-slate-700">
                    {group.permissions.map((perm) => (
                      <label
                        key={perm.key}
                        className={`
                          flex items-center gap-3 px-4 py-2.5 cursor-pointer
                          transition-colors duration-150
                          hover:bg-gray-50 dark:hover:bg-slate-700/30
                          ${selectedPermissions.has(perm.key)
                            ? 'bg-primary-50/50 dark:bg-primary-900/10'
                            : ''
                          }
                        `}
                      >
                        <input
                          type="checkbox"
                          checked={selectedPermissions.has(perm.key)}
                          onChange={() => togglePermission(perm.key)}
                          className="w-4 h-4 text-primary-600 bg-gray-100 border-gray-300 rounded 
                                   focus:ring-primary-500 dark:focus:ring-primary-600 
                                   dark:ring-offset-gray-800 focus:ring-2 
                                   dark:bg-gray-700 dark:border-gray-600"
                        />
                        <div className="flex-1 min-w-0">
                          <p className="text-sm font-medium text-gray-700 dark:text-gray-300">
                            {perm.label}
                          </p>
                          {perm.description && (
                            <p className="text-xs text-gray-400 dark:text-gray-500 truncate">
                              {perm.description}
                            </p>
                          )}
                        </div>
                        <code className="text-xs text-gray-400 dark:text-gray-500 font-mono hidden sm:block">
                          {perm.key}
                        </code>
                      </label>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Кнопки збереження */}
          <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
            <Button variant="secondary" onClick={onClose}>
              Скасувати
            </Button>
            <Button onClick={handleSave} isLoading={isSaving}>
              <Shield className="w-4 h-4 mr-2" />
              Зберегти права
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
};
