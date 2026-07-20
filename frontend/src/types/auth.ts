/**
 * Типи для авторизації та користувачів.
 * Відповідають Pydantic схемам бекенду.
 */

export interface User {
  id: string;           // UUID
  name: string;         // Повне ім'я
  login: string;        // Логін для входу
  role: UserRole;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export type UserRole = 'admin' | 'cashier' | 'manager' | 'owner';

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginPinRequest {
  login: string;        // Виправлено: username -> login
  pin_code: string;
}

export interface TokenResponse {
  access_token: string;
  refresh_token?: string;  // Опціонально, бекенд може не повертати
  token_type: string;
  user: User;
}

export interface RefreshTokenRequest {
  refresh_token: string;
}

export interface AuthState {
  user: User | null;
  accessToken: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  isLoading: boolean;
}
