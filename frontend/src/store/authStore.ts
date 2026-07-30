import { create } from 'zustand';
import axios from 'axios';
import { User } from '@/types/auth';

const API_BASE_URL = '/api/v1';

interface AuthStore {
  user: User | null;
  accessToken: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  setUser: (user: User) => void;
  setTokens: (accessToken: string, refreshToken: string) => void;
  setLoading: (loading: boolean) => void;
  login: (user: User, accessToken: string, refreshToken?: string | null) => void;
  logout: () => void;
  initialize: () => Promise<void>;
}

function clearStorage() {
  localStorage.removeItem('accessToken');
  localStorage.removeItem('refreshToken');
  localStorage.removeItem('user');
}

export const useAuthStore = create<AuthStore>((set) => ({
  user: null,
  accessToken: null,
  refreshToken: null,
  isAuthenticated: false,
  isLoading: true,

  setUser: (user: User) => set({ user }),

  setTokens: (accessToken: string, refreshToken: string) => {
    localStorage.setItem('accessToken', accessToken);
    localStorage.setItem('refreshToken', refreshToken);
    set({ accessToken, refreshToken, isAuthenticated: true });
  },

  setLoading: (isLoading: boolean) => set({ isLoading }),

  login: (user: User, accessToken: string, refreshToken?: string | null) => {
    localStorage.setItem('accessToken', accessToken);
    // Зберігаємо refresh_token тільки якщо він реально повернутий з бекенду
    if (refreshToken) {
      localStorage.setItem('refreshToken', refreshToken);
    } else {
      // Якщо refresh_token не повернуто — не зберігаємо нічого в refreshToken
      localStorage.removeItem('refreshToken');
    }
    localStorage.setItem('user', JSON.stringify(user));
    set({
      user,
      accessToken,
      refreshToken: refreshToken || null,
      isAuthenticated: true,
      isLoading: false,
    });
  },

  logout: () => {
    clearStorage();
    set({
      user: null,
      accessToken: null,
      refreshToken: null,
      isAuthenticated: false,
      isLoading: false,
    });
  },

  initialize: async () => {
    const accessToken = localStorage.getItem('accessToken');
    const refreshToken = localStorage.getItem('refreshToken');
    const userStr = localStorage.getItem('user');

    // ═══════════════════════════════════════════════════════════════
    // ТИМЧАСОВЕ ЛОГУВАННЯ — прибрати після діагностики 401
    // ═══════════════════════════════════════════════════════════════
    // ═══════════════════════════════════════════════════════════════
    // ДІАГНОСТИКА ТОКЕНА — декодуємо exp
    // ═══════════════════════════════════════════════════════════════
    if (accessToken) {
      try {
        const payload = JSON.parse(atob(accessToken.split('.')[1]));
        console.log(
          '[Auth] initialize() - Token exp:',
          new Date(payload.exp * 1000).toLocaleString(),
          'iat:',
          new Date(payload.iat * 1000).toLocaleString(),
          'sub:',
          payload.sub,
          'role:',
          payload.role,
        );
      } catch (e) {
        console.log('[Auth] initialize() - Cannot decode token:', e);
      }
    }
    console.log(
      '[Auth] initialize() - Token:',
      !!accessToken,
      'Refresh:',
      !!refreshToken,
      'User:',
      !!userStr,
      'TokenFirstChars:',
      accessToken ? accessToken.substring(0, 10) + '...' : 'N/A',
    );
    // ═══════════════════════════════════════════════════════════════

    // Якщо немає ні токена, ні користувача — одразу виходимо
    if (!accessToken || !userStr) {
      set({ isLoading: false });
      return;
    }

    let user: User;
    try {
      user = JSON.parse(userStr) as User;
    } catch {
      // Пошкоджені дані — очищаємо
      clearStorage();
      set({ isLoading: false });
      return;
    }

    try {
      // Перевіряємо, чи токен ще валідний
      const response = await axios.get(`${API_BASE_URL}/auth/verify`, {
        headers: { Authorization: `Bearer ${accessToken}` },
      });

      if (response.data.valid) {
        // Токен валідний — встановлюємо стан
        set({
          user,
          accessToken,
          refreshToken: refreshToken || null,
          isAuthenticated: true,
          isLoading: false,
        });
        return;
      }
    } catch {
      // Токен невалідний або помилка мережі — пробуємо refresh нижче
    }

    // Спроба оновити токен через refresh_token
    if (refreshToken) {
      try {
        const refreshResponse = await axios.post(`${API_BASE_URL}/auth/refresh`, {
          refresh_token: refreshToken,
        });

        const { access_token, refresh_token: newRefreshToken } = refreshResponse.data;

        // Зберігаємо нові токени
        localStorage.setItem('accessToken', access_token);
        if (newRefreshToken) {
          localStorage.setItem('refreshToken', newRefreshToken);
        } else {
          localStorage.removeItem('refreshToken');
        }

        set({
          user,
          accessToken: access_token,
          refreshToken: newRefreshToken || null,
          isAuthenticated: true,
          isLoading: false,
        });
        return;
      } catch {
        // Refresh не вдався — очищаємо
        clearStorage();
      }
    } else {
      // Немає refresh_token — очищаємо
      clearStorage();
    }

    // Якщо дійшли сюди — автентифікація не вдалась
    set({
      user: null,
      accessToken: null,
      refreshToken: null,
      isAuthenticated: false,
      isLoading: false,
    });
  },
}));
