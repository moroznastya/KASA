import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';
import { authService } from '@/services/authService';

export function useAuth() {
  const navigate = useNavigate();
  const {
    user,
    accessToken,
    isAuthenticated,
    isLoading,
    login,
    logout,
    setUser,
    setLoading,
    initialize,
  } = useAuthStore();

  useEffect(() => {
    initialize();
  }, [initialize]);

  useEffect(() => {
    if (!isLoading && isAuthenticated && accessToken) {
      authService.getCurrentUser().then(setUser).catch(() => {
        logout();
        navigate('/login');
      });
    }
  }, [isLoading, isAuthenticated, accessToken, setUser, logout, navigate]);

  const handleLogin = async (username: string, pinCode: string) => {
    // Виправлено: передаємо login замість username
    const response = await authService.loginPin({
      login: username,
      pin_code: pinCode,
    });
    login(response.user, response.access_token, response.refresh_token);
    return response;
  };

  const handleLogout = async () => {
    try {
      await authService.logout();
    } catch {
      // Ignore
    }
    logout();
    navigate('/login');
  };

  return {
    user,
    isAuthenticated,
    isLoading,
    login: handleLogin,
    logout: handleLogout,
  };
}
