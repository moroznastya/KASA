import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { LogOut, Moon, Sun, User, Clock, Store } from 'lucide-react';
import { useUIStore } from '@/store/uiStore';
import { SyncStatus } from '@/hooks/useOfflineSync';
import { useAuthStore } from '@/store/authStore';
import { useStoreStore } from '@/store/storeStore';
import { authService } from '@/services/authService';
import { Select } from '@/components/ui/Select';
import { useQueryClient } from '@tanstack/react-query';

const moduleNames: Record<string, string> = {
  dashboard: 'Панель керування',
  pos: 'POS-каса',
  cash: 'Каса',
  products: 'Товари',
  categories: 'Категорії',
  suppliers: 'Постачальники',
  documents: 'Документи',
  reports: 'Звіти',
  ledger: 'Взаєморозрахунки',
  'work-time': 'Робочий час',
};

export const Header: React.FC = () => {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { activeModule, theme, toggleTheme } = useUIStore();
  const user = useAuthStore((state) => state.user);
  const logout = useAuthStore((state) => state.logout);
  const stores = useStoreStore((state) => state.stores);
  const activeStoreId = useStoreStore((state) => state.activeStoreId);
  const setActiveStore = useStoreStore((state) => state.setActiveStore);
  const [currentTime, setCurrentTime] = useState(new Date());

  useEffect(() => {
    const timer = setInterval(() => setCurrentTime(new Date()), 1000);
    return () => clearInterval(timer);
  }, []);

  const handleLogout = async () => {
    try {
      await authService.logout();
    } catch {
      // Ignore
    }
    logout();
    navigate('/login');
  };

  // Switcher точок: лише для власників/адміністраторів (>1 точки).
  // Касир — без switcher (його точка визначається призначенням).
  const showStoreSwitcher =
    !!user && user.role !== 'cashier' && stores.length > 1;

  const handleStoreChange = (storeId: string) => {
    if (!storeId || storeId === activeStoreId) return;
    setActiveStore(storeId);
    // Перезавантаження даних сторінки: інвалідація всіх активних запитів.
    void queryClient.invalidateQueries();
  };

  return (
    <header className="h-16 bg-white dark:bg-slate-800 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between px-6">
      {/* Module name */}
      <div>
        <h1 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          {moduleNames[activeModule] || 'Torgashka'}
        </h1>
      </div>

      {/* Right side */}
      <div className="flex items-center gap-4">
        {showStoreSwitcher && (
          <Select
            title="Активна торгівельна точка"
            value={activeStoreId ?? ''}
            onChange={(e) => handleStoreChange(e.target.value)}
            options={stores.map((s) => ({ value: s.id, label: s.name }))}
            leftIcon={<Store className="w-4 h-4 text-primary-500 dark:text-primary-400" />}
            containerClassName="w-[200px]"
          />
        )}

        {/* Індикатор офлайн-синхронізації (Tauri SQLite черга; null у браузері) */}
        <SyncStatus />

        {/* Clock */}
        <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 whitespace-nowrap shrink-0">
          <Clock className="w-4 h-4" />
          <span>
            {currentTime.toLocaleDateString('uk-UA', {
              day: '2-digit',
              month: '2-digit',
              year: 'numeric',
            })}{' '}
            {currentTime.toLocaleTimeString('uk-UA', {
              hour: '2-digit',
              minute: '2-digit',
            })}
          </span>
        </div>

        {/* Theme toggle */}
        <button
          onClick={toggleTheme}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          title={theme === 'light' ? 'Темна тема' : 'Світла тема'}
        >
          {theme === 'light' ? <Moon className="w-4 h-4" /> : <Sun className="w-4 h-4" />}
        </button>

        {/* User info + Logout */}
        {user && (
          <button
            onClick={handleLogout}
            className="flex items-center gap-2 pl-3 border-l border-gray-200 dark:border-slate-700 text-gray-600 dark:text-gray-400 hover:bg-danger-50 dark:hover:bg-danger-900/20 rounded-lg transition-colors py-1.5"
            title="Вийти"
          >
            <div className="w-6 h-6 bg-primary-100 dark:bg-primary-900/30 rounded-full flex items-center justify-center">
              <User className="w-3 h-3 text-primary-600 dark:text-primary-400" />
            </div>
            <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
              {user.name}
            </span>
            <LogOut className="w-5 h-5 text-red-500" />
          </button>
        )}
      </div>
    </header>
  );
};
