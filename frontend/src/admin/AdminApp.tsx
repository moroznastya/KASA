import React, { Suspense, lazy } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { ProtectedRoute } from '@/components/layout/ProtectedRoute';
import { RoleRoute } from '@/components/layout/RoleRoute';
import { Spinner } from '@/components/ui/Spinner';
import { AdminShell } from './AdminShell';

/**
 * ВЕБ-адмінка (Етап 6, ТЗ §6): окрема SPA для браузера з тим самим API.
 * ТІЛЬКИ адмін-роути мережі — без POS/cash. Спільні компоненти/сервіси
 * перевикористовуються (auth, api, сторінки, layout).
 *
 * Збірка: `npm run build:admin` (vite.admin.config.ts) → dist-admin/.
 * Основна Tauri-збірка (vite.config.ts) не зачіпається.
 */
const LoginPage = lazy(() => import('@/pages/auth/LoginPage'));
const SetupPage = lazy(() => import('@/pages/setup/SetupPage'));
const NetworkReportsPage = lazy(() => import('@/pages/network/NetworkReportsPage'));
const NetworkFinancesPage = lazy(() => import('@/pages/network/NetworkFinancesPage'));
const NetworkDevicesPage = lazy(() => import('@/pages/network/DevicesPage'));
const AuditLogPage = lazy(() => import('@/pages/network/AuditLogPage'));
const StoresPage = lazy(() => import('@/pages/settings/StoresPage'));
const StoreDetailPage = lazy(() => import('@/pages/settings/StoreDetailPage'));
const DataSourcePage = lazy(() => import('@/pages/settings/DataSourcePage'));

const PageLoader: React.FC = () => (
    <div className="flex items-center justify-center min-h-[60vh]">
        <div className="text-center">
            <Spinner size="lg" />
            <p className="mt-4 text-sm text-gray-500">Завантаження...</p>
        </div>
    </div>
);

export const AdminApp: React.FC = () => (
    <BrowserRouter>
        <Suspense fallback={<PageLoader />}>
            <Routes>
                {/* Публічні: логін + майстер першого встановлення (fresh-БД). */}
                <Route path="/login" element={<LoginPage />} />
                <Route path="/setup" element={<SetupPage />} />

                <Route
                    path="/"
                    element={
                        <ProtectedRoute>
                            <AdminShell />
                        </ProtectedRoute>
                    }
                >
                    <Route index element={<Navigate to="/network/reports" replace />} />
                    <Route
                        path="network/reports"
                        element={
                            <RoleRoute roles={['admin', 'owner']}>
                                <NetworkReportsPage />
                            </RoleRoute>
                        }
                    />
                    <Route
                        path="network/finances"
                        element={
                            <RoleRoute roles={['admin', 'owner']}>
                                <NetworkFinancesPage />
                            </RoleRoute>
                        }
                    />
                    <Route
                        path="network/devices"
                        element={
                            <RoleRoute roles={['admin', 'owner']}>
                                <NetworkDevicesPage />
                            </RoleRoute>
                        }
                    />
                    <Route
                        path="network/audit"
                        element={
                            <RoleRoute roles={['admin', 'owner']}>
                                <AuditLogPage />
                            </RoleRoute>
                        }
                    />
                    <Route
                        path="settings/stores"
                        element={
                            <RoleRoute roles={['admin', 'store_manager']}>
                                <StoresPage />
                            </RoleRoute>
                        }
                    />
                    <Route
                        path="settings/stores/:storeId"
                        element={
                            <RoleRoute roles={['admin', 'store_manager']}>
                                <StoreDetailPage />
                            </RoleRoute>
                        }
                    />
                    <Route
                        path="settings/data-source"
                        element={
                            <RoleRoute roles={['admin', 'store_manager']}>
                                <DataSourcePage />
                            </RoleRoute>
                        }
                    />
                    <Route path="*" element={<Navigate to="/network/reports" replace />} />
                </Route>

                <Route path="*" element={<Navigate to="/network/reports" replace />} />
            </Routes>
        </Suspense>
    </BrowserRouter>
);
