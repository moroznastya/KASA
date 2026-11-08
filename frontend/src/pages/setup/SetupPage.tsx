import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';
import { authService } from '@/services/authService';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { ArrowRight, ArrowLeft, Lock, User, Store as StoreIcon, UserCog, Loader2, LogIn } from 'lucide-react';
import toast from 'react-hot-toast';
import api from '@/services/api';
import logo from '@/assets/logo.png';


/**
 * Майстер першого встановлення (fresh-БД без користувачів).
 *
 * Роут /setup — САМОДОСТАТНІЙ (без ProtectedRoute): на свіжій БД авторизації
 * ще немає, тому ця сторінка створює ПЕРШОГО власника через публічний
 * POST /api/v1/setup (див. torgashka-api/src/setup.rs).
 *
 * Два режими на кроку 1 «Авторизація»:
 *   • «Створити нову систему» — логін + пароль майбутнього власника →
 *     крок 2 «Власник і точка».
 *   • «Увійти з наявними даними» — вхід за вже існуючим обліковим записом
 *     (напр., igor2104@i.ua / white119), якщо БД вже наповнена даними.
 *
 * Після успіху: токени зберігаються через useAuthStore.login() → '/'
 * (AppLayout покаже sidebar — у власника вже є точка із user_stores).
 */
type SetupMode = 'create' | 'login';

