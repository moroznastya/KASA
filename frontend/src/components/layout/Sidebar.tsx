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
  Users,
  ChevronLeft,
  ChevronRight,
  UserCog,
  Settings,
  Clock,
  Printer,
} from 'lucide-react';
import { useUIStore } from '@/store/uiStore';
import { useAuthStore } from '@/store/authStore';

interface NavItem {
  path: string;
  label: string;
  icon: React.ReactNode;
  module: string;
  roles?: ('admin' | 'cashier')[];
}

const navItems: NavItem[] = [
  {
    path: '/',
    label: 'Панель керування',
    icon: <LayoutDashboard className="w-5 h-5" />,
    module: 'dashboard',
    roles: ['admin'],
  },
  {
    path: '/pos',
    label: 'POS-каса',
    icon: <ShoppingCart className="w-5 h-5" />,
    module: 'pos',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/debtors',
    label: 'Боржники',
    icon: <Users className="w-5 h-5" />,
    module: 'debtors',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/receipts',
    label: 'Чеки',
    icon: <Receipt className="w-5 h-5" />,
    module: 'receipts',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/products',
    label: 'Товари',
    icon: <Package className="w-5 h-5" />,
    module: 'products',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/printing',
    label: 'Друк цінників та етикеток',
    icon: <Printer className="w-5 h-5" />,
    module: 'products',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/categories',
    label: 'Категорії',
    icon: <Tags className="w-5 h-5" />,
    module: 'categories',
    roles: ['admin'],
  },
  {
    path: '/suppliers',
    label: 'Постачальники',
    icon: <Truck className="w-5 h-5" />,
    module: 'suppliers',
    roles: ['admin'],
  },
  {
    path: '/documents',
    label: 'Накладні',
    icon: <FileText className="w-5 h-5" />,
    module: 'documents',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/reports',
    label: 'Звіти',
    icon: <BarChart3 className="w-5 h-5" />,
    module: 'reports',
    roles: ['admin'],
  },
  {
    path: '/work-time',
    label: 'Робочий час',
    icon: <Clock className="w-5 h-5" />,
    module: 'work-time',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/ledger',
    label: 'Взаєморозрахунки',
    icon: <BookOpen className="w-5 h-5" />,
    module: 'ledger',
    roles: ['admin', 'cashier'],
  },
  {
    path: '/users',
    label: 'Користувачі',
    icon: <UserCog className="w-5 h-5" />,
    module: 'users',
    roles: ['admin'],
  },
  {
    path: '/settings',
    label: 'Налаштування',
    icon: <Settings className="w-5 h-5" />,
    module: 'settings',
    roles: ['admin'],
  },
];

export const Sidebar: React.FC = () => {
  const { sidebarOpen, toggleSidebar, setActiveModule } = useUIStore();
  const user = useAuthStore((state) => state.user);

  const visibleItems = navItems.filter(
    (item) => !item.roles || item.roles.includes(user?.role as 'admin' | 'cashier')
  );

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
        {visibleItems.map((item) => (
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

      {/* User info & Toggle */}
      <div className="border-t border-gray-200 dark:border-slate-700">
        {user && sidebarOpen && (
          <div className="px-4 py-3 border-b border-gray-200 dark:border-slate-700">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center flex-shrink-0">
                <span className="text-sm font-medium text-primary-700 dark:text-primary-400">
                  {user.name.charAt(0).toUpperCase()}
                </span>
              </div>
              <div className="min-w-0">
                <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                  {user.name} ({user.role === 'admin' ? 'Адміністратор' : 'Касир'})
                </p>
              </div>
            </div>
          </div>
        )}
        <div className="p-2">
          <button
            onClick={toggleSidebar}
            className={`
              w-full flex items-center rounded-lg transition-all duration-200
              text-gray-600 dark:text-gray-400
              hover:bg-gray-100 dark:hover:bg-slate-700
              ${sidebarOpen
                ? 'gap-3 px-3 py-2.5 justify-start'
                : 'justify-center p-2'
              }
            `}
            title={sidebarOpen ? 'Згорнути' : 'Розгорнути'}
          >
            {sidebarOpen ? (
              <>
                <ChevronLeft className="w-4 h-4" />
                <span className="text-sm font-medium">Згорнути</span>
              </>
            ) : (
              <ChevronRight className="w-4 h-4" />
            )}
          </button>
        </div>
      </div>
    </aside>
  );
};
