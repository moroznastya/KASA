import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'path';

export default defineConfig({
  // base './' — відносні шляхи до assets (критично для Tauri: без цього
  // WebKit після substitute-data вважає origin = http://localhost і
  // /assets/*.js резолвиться як http://localhost/assets/*.js →
  // "Could not connect to localhost: Connection refused").
  base: './',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    host: '0.0.0.0',
    port: 5173,
    // Не стежити за src-tauri/target (сотні тисяч файлів Rust-білду) —
    // інакше ENOSPC (file watchers) і vite падає.
    watch: {
      ignored: ['**/src-tauri/target/**', '**/node_modules/**'],
    },
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
});
