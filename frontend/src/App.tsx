import React, { Suspense, lazy, useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';
import { useUIStore } from '@/store/uiStore';
import { AppLayout } from '@/components/layout/AppLayout';
import { ProtectedRoute } from '@/components/layout/ProtectedRoute';
import { RoleRoute } from '@/components/layout/RoleRoute';
import { Spinner } from '@/components/ui/Spinner';

// Lazy-loaded pages
const LoginPage = lazy(() => import('@/pages/auth/LoginPage'));
const DashboardPage = lazy(() => import('@/pages/dashboard/DashboardPage'));
const ProductListPage = lazy(() => import('@/pages/products/ProductListPage'));
const ProductFormPage = lazy(() => import('@/pages/products/ProductFormPage'));
const CategoryListPage = lazy(() => import('@/pages/categories/CategoryListPage'));
const SupplierListPage = lazy(() => import('@/pages/suppliers/SupplierListPage'));
const SupplierFormPage = lazy(() => import('@/pages/suppliers/SupplierFormPage'));
const SupplierProductsPage = lazy(() => import('@/pages/suppliers/SupplierProductsPage'));
const DocumentListPage = lazy(() => import('@/pages/documents/DocumentListPage'));
const InvoiceFormPage = lazy(() => import('@/pages/documents/InvoiceFormPage'));
const PurchaseOrderFormPage = lazy(() => import('@/pages/documents/PurchaseOrderFormPage'));
const TransferFormPage = lazy(() => import('@/pages/documents/TransferFormPage'));
const WriteOffFormPage = lazy(() => import('@/pages/documents/WriteOffFormPage'));
const ReturnInvoiceFormPage = lazy(() => import('@/pages/documents/ReturnInvoiceFormPage'));
const InventoryFormPage = lazy(() => import('@/pages/documents/InventoryFormPage'));
const DocumentViewPage = lazy(() => import('@/pages/documents/DocumentViewPage'));
const PosPage = lazy(() => import('@/pages/pos/PosPage'));
const CashPage = lazy(() => import('@/pages/cash/CashPage'));
const ReportsPage = lazy(() => import('@/pages/reports/ReportsPage'));
const LedgerPage = lazy(() => import('@/pages/ledger/LedgerPage'));
const ReceiptListPage = lazy(() => import('@/pages/receipts/ReceiptListPage'));
const ReceiptDetailPage = lazy(() => import('@/pages/receipts/ReceiptDetailPage'));
const DebtorsPage = lazy(() => import('@/pages/debtors/DebtorsPage'));
const UsersPage = lazy(() => import('@/pages/users/UsersPage'));
const SettingsPage = lazy(() => import('@/pages/settings/SettingsPage'));
const PrroPage = lazy(() => import('@/pages/prro/PrroPage'));
const PrroSettings = lazy(() => import('@/pages/settings/PrroSettings'));
const WorkTimePage = lazy(() => import('@/pages/work-time/WorkTimePage'));
const PrintTemplatesPage = lazy(() => import('@/pages/settings/PrintTemplatesPage'));
const DevicesPage = lazy(() => import('@/pages/settings/DevicesPage'));
const DeviceSyncPage = lazy(() => import('@/pages/settings/DeviceSyncPage'));
const StoresPage = lazy(() => import('@/pages/settings/StoresPage'));
const PrintLabelsPriceTagsPage = lazy(() => import('@/pages/printing/PrintLabelsPriceTagsPage'));
const OnboardingPage = lazy(() => import('@/pages/onboarding/OnboardingPage'));
const SetupPage = lazy(() => import('@/pages/setup/SetupPage'));
const AvailabilityPage = lazy(() => import('@/pages/inventory/AvailabilityPage'));

const PageLoader: React.FC = () => (
  <div className="flex items-center justify-center min-h-[60vh]">
    <div className="text-center">
      <Spinner size="lg" />
      <p className="mt-4 text-sm text-gray-500">Завантаження...</p>
    </div>
  </div>
);

const App: React.FC = () => {
  const initialize = useAuthStore((state) => state.initialize);
  const theme = useUIStore((state) => state.theme);
  const setTheme = useUIStore((state) => state.setTheme);

  useEffect(() => {
    initialize();
    // Apply theme on mount
    const savedTheme = localStorage.getItem('theme') as 'light' | 'dark' | null;
    if (savedTheme) {
      setTheme(savedTheme);
    } else if (window.matchMedia('(prefers-color-scheme: dark)').matches) {
      setTheme('dark');
    }
  }, [initialize, setTheme]);

  useEffect(() => {
    if (theme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, [theme]);

  return (
    <BrowserRouter
  future={{
    v7_startTransition: true,
    v7_relativeSplatPath: true,
  }}>
      <Suspense fallback={<PageLoader />}>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          {/* Майстер першого встановлення — САМОДОСТАТНІЙ (без ProtectedRoute):
              на fresh-БД авторизації ще немає, сторінка створює першого власника. */}
          <Route path="/setup" element={<SetupPage />} />
          <Route
            path="/onboarding"
            element={
              <ProtectedRoute>
                <OnboardingPage />
              </ProtectedRoute>
            }
          />
          <Route
            path="/"
            element={
              <ProtectedRoute>
                <AppLayout />
              </ProtectedRoute>
            }
          >
            {/* Доступно всім */}
            <Route index element={
              <RoleRoute roles={['admin']}>
                <DashboardPage />
              </RoleRoute>
            } />
            <Route path="pos" element={<PosPage />} />
            <Route path="cash" element={
              <RoleRoute roles={['admin', 'owner']}>
                <CashPage />
              </RoleRoute>
            } />
            <Route path="inventory/availability" element={<AvailabilityPage />} />
            <Route path="debtors" element={<DebtorsPage />} />
            <Route path="products" element={<ProductListPage />} />
            <Route path="products/new" element={
              <RoleRoute roles={['admin']}>
                <ProductFormPage />
              </RoleRoute>
            } />
            <Route path="products/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <ProductFormPage />
              </RoleRoute>
            } />
            <Route path="receipts" element={<ReceiptListPage />} />
            <Route path="receipts/:id" element={<ReceiptDetailPage />} />

            {/* Тільки для адміністратора */}
            <Route path="categories" element={
              <RoleRoute roles={['admin']}>
                <CategoryListPage />
              </RoleRoute>
            } />
            <Route path="suppliers" element={
              <RoleRoute roles={['admin']}>
                <SupplierListPage />
              </RoleRoute>
            } />
            <Route path="suppliers/new" element={
              <RoleRoute roles={['admin']}>
                <SupplierFormPage />
              </RoleRoute>
            } />
            <Route path="suppliers/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <SupplierFormPage />
              </RoleRoute>
            } />
            <Route path="suppliers/:id/products" element={
              <RoleRoute roles={['admin']}>
                <SupplierProductsPage />
              </RoleRoute>
            } />

            {/* Накладні — касир може переглядати та створювати накладні */}
            <Route path="documents" element={<DocumentListPage />} />
            <Route path="documents/invoice/new" element={<InvoiceFormPage />} />
            <Route path="documents/purchase-order/new" element={
              <RoleRoute roles={['admin']}>
                <PurchaseOrderFormPage />
              </RoleRoute>
            } />
            <Route path="documents/transfer/new" element={
              <RoleRoute roles={['admin']}>
                <TransferFormPage />
              </RoleRoute>
            } />
            <Route path="documents/write-off/new" element={
              <RoleRoute roles={['admin']}>
                <WriteOffFormPage />
              </RoleRoute>
            } />
            <Route path="documents/return/new" element={
              <RoleRoute roles={['admin']}>
                <ReturnInvoiceFormPage />
              </RoleRoute>
            } />
            <Route path="documents/inventory/new" element={
              <RoleRoute roles={['admin']}>
                <InventoryFormPage />
              </RoleRoute>
            } />
            {/* VIEW — інвентаризація */}
            <Route path="documents/inventory/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            {/* EDIT — інвентаризація */}
            <Route path="documents/inventory/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <InventoryFormPage />
              </RoleRoute>
            } />
            {/* VIEW */}
            <Route path="documents/invoice/:id" element={<DocumentViewPage />} />
            {/* EDIT */}
            <Route path="documents/invoice/:id/edit" element={<InvoiceFormPage />} />
            {/* VIEW */}
            <Route path="documents/purchase-order/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            {/* EDIT */}
            <Route path="documents/purchase-order/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <PurchaseOrderFormPage />
              </RoleRoute>
            } />
            {/* VIEW */}
            <Route path="documents/transfer/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            {/* EDIT */}
            <Route path="documents/transfer/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <TransferFormPage />
              </RoleRoute>
            } />
            {/* VIEW */}
            <Route path="documents/write-off/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            {/* EDIT */}
            <Route path="documents/write-off/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <WriteOffFormPage />
              </RoleRoute>
            } />
            {/* VIEW */}
            <Route path="documents/return/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            {/* EDIT */}
            <Route path="documents/return/:id/edit" element={
              <RoleRoute roles={['admin']}>
                <ReturnInvoiceFormPage />
              </RoleRoute>
            } />

            <Route path="reports" element={
              <RoleRoute roles={['admin']}>
                <ReportsPage />
              </RoleRoute>
            } />

            {/* Взаєморозрахунки — касир може переглядати */}
            <Route path="ledger" element={<LedgerPage />} />

            <Route path="users" element={
              <RoleRoute roles={['admin']}>
                <UsersPage />
              </RoleRoute>
            } />
            <Route path="settings" element={
              <RoleRoute roles={['admin']}>
                <SettingsPage />
              </RoleRoute>
            } />
            <Route path="settings/print-templates" element={
              <RoleRoute roles={['admin']}>
                <PrintTemplatesPage />
              </RoleRoute>
            } />
            <Route path="settings/prro" element={
              <RoleRoute roles={['admin']}>
                <PrroSettings />
              </RoleRoute>
            } />
            <Route path="settings/devices" element={
              <RoleRoute roles={['admin']}>
                <DevicesPage />
              </RoleRoute>
            } />
            <Route path="settings/device-sync" element={
              <RoleRoute roles={['admin']}>
                <DeviceSyncPage />
              </RoleRoute>
            } />
            <Route path="settings/stores" element={
              <RoleRoute roles={['admin']}>
                <StoresPage />
              </RoleRoute>
            } />
            <Route path="prro" element={
              <RoleRoute roles={['admin', 'cashier']}>
                <PrroPage />
              </RoleRoute>
            } />
            <Route path="work-time" element={<WorkTimePage />} />
            <Route path="printing" element={<PrintLabelsPriceTagsPage />} />
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Suspense>
    </BrowserRouter>
  );
};

export default App;
