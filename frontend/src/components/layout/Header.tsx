import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { LogOut, Moon, Sun, User, Clock } from 'lucide-react';
import { useUIStore } from '@/store/uiStore';
import { useAuthStore } from '@/store/authStore';
import { authService } from '@/services/authService';

const moduleNames: Record<string, string> = {
  dashboard: 'Панель керування',
  pos: 'POS-каса',
  products: 'Товари',
  categories: 'Категорії',
  suppliers: 'Постачальники',
  documents: 'Документи',
  reports: 'Звіти',
  ledger: 'Взаєморозрахунки',
};

export const Header: React.FC = () => {
  const navigate = useNavigate();
  const { activeModule, theme, toggleTheme } = useUIStore();
  const logout = useAuthStore((state) => state.logout);
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

  return (
    <header className="h-16 bg-white dark:bg-slate-800 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between px-6">
      {/* Module name */}
      <div>
        <h1 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          {moduleNames[activeModule] || 'Kasa POS'}
        </h1>
      </div>

      {/* Right side */}
      <div className="flex items-center gap-4">
        {/* Clock */}
        <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
          <Clock className="w-4 h-4" />
          <span>
            {currentTime.toLocaleDateString('uk-UA', {
              day: '2-digit',
              month: 'long',
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

        {/* Logout */}
        <button
          onClick={handleLogout}
          className="flex items-center gap-2 px-3 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-danger-600 hover:bg-danger-50 dark:hover:bg-danger-900/20 rounded-lg transition-colors"
        >
          <LogOut className="w-4 h-4" />
          <span className="hidden sm:inline">Вихід</span>
        </button>
      </div>
    </header>
  );
};
