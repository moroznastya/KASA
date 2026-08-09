import axios, { AxiosError, InternalAxiosRequestConfig } from 'axios';
import { useAuthStore } from '@/store/authStore';

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
    // ТИМЧАСОВЕ ЛОГУВАННЯ — прибрати після діагностики 401
    // ═══════════════════════════════════════════════════════════════
    console.log(
      '[API] Request:',
      config.method?.toUpperCase(),
      config.url,
      'HasToken:',
      !!token,
      'Token10:',
      token ? token.substring(0, 10) + '...' + token.slice(-10) : 'N/A',
      'ContentType:',
      config.headers['Content-Type'] || 'N/A',
    );
    // ═══════════════════════════════════════════════════════════════
    if (token) {
      config.headers.Authorization = `Bearer ${token}`;
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
      // ТИМЧАСОВЕ ЛОГУВАННЯ 401 
      // ═══════════════════════════════════════════════════════════════
      const failedToken = useAuthStore.getState().accessToken;
      let tokenExpInfo = 'N/A';
      if (failedToken) {
        try {
          const p = JSON.parse(atob(failedToken.split('.')[1]));
          tokenExpInfo = `exp=${new Date(p.exp*1000).toLocaleString()} iat=${new Date(p.iat*1000).toLocaleString()} role=${p.role}`;
        } catch(e) { tokenExpInfo = `parse error: ${e}`; }
      }
      console.log(
        '[API] 401 отримано для:',
        originalRequest.url,
        'Method:',
        originalRequest.method?.toUpperCase(),
        'RefreshToken:',
        !!useAuthStore.getState().refreshToken,
        'TokenInfo:',
        tokenExpInfo,
        'ResponseHeaders:',
        error.response?.headers,
      );
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
