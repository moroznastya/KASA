import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  UserCog,
  Store as StoreIcon,
  ShoppingCart,
  CheckCircle2,
  ArrowRight,
  ArrowLeft,
  Building2,
  UserPlus,
} from 'lucide-react';
import { useAuthStore } from '@/store/authStore';
import { useStoreStore } from '@/store/storeStore';
import { storeService } from '@/services/storeService';
import { userService } from '@/services/userService';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import toast from 'react-hot-toast';

/**
 * Онбординг (Етап 4 мультиточковості): 4 кроки — власник → точка → каса → готово.
 *
 * ⚠️ Перший власник створюється через /setup (майстер першого встановлення,
 *    POST /api/v1/setup — публічний, без JWT). На fresh-БД LoginPage редиректить
 *    на /setup; сюди цей флоу потрапляє вже з авторизованим користувачем, який
 *    увійшов, але GET /api/v1/stores повернув пустий список
 *    (AppLayout → redirect на /onboarding).
 *
 * TODO (відомі обмеження, задокументовані):
 *  - Пермішен inventory.view_other_stores на бекенді ще не існує — коли з'явиться,
 *    сторінка наявності підхопить його автоматично.
 */
const STEPS = ['Власник', 'Точка', 'Каса', 'Готово'];

const OnboardingPage: React.FC = () => {
  const navigate = useNavigate();
  const user = useAuthStore((state) => state.user);
  const loadStores = useStoreStore((state) => state.loadStores);

  const [step, setStep] = useState(0);

  // Крок 2: точка
  const [storeName, setStoreName] = useState('');
  const [storeAddress, setStoreAddress] = useState('');
  const [storePhone, setStorePhone] = useState('');
  const [creatingStore, setCreatingStore] = useState(false);

  // Крок 3: каса (касир)
  const [cashierName, setCashierName] = useState('');
  const [cashierLogin, setCashierLogin] = useState('');
  const [cashierPassword, setCashierPassword] = useState('');
  const [creatingCashier, setCreatingCashier] = useState(false);

  // Касир не може проходити онбординг власника.
  if (user?.role === 'cashier') {
    return (
      <div className="min-h-screen bg-gradient-to-br from-primary-50 to-blue-100 dark:from-slate-900 dark:to-slate-800 flex items-center justify-center p-4">
        <div className="w-full max-w-md card p-8 text-center">
          <Building2 className="w-12 h-12 text-primary-500 mx-auto mb-4" />
          <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100 mb-2">
            Немає доступу до точок
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-6">
            Вас ще не призначено на жодну торговельну точку. Зверніться до адміністратора.
          </p>
          <Button onClick={() => navigate('/login')}>На сторінку входу</Button>
        </div>
      </div>
    );
  }

  const handleCreateStore = async () => {
    if (!storeName.trim()) {
      toast.error('Введіть назву точки');
      return;
    }
    setCreatingStore(true);
    try {
      const store = await storeService.create({
        name: storeName.trim(),
        address: storeAddress.trim() || undefined,
        phone: storePhone.trim() || undefined,
      });
      toast.success(`Точку «${store.name}» створено`);
      await loadStores();
      setStep(2);
    } catch (e: any) {
      toast.error(e?.response?.data?.detail || 'Не вдалося створити точку');
    } finally {
      setCreatingStore(false);
    }
  };

  const handleCreateCashier = async (skip: boolean) => {
    if (!skip) {
      if (!cashierName.trim() || !cashierLogin.trim() || !cashierPassword.trim()) {
        toast.error('Заповніть ім\'я, логін і пароль касира');
        return;
      }
      setCreatingCashier(true);
      try {
        const created = await userService.create({
          name: cashierName.trim(),
          login: cashierLogin.trim(),
          password: cashierPassword,
          role: 'cashier',
          is_active: true,
        });
        toast.success(`Касира «${created.name}» створено`);
      } catch (e: any) {
        toast.error(e?.response?.data?.detail || 'Не вдалося створити касира');
        setCreatingCashier(false);
        return;
      }
    }
    setCreatingCashier(false);
    setStep(3);
  };

  const finish = async () => {
    if (user) {
      try {
        const updated = await userService.update(user.id, { onboarding_completed: true });
        useAuthStore.getState().setUser(updated);
        localStorage.setItem('user', JSON.stringify(updated));
      } catch {
        // Мережа впала — локально позначаємо завершеним, щоб користувач не застряг.
        const localUser = { ...user, onboarding_completed: true };
        useAuthStore.getState().setUser(localUser);
        localStorage.setItem('user', JSON.stringify(localUser));
      }
    }
    navigate('/');
  };

  return (
    <div className="min-h-screen bg-gradient-to-br from-primary-50 to-blue-100 dark:from-slate-900 dark:to-slate-800 flex items-center justify-center p-4">
      <div className="w-full max-w-lg">
        {/* Logo */}
        <div className="text-center mb-6">
          <div className="inline-flex items-center justify-center w-14 h-14 bg-primary-600 rounded-2xl mb-3 shadow-lg shadow-primary-200 dark:shadow-primary-900/30">
            <span className="text-white font-bold text-2xl">K</span>
          </div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Налаштування системи</h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Створіть першу торговельну точку та касу
          </p>
        </div>

        {/* Step indicator */}
        <div className="flex items-center justify-center gap-2 mb-6">
          {STEPS.map((label, i) => (
            <React.Fragment key={label}>
              {i > 0 && <div className={`h-px w-8 ${i <= step ? 'bg-primary-500' : 'bg-gray-300 dark:bg-slate-600'}`} />}
              <div className="flex flex-col items-center gap-1">
                <div
                  className={`
                    w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors
                    ${i < step
                      ? 'bg-primary-500 text-white'
                      : i === step
                        ? 'bg-primary-600 text-white ring-4 ring-primary-200 dark:ring-primary-900/40'
                        : 'bg-gray-200 dark:bg-slate-700 text-gray-500 dark:text-gray-400'}
                  `}
                >
                  {i < step ? <CheckCircle2 className="w-4 h-4" /> : i + 1}
                </div>
                <span className={`text-[11px] ${i <= step ? 'text-primary-700 dark:text-primary-400' : 'text-gray-400 dark:text-gray-500'}`}>
                  {label}
                </span>
              </div>
            </React.Fragment>
          ))}
        </div>

        {/* Card */}
        <div className="card p-6">
          {step === 0 && (
            <div className="space-y-4">
              <div className="flex items-center gap-3 p-4 rounded-xl bg-primary-50 dark:bg-primary-900/10 border border-primary-100 dark:border-primary-800">
                <div className="w-10 h-10 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center flex-shrink-0">
                  <UserCog className="w-5 h-5 text-primary-600 dark:text-primary-400" />
                </div>
                <div className="min-w-0">
                  <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                    {user?.name}
                  </p>
                  <p className="text-xs text-gray-500 dark:text-gray-400">
                    Ви — власник системи ({user?.role})
                  </p>
                </div>
              </div>
              <p className="text-sm text-gray-600 dark:text-gray-300 leading-relaxed">
                Ви увійшли як власник. Далі створіть першу торговельну точку, на якій
                працюватиме каса.
              </p>
              <p className="text-xs text-gray-400 leading-relaxed">
                ℹ️ Створення першого облікового запису власника виконується на бекенді
                (немає публічного setup-ендпоінта) — ви вже авторизовані, тож крок пропускається.
              </p>
              <div className="flex justify-end">
                <Button onClick={() => setStep(1)} icon={<ArrowRight className="w-4 h-4" />}>
                  Далі
                </Button>
              </div>
            </div>
          )}

          {step === 1 && (
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-gray-700 dark:text-gray-300">
                <StoreIcon className="w-5 h-5 text-primary-500" />
                <h2 className="text-lg font-semibold">Торговельна точка</h2>
              </div>
              <Input
                label="Назва точки *"
                placeholder="Наприклад: Магазин на Головній"
                value={storeName}
                onChange={(e) => setStoreName(e.target.value)}
              />
              <Input
                label="Адреса"
                placeholder="вул. Головна, 1"
                value={storeAddress}
                onChange={(e) => setStoreAddress(e.target.value)}
              />
              <Input
                label="Телефон"
                placeholder="+380..."
                value={storePhone}
                onChange={(e) => setStorePhone(e.target.value)}
              />
              <div className="flex justify-between pt-2">
                <Button variant="secondary" onClick={() => setStep(0)} icon={<ArrowLeft className="w-4 h-4" />}>
                  Назад
                </Button>
                <Button onClick={handleCreateStore} isLoading={creatingStore} icon={<ArrowRight className="w-4 h-4" />}>
                  Створити точку
                </Button>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-gray-700 dark:text-gray-300">
                <ShoppingCart className="w-5 h-5 text-primary-500" />
                <h2 className="text-lg font-semibold">Створення каси</h2>
              </div>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                Створіть обліковий запис касира для роботи на цій точці.
              </p>
              <Input
                label="Ім'я касира *"
                placeholder="ПІБ"
                value={cashierName}
                onChange={(e) => setCashierName(e.target.value)}
              />
              <Input
                label="Логін *"
                placeholder="login"
                value={cashierLogin}
                onChange={(e) => setCashierLogin(e.target.value)}
              />
              <Input
                label="Пароль *"
                type="password"
                value={cashierPassword}
                onChange={(e) => setCashierPassword(e.target.value)}
              />
              <div className="flex justify-between pt-2">
                <Button variant="secondary" onClick={() => setStep(1)} icon={<ArrowLeft className="w-4 h-4" />}>
                  Назад
                </Button>
                <div className="flex gap-2">
                  <Button variant="secondary" onClick={() => handleCreateCashier(true)}>
                    Пропустити
                  </Button>
                  <Button
                    onClick={() => handleCreateCashier(false)}
                    isLoading={creatingCashier}
                    icon={<UserPlus className="w-4 h-4" />}
                  >
                    Створити касира
                  </Button>
                </div>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="space-y-4 text-center">
              <CheckCircle2 className="w-14 h-14 text-green-500 mx-auto" />
              <h2 className="text-xl font-bold text-gray-900 dark:text-gray-100">
                Готово!
              </h2>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                Систему налаштовано. Точку створено, тепер можна починати роботу.
              </p>
              <div className="pt-2">
                <Button onClick={finish} icon={<ArrowRight className="w-4 h-4" />}>
                  Почати роботу
                </Button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default OnboardingPage;
