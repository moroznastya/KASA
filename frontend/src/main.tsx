import React from 'react';
import ReactDOM from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Toaster } from 'react-hot-toast';
import App from './App';
import './index.css';
import { logFrontendError } from './services/tauri/offline';

// ── Фронтенд-пастка помилок (діагностика «синього екрану» у Tauri) ──
// Усі помилки JS та неперехоплені проміси пишуться у /tmp/kasa-frontend.log
// через Tauri-команду log_frontend_error, щоб знайти, що саме падає на /pos.
if (typeof window !== 'undefined') {
  window.addEventListener('error', (e) => {
    void logFrontendError('window.onerror: ' + (e.message || '') + ' @ ' + (e.filename || '') + ':' + (e.lineno || ''));
  });
  window.addEventListener('unhandledrejection', (e) => {
    void logFrontendError('unhandledrejection: ' + String(e.reason).slice(0, 300));
  });
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
      staleTime: 1000 * 60 * 5, // 5 minutes
    },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
      <Toaster
        position="top-right"
        toastOptions={{
          duration: 3000,
          style: {
            borderRadius: '12px',
            padding: '12px 16px',
            fontSize: '14px',
          },
          success: {
            iconTheme: {
              primary: '#16a34a',
              secondary: '#fff',
            },
          },
          error: {
            iconTheme: {
              primary: '#dc2626',
              secondary: '#fff',
            },
          },
        }}
      />
    </QueryClientProvider>
  </React.StrictMode>
);
