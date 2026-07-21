import React, { useState, useCallback, useRef, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';
import { authService } from '@/services/authService';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { User, Delete, ArrowLeft } from 'lucide-react';
import toast from 'react-hot-toast';

const PIN_LENGTH = 4;

const LoginPage: React.FC = () => {
  const navigate = useNavigate();
  const login = useAuthStore((state) => state.login);
  const [username, setUsername] = useState('');
  const [pin, setPin] = useState('');
  const [step, setStep] = useState<'username' | 'pin'>('username');
  const [isLoading, setIsLoading] = useState(false);
  const hiddenInputRef = useRef<HTMLInputElement>(null);

  // Always keep hidden input focused when on pin step
  useEffect(() => {
    if (step === 'pin' && hiddenInputRef.current) {
      hiddenInputRef.current.focus();
    }
  }, [step]);

  // Re-focus hidden input when clicking anywhere on the card
  const handleCardClick = () => {
    if (step === 'pin' && hiddenInputRef.current) {
      hiddenInputRef.current.focus();
    }
  };

  const handleUsernameSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!username.trim()) {
      toast.error('Введіть ім\'я користувача');
      return;
    }
    setStep('pin');
  };

  const handlePinKeyPress = useCallback(
    (key: string) => {
      if (key === 'delete') {
        setPin((prev) => prev.slice(0, -1));
      } else if (key === 'clear') {
        setPin('');
      } else if (pin.length < PIN_LENGTH) {
        setPin((prev) => prev + key);
      }
      // Re-focus hidden input after any action
      setTimeout(() => hiddenInputRef.current?.focus(), 0);
    },
    [pin]
  );

  const handleHiddenInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value.replace(/\D/g, '').slice(0, PIN_LENGTH);
    setPin(value);
  };

  const handleHiddenInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && pin.length === PIN_LENGTH) {
      handleLogin();
    }
    // Allow backspace to work naturally
  };

  const handleLogin = async () => {
    if (pin.length !== PIN_LENGTH) {
      toast.error('Введіть повний PIN-код');
      return;
    }

    setIsLoading(true);
    try {
      const response = await authService.loginPin({
        login: username.trim(),
        pin_code: pin,
      });
      login(response.user, response.access_token, response.refresh_token);
      toast.success('Вхід виконано успішно');
      navigate('/');
    } catch (error: any) {
      const detail = error?.response?.data?.detail || 'Невірний PIN-код';
      toast.error(detail);
      setPin('');
    } finally {
      setIsLoading(false);
    }
  };

  const handleBack = () => {
    setStep('username');
    setPin('');
  };

  const pinDisplay = Array.from({ length: PIN_LENGTH }, (_, i) => (
    <span
      key={i}
      className={`w-4 h-4 rounded-full transition-all duration-200 ${
        i < pin.length
          ? 'bg-primary-600 scale-100'
          : 'bg-gray-300 dark:bg-slate-600 scale-75'
      }`}
    />
  ));

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
        <div className="card p-6" onClick={handleCardClick}>
          {step === 'username' ? (
            <form onSubmit={handleUsernameSubmit}>
              <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-4">
                Вхід в систему
              </h2>
              <Input
                label="Ім'я користувача"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="Введіть логін"
                icon={<User className="w-4 h-4" />}
                autoFocus
              />
              <Button type="submit" className="w-full mt-6" size="lg">
                Далі
              </Button>
            </form>
          ) : (
            <div>
              <div className="flex items-center justify-between mb-4">
                <button
                  onClick={handleBack}
                  className="p-1 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
                >
                  <ArrowLeft className="w-5 h-5" />
                </button>
                <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                  Введіть PIN-код
                </h2>
                <div className="w-7" />
              </div>

              <p className="text-sm text-gray-500 dark:text-gray-400 text-center mb-6">
                Користувач: <span className="font-medium text-gray-700 dark:text-gray-300">{username}</span>
              </p>

              {/* PIN dots */}
              <div className="flex justify-center gap-3 mb-8">{pinDisplay}</div>

              {/* Hidden input for physical keyboard - always present but invisible */}
              <div className="sr-only" aria-hidden="true">
                <input
                  ref={hiddenInputRef}
                  type="password"
                  inputMode="numeric"
                  maxLength={PIN_LENGTH}
                  value={pin}
                  onChange={handleHiddenInputChange}
                  onKeyDown={handleHiddenInputKeyDown}
                  tabIndex={0}
                />
              </div>

              {/* PIN Keyboard */}
              <div className="grid grid-cols-3 gap-3 max-w-[220px] mx-auto">
                {[1, 2, 3, 4, 5, 6, 7, 8, 9].map((num) => (
                  <button
                    key={num}
                    className="pin-key"
                    onClick={() => handlePinKeyPress(String(num))}
                    disabled={pin.length >= PIN_LENGTH}
                  >
                    {num}
                  </button>
                ))}
                <button
                  className="pin-key text-sm text-gray-500"
                  onClick={() => handlePinKeyPress('clear')}
                >
                  Скид
                </button>
                <button
                  className="pin-key"
                  onClick={() => handlePinKeyPress('0')}
                  disabled={pin.length >= PIN_LENGTH}
                >
                  0
                </button>
                <button
                  className="pin-key text-danger-500"
                  onClick={() => handlePinKeyPress('delete')}
                >
                  <Delete className="w-5 h-5" />
                </button>
              </div>

              <Button
                className="w-full mt-6"
                size="lg"
                onClick={handleLogin}
                isLoading={isLoading}
                disabled={pin.length !== PIN_LENGTH}
              >
                Увійти
              </Button>
            </div>
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
