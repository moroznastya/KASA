import React from 'react';
import ReactDOM from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Toaster } from 'react-hot-toast';
import { AdminApp } from './AdminApp';
import '../index.css';

/**
 * Точка входу ВЕБ-адмінки (Етап 6, ТЗ §6) — браузерна SPA (admin.html).
 * Авторизація — через /auth/login (localStorage), як у desktop-збірці.
 * На відміну від src/main.tsx не реєструє Tauri-логер помилок (у браузері
 * Tauri-команд немає).
 */
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
            <AdminApp />
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
    </React.StrictMode>,
);
