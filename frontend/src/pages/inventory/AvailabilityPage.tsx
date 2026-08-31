import React, { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Search, PackageSearch, ArrowLeft } from 'lucide-react';
import { storeService } from '@/services/storeService';
import { useAuthStore } from '@/store/authStore';
import { Input } from '@/components/ui/Input';
import { EmptyState } from '@/components/ui/EmptyState';
import { formatCurrency } from '@/utils/format';
import { useBackNavigation } from '@/hooks/useBackNavigation';

/**
 * Сторінка «Наявність в інших точках» (Етап 4 мультиточковості).
 * Read-only: пошук за назвою/ШК → GET /api/v1/inventory/availability
 * (залишки по ВСІХ точках користувача, незалежно від активної).
 * Доступ: admin/owner або пермішен inventory.view_other_stores.
 */
const AvailabilityPage: React.FC = () => {
  const user = useAuthStore((state) => state.user);
  const { goBack } = useBackNavigation();
  const [query, setQuery] = useState('');

  const canView =
    user?.role === 'admin' ||
    user?.role === 'owner' ||
    !!user?.permissions?.includes('inventory.view_other_stores');

  const { data: items = [], isLoading } = useQuery({
    queryKey: ['inventory-availability'],
    queryFn: () => storeService.availability(),
    enabled: canView,
  });

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (item) =>
        item.title.toLowerCase().includes(q) ||
        (item.barcode && item.barcode.toLowerCase().includes(q))
    );
  }, [items, query]);

  if (!canView) {
    return (
      <div className="max-w-4xl mx-auto">
        <EmptyState
          icon={<PackageSearch className="w-16 h-16" />}
          message="Немає доступу"
          description="Перегляд наявності в інших точках доступний адміністратору або за правом inventory.view_other_stores."
        />
      </div>
    );
  }

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <button
          aria-label="Назад"
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Наявність в точках
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Залишки по всіх торговельних точках (перегляд)
          </p>
        </div>
      </div>

      <div className="card p-6 space-y-6">
        <Input
          label="Пошук товару"
          placeholder="Назва або штрих-код..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          icon={<Search className="w-4 h-4" />}
        />

        {isLoading ? (
          <div className="text-center py-12 text-gray-500 dark:text-gray-400">
            Завантаження...
          </div>
        ) : filtered.length === 0 ? (
          <EmptyState
            message={query ? 'Нічого не знайдено' : 'Немає даних'}
            description={
              query
                ? 'Спробуйте інший запит за назвою або штрих-кодом'
                : 'Наявність товарів у точках з\'явиться тут'
            }
          />
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-gray-200 dark:border-slate-700 text-left text-gray-500 dark:text-gray-400">
                  <th className="px-4 py-3 font-medium">Товар</th>
                  <th className="px-4 py-3 font-medium">ШК</th>
                  <th className="px-4 py-3 font-medium">Од.</th>
                  <th className="px-4 py-3 font-medium">Точка</th>
                  <th className="px-4 py-3 font-medium text-right">Ціна</th>
                  <th className="px-4 py-3 font-medium text-right">Залишок</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((item) => (
                  <React.Fragment key={item.product_id}>
                    {item.stores.length === 0 ? (
                      <tr className="border-b border-gray-100 dark:border-slate-800">
                        <td className="px-4 py-3 font-medium text-gray-900 dark:text-gray-100">
                          {item.title}
                        </td>
                        <td className="px-4 py-3 text-gray-500 dark:text-gray-400">
                          {item.barcode || '—'}
                        </td>
                        <td className="px-4 py-3 text-gray-500 dark:text-gray-400">
                          {item.unit || '—'}
                        </td>
                        <td className="px-4 py-3 text-gray-400 italic" colSpan={3}>
                          немає в жодній точці
                        </td>
                      </tr>
                    ) : (
                      item.stores.map((s, idx) => (
                        <tr
                          key={`${item.product_id}-${s.store_id}`}
                          className="border-b border-gray-100 dark:border-slate-800"
                        >
                          {idx === 0 && (
                            <>
                              <td
                                className="px-4 py-3 font-medium text-gray-900 dark:text-gray-100 align-top"
                                rowSpan={item.stores.length}
                              >
                                {item.title}
                              </td>
                              <td
                                className="px-4 py-3 text-gray-500 dark:text-gray-400 align-top"
                                rowSpan={item.stores.length}
                              >
                                {item.barcode || '—'}
                              </td>
                              <td
                                className="px-4 py-3 text-gray-500 dark:text-gray-400 align-top"
                                rowSpan={item.stores.length}
                              >
                                {item.unit || '—'}
                              </td>
                            </>
                          )}
                          <td className="px-4 py-3 text-gray-700 dark:text-gray-300">
                            {s.store_name}
                          </td>
                          <td className="px-4 py-3 text-right text-gray-900 dark:text-gray-100 font-medium">
                            {formatCurrency(parseFloat(s.price) || 0)}
                          </td>
                          <td className="px-4 py-3 text-right text-gray-700 dark:text-gray-300">
                            {s.quantity}
                          </td>
                        </tr>
                      ))
                    )}
                  </React.Fragment>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
};

export default AvailabilityPage;
