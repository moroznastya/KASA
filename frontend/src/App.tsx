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
const DocumentViewPage = lazy(() => import('@/pages/documents/DocumentViewPage'));
const PosPage = lazy(() => import('@/pages/pos/PosPage'));
const ReportsPage = lazy(() => import('@/pages/reports/ReportsPage'));
const LedgerPage = lazy(() => import('@/pages/ledger/LedgerPage'));
const ReceiptListPage = lazy(() => import('@/pages/receipts/ReceiptListPage'));
const ReceiptDetailPage = lazy(() => import('@/pages/receipts/ReceiptDetailPage'));
const DebtorsPage = lazy(() => import('@/pages/debtors/DebtorsPage'));
const UsersPage = lazy(() => import('@/pages/users/UsersPage'));

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
    <BrowserRouter>
      <Suspense fallback={<PageLoader />}>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
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

            {/* Документи — касир може переглядати та створювати накладні */}
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
            <Route path="documents/invoice/:id" element={<DocumentViewPage />} />
            <Route path="documents/purchase-order/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            <Route path="documents/transfer/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            <Route path="documents/write-off/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
              </RoleRoute>
            } />
            <Route path="documents/return/:id" element={
              <RoleRoute roles={['admin']}>
                <DocumentViewPage />
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
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Suspense>
    </BrowserRouter>
  );
};

export default App;
