import React, { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';
import { useUIStore } from '@/store/uiStore';
import { AppLayout } from '@/components/layout/AppLayout';
import { ProtectedRoute } from '@/components/layout/ProtectedRoute';
import { LoginPage } from '@/pages/auth/LoginPage';
import { DashboardPage } from '@/pages/dashboard/DashboardPage';
import { ProductListPage } from '@/pages/products/ProductListPage';
import { ProductFormPage } from '@/pages/products/ProductFormPage';
import { CategoryListPage } from '@/pages/categories/CategoryListPage';
import { SupplierListPage } from '@/pages/suppliers/SupplierListPage';
import { SupplierFormPage } from '@/pages/suppliers/SupplierFormPage';
import { DocumentListPage } from '@/pages/documents/DocumentListPage';
import { InvoiceFormPage } from '@/pages/documents/InvoiceFormPage';
import { TransferFormPage } from '@/pages/documents/TransferFormPage';
import { WriteOffFormPage } from '@/pages/documents/WriteOffFormPage';
import { ReturnInvoiceFormPage } from '@/pages/documents/ReturnInvoiceFormPage';
import { PosPage } from '@/pages/pos/PosPage';
import { ReportsPage } from '@/pages/reports/ReportsPage';
import { LedgerPage } from '@/pages/ledger/LedgerPage';

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
          <Route index element={<DashboardPage />} />
          <Route path="pos" element={<PosPage />} />
          <Route path="products" element={<ProductListPage />} />
          <Route path="products/new" element={<ProductFormPage />} />
          <Route path="products/:id/edit" element={<ProductFormPage />} />
          <Route path="categories" element={<CategoryListPage />} />
          <Route path="suppliers" element={<SupplierListPage />} />
          <Route path="suppliers/new" element={<SupplierFormPage />} />
          <Route path="suppliers/:id/edit" element={<SupplierFormPage />} />
          <Route path="documents" element={<DocumentListPage />} />
          <Route path="documents/invoice/new" element={<InvoiceFormPage />} />
          <Route path="documents/transfer/new" element={<TransferFormPage />} />
          <Route path="documents/write-off/new" element={<WriteOffFormPage />} />
          <Route path="documents/return/new" element={<ReturnInvoiceFormPage />} />
          <Route path="reports" element={<ReportsPage />} />
          <Route path="ledger" element={<LedgerPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </BrowserRouter>
  );
};

export default App;
