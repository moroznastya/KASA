import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'path';

/**
 * Окрема збірка ВЕБ-адмінки (Етап 6, ТЗ §6) — SPA для браузера, яка
 * говорить з тим самим API сервера (VITE_API_BASE_URL), БЕЗ POS/cash
 * і без Tauri-інтеграцій.
 *
 * Основна збірка (vite.config.ts, Tauri desktop) ЦИМ ФАЙЛОМ НЕ зачіпається:
 * `npm run build` лишається без змін; веб-адмінка збирається окремо:
 *   npm run build:admin
 *
 * API сервера задається при збірці (env підставляється на етапі build):
 *   VITE_API_BASE_URL=https://api.example.com/api/v1 npm run build:admin
 *   (або '/api/v1' для розміщення на тому ж origin/за зворотним проксі).
 *
 * CORS: origin веб-збірки додається на сервері через
 *   TORGASHKA_CORS_ORIGINS=https://admin.example.com,https://example.com
 * (базові дозволені origin: tauri://localhost + localhost:5173/8000).
 *
 * Результат: dist-admin/ (не чіпає dist/ основної збірки).
 */
export default defineConfig({
    base: './',
    plugins: [react(), tailwindcss()],
    resolve: {
        alias: {
            '@': path.resolve(__dirname, './src'),
        },
    },
    server: {
        host: '0.0.0.0',
        port: 5199,
        proxy: {
            '/api': {
                target: 'http://localhost:8000',
                changeOrigin: true,
                secure: false,
            },
            '/uploads': {
                target: 'http://localhost:8000',
                changeOrigin: true,
                secure: false,
            },
        },
    },
    build: {
        outDir: 'dist-admin',
        emptyOutDir: true,
        rollupOptions: {
            input: {
                admin: path.resolve(__dirname, 'admin.html'),
            },
        },
    },
});
