import React, { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import {
  ArrowLeft,
  Plus,
  MapPin,
  Phone,
  UserPlus,
  Store as StoreIcon,
  Star,
} from 'lucide-react';
import { storeService } from '@/services/storeService';
import { userService } from '@/services/userService';
import { useStoreStore } from '@/store/storeStore';
import type { Store } from '@/types/store';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { Modal } from '@/components/ui/Modal';
import { Spinner } from '@/components/ui/Spinner';
import { Badge } from '@/components/ui/Badge';
import { toast } from 'react-hot-toast';

// ── Конфігурація ролей ────────────────────────
const ROLE_LABELS: Record<string, string> = {
  owner: 'Власник',
  admin: 'Адміністратор',
  manager: 'Менеджер',
  cashier: 'Касир',
};

const ROLE_COLORS: Record<string, string> = {
  owner: 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-400',
  admin: 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400',
  manager: 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400',
  cashier: 'bg-gray-100 dark:bg-slate-700 text-gray-700 dark:text-gray-300',
};

const ROLE_OPTIONS: SelectOption[] = [
  { value: 'owner', label: 'Власник' },
  { value: 'admin', label: 'Адміністратор' },
  { value: 'manager', label: 'Менеджер' },
  { value: 'cashier', label: 'Касир' },
];

/** Чи може користувач призначати інших на цю точку */
const canAssign = (store: Store): boolean =>
  store.role === 'owner' || store.role === 'admin';

// ═══════════════════════════════════════════════════════════════
// СТОРІНКА: Торгові точки
// ═══════════════════════════════════════════════════════════════
const StoresPage: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeStoreId = useStoreStore((state) => state.activeStoreId);
  const loadStores = useStoreStore((state) => state.loadStores);

  // ── Список точок ─────────────────────────────
  const { data: stores, isLoading, isError } = useQuery<Store[]>({
    queryKey: ['stores'],
    queryFn: () => storeService.list(),
  });

  // ── Стан: модалка створення ──────────────────
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [createForm, setCreateForm] = useState({ name: '', address: '', phone: '' });

  // ── Стан: модалка призначення ────────────────
  const [assignStore, setAssignStore] = useState<Store | null>(null);
  const [assignForm, setAssignForm] = useState({ user_id: '', role: 'cashier' });

  // ── Список користувачів (для призначення) ────
  const { data: users } = useQuery({
    queryKey: ['users'],
    queryFn: () => userService.list(),
    enabled: !!assignStore,
  });

  // ── Мутація: створення точки ─────────────────
  const createMutation = useMutation({
    mutationFn: (data: { name: string; address?: string; phone?: string }) =>
      storeService.create(data),
    onSuccess: () => {
      toast.success('Точку створено');
      setIsCreateOpen(false);
      setCreateForm({ name: '', address: '', phone: '' });
      // Оновлюємо перемикач у хедері та список на сторінці
      loadStores();
      queryClient.invalidateQueries({ queryKey: ['stores'] });
    },
    onError: (err: any) => {
      toast.error(err?.response?.data?.detail || 'Помилка створення точки');
    },
  });

  // ── Мутація: призначення користувача ─────────
  const assignMutation = useMutation({
    mutationFn: (data: { user_id: string; store_id: string; role?: string }) =>
      storeService.assignUser(data),
    onSuccess: () => {
      toast.success('Користувача призначено');
      setAssignStore(null);
      setAssignForm({ user_id: '', role: 'cashier' });
      queryClient.invalidateQueries({ queryKey: ['stores'] });
    },
    onError: (err: any) => {
      // Сервер поверне помилку, якщо користувач вже призначений на точку
      toast.error(err?.response?.data?.detail || 'Помилка призначення користувача');
    },
  });

  // ── Обробники ─────────────────────────────────
  const handleCreate = () => {
    if (!createForm.name.trim()) {
      toast.error('Введіть назву точки');
      return;
    }
    createMutation.mutate({
      name: createForm.name.trim(),
      address: createForm.address.trim() || undefined,
      phone: createForm.phone.trim() || undefined,
    });
  };

  const handleAssign = () => {
    if (!assignStore) return;
    if (!assignForm.user_id) {
      toast.error('Оберіть користувача');
      return;
    }
    assignMutation.mutate({
      user_id: assignForm.user_id,
      store_id: assignStore.id,
      role: assignForm.role,
    });
  };

  const openAssign = (store: Store) => {
    setAssignStore(store);
    setAssignForm({ user_id: '', role: 'cashier' });
  };

  const userOptions: SelectOption[] = (users || []).map((u) => ({
    value: u.id,
    label: `${u.name} (${u.login})`,
  }));

  // ── Стани завантаження ───────────────────────
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
          <p className="text-red-500 font-medium">Помилка завантаження точок</p>
          <p className="text-sm text-gray-500 mt-1">Спробуйте пізніше</p>
        </div>
      </div>
    );
  }

  const storeList = stores || [];

  return (
    <div className="max-w-5xl mx-auto px-4 py-6 space-y-6">
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
              Торгові точки
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Керування торговими точками та доступом користувачів
            </p>
          </div>
        </div>
        <Button onClick={() => setIsCreateOpen(true)} icon={<Plus className="w-4 h-4" />}>
          Додати точку
        </Button>
      </div>

      {/* ── Список точок ───────────────────── */}
      {storeList.length === 0 ? (
        <div className="text-center py-16">
          <div className="w-20 h-20 mx-auto mb-4 rounded-full bg-gray-100 dark:bg-slate-700 flex items-center justify-center">
            <StoreIcon className="w-10 h-10 text-gray-400" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            Ще немає точок
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-6 max-w-md mx-auto">
            Створіть першу торгову точку, щоб почати роботу
          </p>
          <Button onClick={() => setIsCreateOpen(true)} icon={<Plus className="w-4 h-4" />}>
            Додати точку
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {storeList.map((store) => (
            <div
              key={store.id}
              className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden hover:shadow-md transition-shadow"
            >
              <div className="p-5">
                <div className="flex items-start justify-between gap-2 mb-3">
                  <div className="flex items-center gap-3 min-w-0">
                    <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400 flex-shrink-0">
                      <StoreIcon className="w-5 h-5" />
                    </div>
                    <div className="min-w-0">
                      <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
                        {store.name}
                      </h3>
                      <span className={`inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium mt-1 ${ROLE_COLORS[store.role] || ROLE_COLORS.cashier}`}>
                        {ROLE_LABELS[store.role] || store.role}
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 flex-shrink-0">
                    {store.id === activeStoreId && (
                      <Badge variant="success" size="sm">
                        Активна
                      </Badge>
                    )}
                    {store.is_default && (
                      <Badge variant="primary" size="sm">
                        <Star className="w-3 h-3 mr-1" />
                        За замовчуванням
                      </Badge>
                    )}
                  </div>
                </div>

                <div className="space-y-1 text-sm text-gray-500 dark:text-gray-400">
                  {store.address && (
                    <p className="flex items-center gap-1.5">
                      <MapPin className="w-3.5 h-3.5 flex-shrink-0" />
                      {store.address}
                    </p>
                  )}
                  {store.phone && (
                    <p className="flex items-center gap-1.5">
                      <Phone className="w-3.5 h-3.5 flex-shrink-0" />
                      {store.phone}
                    </p>
                  )}
                  {!store.address && !store.phone && (
                    <p className="text-xs text-gray-400 dark:text-gray-500">
                      Без адреси та телефону
                    </p>
                  )}
                </div>
              </div>

              {canAssign(store) && (
                <div className="px-5 py-3 bg-gray-50 dark:bg-slate-700/50 border-t border-gray-100 dark:border-slate-700 flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => openAssign(store)}
                    icon={<UserPlus className="w-3.5 h-3.5" />}
                    className="text-gray-600 dark:text-gray-400"
                  >
                    Призначити користувача
                  </Button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {/* ── Модалка: створення точки ───────── */}
      <Modal
        isOpen={isCreateOpen}
        onClose={() => setIsCreateOpen(false)}
        title="Додати точку"
        size="md"
      >
        <div className="space-y-4">
          <Input
            label="Назва"
            value={createForm.name}
            onChange={(e) => setCreateForm((prev) => ({ ...prev, name: e.target.value }))}
            placeholder='Наприклад: "Магазин на Хрещатику"'
            required
          />
          <Input
            label="Адреса"
            value={createForm.address}
            onChange={(e) => setCreateForm((prev) => ({ ...prev, address: e.target.value }))}
            placeholder="м. Київ, вул. Хрещатик, 1"
          />
          <Input
            label="Телефон"
            value={createForm.phone}
            onChange={(e) => setCreateForm((prev) => ({ ...prev, phone: e.target.value }))}
            placeholder="+380 00 000 00 00"
          />
        </div>
        <div className="flex items-center justify-end gap-3 pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => setIsCreateOpen(false)}>
            Скасувати
          </Button>
          <Button
            onClick={handleCreate}
            disabled={createMutation.isPending}
            icon={createMutation.isPending ? undefined : <Plus className="w-4 h-4" />}
          >
            {createMutation.isPending ? 'Створення...' : 'Створити'}
          </Button>
        </div>
      </Modal>

      {/* ── Модалка: призначення користувача ── */}
      <Modal
        isOpen={!!assignStore}
        onClose={() => setAssignStore(null)}
        title={`Призначити користувача — ${assignStore?.name || ''}`}
        size="md"
      >
        <div className="space-y-4">
          <Select
            label="Користувач"
            options={userOptions}
            value={assignForm.user_id}
            onChange={(e) => setAssignForm((prev) => ({ ...prev, user_id: e.target.value }))}
            placeholder="Оберіть користувача"
          />
          <Select
            label="Роль на точці"
            options={ROLE_OPTIONS}
            value={assignForm.role}
            onChange={(e) => setAssignForm((prev) => ({ ...prev, role: e.target.value }))}
          />
        </div>
        <div className="flex items-center justify-end gap-3 pt-4 mt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={() => setAssignStore(null)}>
            Скасувати
          </Button>
          <Button
            onClick={handleAssign}
            disabled={assignMutation.isPending}
            icon={assignMutation.isPending ? undefined : <UserPlus className="w-4 h-4" />}
          >
            {assignMutation.isPending ? 'Призначення...' : 'Призначити'}
          </Button>
        </div>
      </Modal>
    </div>
  );
};

export default StoresPage;
