import axios, { AxiosError, InternalAxiosRequestConfig } from 'axios';
import { useAuthStore } from '@/store/authStore';
import { useStoreStore } from '@/store/storeStore';

const API_BASE_URL = import.meta.env.DEV ? '/api/v1' : (import.meta.env.VITE_API_BASE_URL || 'http://127.0.0.1:8000/api/v1');

const api = axios.create({
  baseURL: API_BASE_URL,
  timeout: 30000,
});

// Request interceptor to add JWT token
api.interceptors.request.use(
  (config: InternalAxiosRequestConfig) => {
    const token = useAuthStore.getState().accessToken;
    // ═══════════════════════════════════════════════════════════════
    if (token) {
      config.headers.Authorization = `Bearer ${token}`;
    }

    // ── Мультиточковість (Етап 4): X-Store-Id на бізнес-запитах ──
    // Публічні шляхи (/auth/*, /health) і управління точками (/stores,
    // /user-stores) НЕ потребують X-Store-Id (store_context.rs: is_public_path,
    // is_store_management_path). Решта — обов'язковий заголовок: без нього 400.
    const reqUrl = config.url || '';
    const isPublicPath =
      reqUrl.startsWith('/auth/') || reqUrl === '/health';
    const isStoreMgmtPath =
      reqUrl === '/stores' ||
      reqUrl.startsWith('/stores/') ||
      reqUrl === '/user-stores';
    if (!isPublicPath && !isStoreMgmtPath) {
      // Per-request X-Store-Id (синхронізація офлайн-чеків — кожен чек зі
      // СВОЄЮ точкою з черги) має пріоритет над поточною активною точкою.
      if (!config.headers['X-Store-Id']) {
        const storeId = useStoreStore.getState().activeStoreId;
        if (storeId) {
          config.headers['X-Store-Id'] = storeId;
        }
      }
    }

    // Якщо дані — FormData, не встановлюємо Content-Type (axios встановить multipart/form-data автоматично)
    if (!(config.data instanceof FormData)) {
      config.headers['Content-Type'] = 'application/json';
    }
    return config;
  },
  (error) => {
    return Promise.reject(error);
  }
);

// Response interceptor for token refresh
let isRefreshing = false;
let failedQueue: Array<{
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}> = [];

const processQueue = (error: AxiosError | null, token: string | null = null) => {
  failedQueue.forEach((prom) => {
    if (error) {
      prom.reject(error);
    } else {
      prom.resolve(token);
    }
  });
  failedQueue = [];
};

api.interceptors.response.use(
  (response) => response,
  async (error: AxiosError) => {
    const originalRequest = error.config as InternalAxiosRequestConfig & { _retry?: boolean };

    if (error.response?.status === 401 && !originalRequest._retry) {
      // ═══════════════════════════════════════════════════════════════

      if (isRefreshing) {
        return new Promise((resolve, reject) => {
          failedQueue.push({ resolve, reject });
        }).then((token) => {
          originalRequest.headers.Authorization = `Bearer ${token}`;
          return api(originalRequest);
        });
      }

      originalRequest._retry = true;
      isRefreshing = true;

      const refreshToken = useAuthStore.getState().refreshToken;
      if (!refreshToken) {
        useAuthStore.getState().logout();
        isRefreshing = false;
        return Promise.reject(error);
      }

      try {
        const response = await axios.post(`${API_BASE_URL}/auth/refresh`, {
          refresh_token: refreshToken,
        });

        const { access_token, refresh_token } = response.data;
        useAuthStore.getState().setTokens(access_token, refresh_token);

        processQueue(null, access_token);

        originalRequest.headers.Authorization = `Bearer ${access_token}`;
        return api(originalRequest);
      } catch (refreshError) {
        processQueue(refreshError as AxiosError, null);
        useAuthStore.getState().logout();
        return Promise.reject(refreshError);
      } finally {
        isRefreshing = false;
      }
    }

    return Promise.reject(error);
  }
);

export default api;
