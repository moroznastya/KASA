import React from 'react';
import { NavLink } from 'react-router-dom';
import {
  LayoutDashboard,
  ShoppingCart,
  Package,
  Tags,
  Truck,
  FileText,
  BarChart3,
  BookOpen,
  Receipt,
  ChevronLeft,
  ChevronRight,
  LogOut,
  Settings,
} from 'lucide-react';
import { useUIStore } from '@/store/uiStore';
import { useAuthStore } from '@/store/authStore';

interface NavItem {
  path: string;
  label: string;
  icon: React.ReactNode;
  module: string;
}

const navItems: NavItem[] = [
  {
    path: '/',
    label: 'Панель керування',
    icon: <LayoutDashboard className="w-5 h-5" />,
    module: 'dashboard',
  },
  {
    path: '/pos',
    label: 'POS-каса',
    icon: <ShoppingCart className="w-5 h-5" />,
    module: 'pos',
  },
  {
    path: '/receipts',
    label: 'Чеки',
    icon: <Receipt className="w-5 h-5" />,
    module: 'receipts',
  },
  {
    path: '/products',
    label: 'Товари',
    icon: <Package className="w-5 h-5" />,
    module: 'products',
  },
  {
    path: '/categories',
    label: 'Категорії',
    icon: <Tags className="w-5 h-5" />,
    module: 'categories',
  },
  {
    path: '/suppliers',
    label: 'Постачальники',
    icon: <Truck className="w-5 h-5" />,
    module: 'suppliers',
  },
  {
    path: '/documents',
    label: 'Документи',
    icon: <FileText className="w-5 h-5" />,
    module: 'documents',
  },
  {
    path: '/reports',
    label: 'Звіти',
    icon: <BarChart3 className="w-5 h-5" />,
    module: 'reports',
  },
  {
    path: '/ledger',
    label: 'Взаєморозрахунки',
    icon: <BookOpen className="w-5 h-5" />,
    module: 'ledger',
  },
];

export const Sidebar: React.FC = () => {
  const { sidebarOpen, toggleSidebar, setActiveModule } = useUIStore();
  const user = useAuthStore((state) => state.user);

  return (
    <aside
      className={`
        fixed left-0 top-0 h-full z-40
        bg-white dark:bg-slate-800
        border-r border-gray-200 dark:border-slate-700
        transition-all duration-300 ease-in-out
        flex flex-col
        ${sidebarOpen ? 'w-64' : 'w-16'}
      `}
    >
      {/* Logo */}
      <div className="flex items-center h-16 px-4 border-b border-gray-200 dark:border-slate-700">
        <div className="flex items-center gap-3 min-w-0">
          <div className="w-8 h-8 bg-primary-600 rounded-lg flex items-center justify-center flex-shrink-0">
            <span className="text-white font-bold text-sm">K</span>
          </div>
          {sidebarOpen && (
            <span className="font-bold text-lg text-gray-900 dark:text-gray-100 whitespace-nowrap">
              Kasa POS
            </span>
          )}
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 py-4 px-2 space-y-1 overflow-y-auto">
        {navItems.map((item) => (
          <NavLink
            key={item.path}
            to={item.path}
            onClick={() => setActiveModule(item.module as any)}
            className={({ isActive }) =>
              `
              flex items-center gap-3 px-3 py-2.5 rounded-lg transition-all duration-150
              ${
                isActive
                  ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-400'
                  : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-slate-700/50'
              }
              ${!sidebarOpen ? 'justify-center px-2' : ''}
            `
            }
            title={!sidebarOpen ? item.label : undefined}
          >
            {item.icon}
            {sidebarOpen && (
              <span className="text-sm font-medium">{item.label}</span>
            )}
          </NavLink>
        ))}
      </nav>

      {/* User info & toggle */}
      <div className="border-t border-gray-200 dark:border-slate-700 p-2">
        {sidebarOpen && user && (
          <div className="px-3 py-2 mb-2">
            <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
              {user.name}
            </p>
            <p className="text-xs text-gray-500 dark:text-gray-400 capitalize">
              {user.role === 'admin'
                ? 'Адміністратор'
                : user.role === 'cashier'
                ? 'Касир'
                : user.role === 'manager'
                ? 'Менеджер'
                : 'Власник'}
            </p>
          </div>
        )}
        <button
          onClick={toggleSidebar}
          className="w-full flex items-center justify-center p-2 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          {sidebarOpen ? <ChevronLeft className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
        </button>
      </div>
    </aside>
  );
};
