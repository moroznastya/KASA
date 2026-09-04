import React from 'react';
import { Outlet } from 'react-router-dom';
import { BarChart3, Landmark, Network, FileClock, Store, Database } from 'lucide-react';
import { Sidebar, NavItem } from '@/components/layout/Sidebar';
import { Header } from '@/components/layout/Header';
import { useUIStore } from '@/store/uiStore';

/**
 * Оболонка ВЕБ-адмінки (Етап 6, §6): ТІЛЬКИ адмін-навігація мережі,
 * без POS/cash-пунктів. Спільні Header/Sidebar перевикористовуються.
 *
 * На відміну від AppLayout (desktop) тут НЕМАЄ редиректів онбордингу і
 * не потрібна активна точка: мережеві адмін-сторінки (/admin/*) працюють
 * поза store_middleware.
 */
const ADMIN_NAV: NavItem[] = [
    {
        path: '/network/reports',
        label: 'Дашборд мережі',
        icon: <BarChart3 className="w-5 h-5" />,
        module: 'reports',
        roles: ['admin'],
    },
    {
        path: '/network/finances',
        label: 'Фінанси мережі',
        icon: <Landmark className="w-5 h-5" />,
        module: 'reports',
        roles: ['admin'],
    },
    {
        path: '/network/devices',
        label: 'Каси мережі',
        icon: <Network className="w-5 h-5" />,
        module: 'network',
        roles: ['admin', 'owner'],
    },
    {
        path: '/network/audit',
        label: 'Аудит-лог',
        icon: <FileClock className="w-5 h-5" />,
        module: 'network',
        roles: ['admin'],
    },
    {
        path: '/settings/stores',
        label: 'Точки',
        icon: <Store className="w-5 h-5" />,
        module: 'settings',
        roles: ['admin', 'store_manager'],
    },
    {
        path: '/settings/data-source',
        label: 'Джерело даних',
        icon: <Database className="w-5 h-5" />,
        module: 'settings',
        roles: ['admin', 'store_manager'],
    },
];

export const AdminShell: React.FC = () => {
    const { sidebarOpen } = useUIStore();

    return (
        <div className="min-h-screen bg-gray-50 dark:bg-slate-900">
            <Sidebar items={ADMIN_NAV} />
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
