import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';
import { authService } from '@/services/authService';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select, SelectOption } from '@/components/ui/Select';
import { User, ArrowLeft, Loader2 } from 'lucide-react';
import toast from 'react-hot-toast';
import api from '@/services/api';

interface UserOption {
  id: string;
  name: string;
  login: string;
}

const LoginPage: React.FC = () => {
  const navigate = useNavigate();
  const storeLogin = useAuthStore((state) => state.login);
  const [users, setUsers] = useState<UserOption[]>([]);
  const [selectedUser, setSelectedUser] = useState<UserOption | null>(null);
  const [password, setPassword] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isFetchingUsers, setIsFetchingUsers] = useState(true);

  useEffect(() => {
    api.get<UserOption[]>('/auth/users-list')
      .then((res) => setUsers(res.data))
      .catch(() => toast.error('Не вдалося завантажити список користувачів'))
      .finally(() => setIsFetchingUsers(false));
  }, []);

  const userOptions: SelectOption[] = users.map((u) => ({
    value: u.id,
    label: u.name,
  }));

  const handleSelectUser = (e: { target: { value: string } }) => {
    const user = users.find(u => u.id === e.target.value);
    setSelectedUser(user || null);
    setPassword('');
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedUser) {
      toast.error('Виберіть користувача');
      return;
    }
    if (!password.trim()) {
      toast.error('Введіть пароль');
      return;
    }

    setIsLoading(true);
    try {
      const response = await authService.login({
        login: selectedUser.login,
        password: password,
      });
      storeLogin(response.user, response.access_token, response.refresh_token);
      toast.success('Вхід виконано успішно');
      navigate('/');
    } catch (error: any) {
      const detail = error?.response?.data?.detail;
      if (Array.isArray(detail)) {
        const messages = detail.map((d: any) => {
          const field = d.loc?.slice(1).join('.') || '';
          return field ? `${field}: ${d.msg}` : d.msg;
        });
        toast.error(messages.join('\n') || 'Помилка входу');
      } else if (typeof detail === 'string') {
        toast.error(detail);
      } else {
        toast.error('Невірний пароль');
      }
      setPassword('');
    } finally {
      setIsLoading(false);
    }
  };

  const handleBack = () => {
    setSelectedUser(null);
    setPassword('');
  };

  return (
    <div className="min-h-screen bg-gradient-to-br from-primary-50 to-blue-100 dark:from-slate-900 dark:to-slate-800 flex items-center justify-center p-4">
      <div className="w-full max-w-sm">
        {/* Logo */}
        <div className="text-center mb-8">
          <div className="inline-flex items-center justify-center w-16 h-16 bg-primary-600 rounded-2xl mb-4 shadow-lg shadow-primary-200 dark:shadow-primary-900/30">
            <span className="text-white font-bold text-2xl">K</span>
          </div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Kasa POS</h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Система управління продажами
          </p>
        </div>

        {/* Card */}
        <div className="card p-6">
          {!selectedUser ? (
            // Step 1: Select user
            <div>
              <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-4">
                Вхід в систему
              </h2>

              {isFetchingUsers ? (
                <div className="flex items-center justify-center py-8">
                  <Loader2 className="w-6 h-6 animate-spin text-gray-400" />
                </div>
              ) : (
                <div className="space-y-2">
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                    Виберіть користувача
                  </label>
                  <Select
                    options={userOptions}
                    placeholder="— Оберіть користувача —"
                    leftIcon={<User className="w-4 h-4" />}
                    onChange={handleSelectUser}
                  />
                </div>
              )}
            </div>
          ) : (
            // Step 2: Enter password
            <form onSubmit={handleSubmit}>
              <div className="flex items-center justify-between mb-4">
                <button aria-label="Назад"
                  type="button"
                  onClick={handleBack}
                  className="p-1 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
                >
                  <ArrowLeft className="w-5 h-5" />
                </button>
                <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                  Введіть пароль
                </h2>
                <div className="w-7" />
              </div>

              <p className="text-sm text-gray-500 dark:text-gray-400 text-center mb-6">
                Користувач: <span className="font-medium text-gray-700 dark:text-gray-300">{selectedUser.name}</span>
              </p>

              <Input
                label="Пароль"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Введіть пароль"
                autoFocus
              />

              <Button
                type="submit"
                className="w-full mt-6"
                size="lg"
                isLoading={isLoading}
                disabled={!password.trim()}
              >
                Увійти
              </Button>
            </form>
          )}
        </div>

        <p className="text-center text-xs text-gray-400 dark:text-gray-500 mt-6">
          Kasa POS v1.0 &copy; {new Date().getFullYear()}
        </p>
      </div>
    </div>
  );
};

export default LoginPage;
