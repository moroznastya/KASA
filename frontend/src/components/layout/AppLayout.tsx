import React, { useEffect } from 'react';
import { Navigate, Outlet } from 'react-router-dom';
import { Sidebar } from './Sidebar';
import { Header } from './Header';
import { useUIStore } from '@/store/uiStore';
import { useStoreStore } from '@/store/storeStore';
import { useAuthStore } from '@/store/authStore';
import { Spinner } from '@/components/ui/Spinner';
import { Building2 } from 'lucide-react';

export const AppLayout: React.FC = () => {
  const { sidebarOpen } = useUIStore();
  const user = useAuthStore((state) => state.user);
  const { stores, storesLoaded, activeStoreId, loadStores } = useStoreStore();

  // Первинне завантаження точок (покриває і вхід, і перезавантаження сторінки).
  useEffect(() => {
    if (!storesLoaded) {
      void loadStores();
    }
  }, [storesLoaded, loadStores]);

  // Поки точки не завантажені — не рендеримо сторінки: без activeStoreId
  // бізнес-запити впадуть з 400 (X-Store-Id обов'язковий).
  if (!storesLoaded) {
    return (
      <div className="flex items-center justify-center min-h-screen bg-gray-50 dark:bg-slate-900">
        <div className="text-center">
          <Spinner size="lg" />
          <p className="mt-4 text-sm text-gray-500">Завантаження точок...</p>
        </div>
      </div>
    );
  }

  // Онбординг: поки onboarding_completed !== true — показуємо налаштування (owner/admin).
  if (user && user.role !== 'cashier' && user.onboarding_completed !== true) {
    return <Navigate to="/onboarding" replace />;
  }

  // Касир/менеджер без жодної точки — НЕ редиректимо на онбординг
  // (глухий кут: онбординг створює точки/касирів і доступний лише owner/admin).
  // Показуємо заглушку замість розсипу «Помилка завантаження» з кожної
  // сторінки (без X-Store-Id бізнес-запити падають 400/403).
  if (user && user.role !== 'admin' && user.role !== 'owner' && stores.length === 0) {
    return (
      <div className="min-h-screen bg-gray-50 dark:bg-slate-900 flex items-center justify-center p-4">
        <div className="w-full max-w-md card p-8 text-center">
          <div className="w-12 h-12 rounded-full bg-amber-100 dark:bg-amber-900/40 flex items-center justify-center mx-auto mb-4">
            <Building2 className="w-6 h-6 text-amber-600 dark:text-amber-400" />
          </div>
          <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100 mb-2">
            Немає доступу до точок
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-6">
            Вам не призначено жодної торговельної точки. Зверніться до адміністратора.
          </p>
        </div>
      </div>
    );
  }

  // Власник/адмін без жодної точки і без збереженої активної — онбординг.
  // (Тепер онбординг працює: перший власник створюється через /setup,
  //  далі точки/касири створюються в онбордингу.)
  if (stores.length === 0 && !activeStoreId) {
    return <Navigate to="/onboarding" replace />;
  }

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-slate-900">
      <Sidebar />
      <div
        className={`
          transition-all duration-300 ease-in-out
          ${sidebarOpen ? 'ml-64' : 'ml-16'}
        `}
      >
        <Header />
        <main className="p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
};