const SetupPage: React.FC = () => {
  const navigate = useNavigate();
  const storeLogin = useAuthStore((state) => state.login);

  const [mode, setMode] = useState<SetupMode>('create');
  const [step, setStep] = useState(0);
  const [checking, setChecking] = useState(true);
  const [initialized, setInitialized] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  // Крок 1 (create): авторизація нового власника
  const [login, setLogin] = useState('');
  const [password, setPassword] = useState('');

  // Крок 1 (login): вхід за наявними даними
  const [existLogin, setExistLogin] = useState('');
  const [existPassword, setExistPassword] = useState('');

  // Крок 2: власник і точка
  const [name, setName] = useState('');
  const [storeName, setStoreName] = useState('');
  const [storeAddress, setStoreAddress] = useState('');
  const [storePhone, setStorePhone] = useState('');

  // Якщо система вже ініціалізована — setup недоступний, ідемо на вхід.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api.get<{ status: string }>('/setup/status');
        if (!cancelled && res.data.status === 'initialized') {
          // Система вже ініціалізована: замість створення показуємо вхід
          // з наявними даними прямо в майстрі першого запуску.
          setInitialized(true);
          setMode('login');
          return;
        }
      } catch {
        // Мережа недоступна — дозволяємо форму, помилка спливе при сабміті.
      } finally {
        if (!cancelled) setChecking(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [navigate]);

  const canNext = step === 0 ? login.trim().length > 0 && password.length >= 6 : name.trim().length > 0 && storeName.trim().length > 0;
  const canLogin = existLogin.trim().length > 0 && existPassword.length > 0;

  const handleNext = () => {
    if (step === 0) {
      if (!login.trim()) return toast.error('Введіть логін');
      if (password.length < 6) return toast.error('Пароль має містити щонайменше 6 символів');
      setStep(1);
    } else {
      if (!name.trim()) return toast.error('Введіть ім\'я власника');
      if (!storeName.trim()) return toast.error('Введіть назву торговельної точки');
      void handleSubmit();
    }
  };

  const handleSubmit = async () => {
    setSubmitting(true);
    try {
      const response = await api.post('/setup', {
        name: name.trim(),
        login: login.trim(),
        password,
        store_name: storeName.trim(),
        store_address: storeAddress.trim() || undefined,
        store_phone: storePhone.trim() || undefined,
      });
      const { access_token, refresh_token, user } = response.data;
      storeLogin(user, access_token, refresh_token);
      toast.success('Систему ініціалізовано. Ласкаво просимо!');
      navigate('/', { replace: true });
    } catch (e: any) {
      const detail = e?.response?.data?.detail;
      if (typeof detail === 'string') {
        toast.error(detail);
      } else if (Array.isArray(detail)) {
        toast.error(detail.map((d: any) => d.msg).join('\n') || 'Помилка ініціалізації');
      } else {
        toast.error(e?.message || 'Не вдалося ініціалізувати систему');
      }
    } finally {
      setSubmitting(false);
    }
  };

  const handleLogin = async () => {
    setSubmitting(true);
    try {
      const { access_token, refresh_token, user } = await authService.login({
        login: existLogin.trim(),
        password: existPassword,
      });
      storeLogin(user, access_token, refresh_token);
      toast.success('Вхід виконано. Ласкаво просимо!');
      navigate('/', { replace: true });
    } catch (e: any) {
      const detail = e?.response?.data?.detail;
      if (typeof detail === 'string') {
        toast.error(detail);
      } else {
        toast.error(e?.message || 'Не вдалося увійти. Перевірте логін і пароль');
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="min-h-screen bg-gradient-to-br from-primary-50 to-blue-100 dark:from-slate-900 dark:to-slate-800 flex items-center justify-center p-4">
      <div className="w-full max-w-md">
        {/* Logo — квадрат, сторона = ширина картки */}
        <div className="mb-8">
          <img
            src={logo}
            alt="Torgashka"
            className="w-full aspect-square object-contain drop-shadow-lg"
          />
        </div>

        {/* Card */}
        <div className="card p-6">
          {checking ? (
            <div className="flex items-center justify-center py-10">
              <Loader2 className="w-6 h-6 animate-spin text-gray-400" />
            </div>
          ) : mode === 'login' ? (
            /* ─── Вхід з наявними даними ─── */
            <div className="space-y-4">
              <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Вхід з наявними даними
              </h2>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                {initialized
                  ? 'Система вже налаштована. Увійдіть за даними, що існують у ній (напр., '
                  : 'Увійдіть за даними, які вже існують у системі (напр., '}
                <span className="font-mono text-xs">igor2104@i.ua</span>).
              </p>
              <Input
                label="Логін"
                type="text"
                value={existLogin}
                onChange={(e) => setExistLogin(e.target.value)}
                placeholder="Ваш логін (e-mail або логін)"
                icon={<User className="w-4 h-4" />}
                autoFocus
              />
              <Input
                label="Пароль"
                type="password"
                value={existPassword}
                onChange={(e) => setExistPassword(e.target.value)}
                placeholder="Ваш пароль"
                icon={<Lock className="w-4 h-4" />}
              />
              <Button
                type="button"
                className="w-full mt-2"
                size="lg"
                onClick={() => void handleLogin()}
                disabled={!canLogin || submitting}
                isLoading={submitting}
              >
                <LogIn className="w-4 h-4 mr-2" /> Увійти
              </Button>
              <div className="pt-1">
                <Button
                  type="button"
                  variant="ghost"
                  className="w-full"
                  onClick={() => { setMode('create'); setExistLogin(''); setExistPassword(''); }}
                  disabled={submitting}
                >
                  <ArrowLeft className="w-4 h-4 mr-2" /> Назад до створення системи
                </Button>
              </div>
            </div>
          ) : (
            <>
              {/* Перемикач режиму (прихований, якщо система вже ініціалізована) */}
              {!initialized && (
              <div className="grid grid-cols-2 gap-2 mb-6">
                <Button
                  type="button"
                  variant={mode === 'create' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setMode('create')}
                  disabled={submitting}
                >
                  Створити нову систему
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => setMode('login')}
                  disabled={submitting}
                >
                  <LogIn className="w-4 h-4 mr-1.5" /> Увійти
                </Button>
              </div>
              )}

              {/* Прогрес кроків */}
              <div className="flex items-center gap-2 mb-6">
                {['Авторизація', 'Власник і точка'].map((label, i) => (
                  <React.Fragment key={label}>
                    {i > 0 && <div className="flex-1 h-0.5 bg-gray-200 dark:bg-slate-600 rounded" />}
                    <div className="flex items-center gap-1.5">
                      <div
                        className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-semibold ${
                          i <= step
                            ? 'bg-primary-600 text-white'
                            : 'bg-gray-200 dark:bg-slate-600 text-gray-500 dark:text-gray-300'
                        }`}
                      >
                        {i + 1}
                      </div>
                      <span className={`text-xs font-medium hidden sm:block ${i <= step ? 'text-gray-800 dark:text-gray-200' : 'text-gray-400 dark:text-gray-500'}`}>
                        {label}
                      </span>
                    </div>
                  </React.Fragment>
                ))}
              </div>

              {step === 0 ? (
                /* ─── Крок 1: Авторизація ─── */
                <div className="space-y-4">
                  <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    Дані авторизації
                  </h2>
                  <p className="text-sm text-gray-500 dark:text-gray-400">
                    Ці логін і пароль будуть даними входу власника в систему.
                  </p>
                  <Input
                    label="Логін"
                    type="text"
                    value={login}
                    onChange={(e) => setLogin(e.target.value)}
                    placeholder="Введіть логін"
                    icon={<User className="w-4 h-4" />}
                    autoFocus
                  />
                  <Input
                    label="Пароль"
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder="Мінімум 6 символів"
                    icon={<Lock className="w-4 h-4" />}
                  />
                  <Button
                    type="button"
                    className="w-full mt-2"
                    size="lg"
                    onClick={() => handleNext()}
                    disabled={!canNext}
                  >
                    Далі <ArrowRight className="w-4 h-4 ml-2" />
                  </Button>
                </div>
              ) : (
                /* ─── Крок 2: Власник і точка ─── */
                <div className="space-y-4">
                  <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    Власник і торговельна точка
                  </h2>
                  <Input
                    label="Ім'я власника"
                    type="text"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="Напр., Ігор Петренко"
                    icon={<UserCog className="w-4 h-4" />}
                    autoFocus
                  />
                  <Input
                    label="Назва точки"
                    type="text"
                    value={storeName}
                    onChange={(e) => setStoreName(e.target.value)}
                    placeholder="Напр., Магазин «Калина»"
                    icon={<StoreIcon className="w-4 h-4" />}
                  />
                  <Input
                    label="Адреса (опційно)"
                    type="text"
                    value={storeAddress}
                    onChange={(e) => setStoreAddress(e.target.value)}
                    placeholder="Напр., вул. Шевченка, 12"
                  />
                  <Input
                    label="Телефон (опційно)"
                    type="text"
                    value={storePhone}
                    onChange={(e) => setStorePhone(e.target.value)}
                    placeholder="Напр., +380 67 123 45 67"
                  />

                  <div className="flex gap-3 pt-2">
                    <Button
                      type="button"
                      variant="secondary"
                      className="flex-1"
                      size="lg"
                      onClick={() => setStep(0)}
                      disabled={submitting}
                    >
                      <ArrowLeft className="w-4 h-4 mr-2" /> Назад
                    </Button>
                    <Button
                      type="button"
                      className="flex-1"
                      size="lg"
                      onClick={() => handleNext()}
                      isLoading={submitting}
                      disabled={!canNext}
                    >
                      Створити
                    </Button>
                  </div>
                </div>
              )}
            </>
          )}
        </div>

        <p className="text-center text-xs text-gray-400 dark:text-gray-500 mt-6">
          Torgashka v1.0 &copy; {new Date().getFullYear()}
        </p>
      </div>
    </div>
  );
};

export default SetupPage;
