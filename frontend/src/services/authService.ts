import api from './api';
import { LoginRequest, LoginPinRequest, TokenResponse, User } from '@/types/auth';

export const authService = {
  async login(data: LoginRequest): Promise<TokenResponse> {
    const formData = new FormData();
    formData.append('username', data.username);
    formData.append('password', data.password);
    const response = await api.post<TokenResponse>('/auth/login', formData, {
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    });
    return response.data;
  },

  async loginPin(data: LoginPinRequest): Promise<TokenResponse> {
    // Виправлено: передаємо login замість username
    const response = await api.post<TokenResponse>('/auth/login-pin', {
      login: data.login,
      pin_code: data.pin_code,
    });
    return response.data;
  },

  async refreshToken(refreshToken: string): Promise<TokenResponse> {
    const response = await api.post<TokenResponse>('/auth/refresh', {
      refresh_token: refreshToken,
    });
    return response.data;
  },

  async getCurrentUser(): Promise<User> {
    const response = await api.get<User>('/auth/me');
    return response.data;
  },

  async logout(): Promise<void> {
    try {
      await api.post('/auth/logout');
    } catch {
      // Ignore logout errors
    }
  },
};
