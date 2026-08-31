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
  onboarding_completed?: boolean;  // Онбординг завершено (owner/admin)
  permissions?: string[] | null;  // Список прав доступу
  created_at: string;
  updated_at: string;
}

export type UserRole = 'admin' | 'cashier' | 'manager' | 'owner';

export interface LoginRequest {
  login: string;
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

/**
 * Структура групи прав для відображення на фронтенді.
 */
export interface PermissionGroup {
  name: string;
  icon: string;
  permissions: PermissionItem[];
}

export interface PermissionItem {
  key: string;
  label: string;
  description: string;
}

export interface PermissionsListResponse {
  groups: PermissionGroup[];
  all_permissions: string[];
}
