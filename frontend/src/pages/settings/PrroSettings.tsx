import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ArrowLeft,
  Save,
  Plug,
  Eye,
  EyeOff,
  FileKey,
  Radio,
  CheckCircle2,
  XCircle,
  Loader2,
} from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { usePrroStore } from '@/store/prroStore';
import { useBackNavigation } from '@/hooks/useBackNavigation';

/** Дозволені розширення ключів КЕП */
const ACCEPTED_KEY_EXTENSIONS = '.dat,.pfx,.p12,.jks,.pem,.key';

/** Формат ключа за розширенням файлу */
function detectKeyFormat(fileName: string): string {
  const ext = fileName.split('.').pop()?.toLowerCase() || '';
  const map: Record<string, string> = {
    pfx: 'pfx',
    p12: 'p12',
    jks: 'jks',
    pem: 'pem',
    dat: 'dat',
    key: 'pem',
  };
  return map[ext] || ext;
}

/** Конвертація файлу в base64 (без data-префікса) */
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      // Відрізаємо "data:application/octet-stream;base64," префікс
      const base64 = result.includes('base64,') ? result.split('base64,')[1] : result;
      resolve(base64);
    };
    reader.onerror = () => reject(new Error('Помилка читання файлу'));
    reader.readAsDataURL(file);
  });
}

const PrroSettings: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();

  const { settings, loading, savingSettings, testingConnection, loadSettings, saveSettings, testConnection } =
    usePrroStore();

  // Стан форми
  const [keyFileName, setKeyFileName] = useState<string>('');
  const [keyFileBase64, setKeyFileBase64] = useState<string | null>(null);
  const [keyPassword, setKeyPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [prroFn, setPrroFn] = useState('');
  const [prroTn, setPrroTn] = useState('');
  const [prroZn, setPrroZn] = useState('');
  const [mode, setMode] = useState<'test' | 'prod'>('test');

  // Стан перевірки з'єднання
  const [connectionResult, setConnectionResult] = useState<{ ok: boolean; message: string } | null>(null);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // Заповнюємо форму зі збережених налаштувань
  useEffect(() => {
    if (!settings) return;
    setKeyFileName(settings.key_file || '');
    setPrroFn(settings.prro_fn || '');
    setPrroTn(settings.prro_tn || '');
    setPrroZn(settings.prro_zn || '');
    setMode(settings.mode === 'prod' ? 'prod' : 'test');
  }, [settings]);

  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setKeyFileName(file.name);
    try {
      const base64 = await fileToBase64(file);
      setKeyFileBase64(base64);
    } catch {
      setKeyFileBase64(null);
    }
    // Дозволяємо вибрати той самий файл повторно
    e.target.value = '';
  };

  const handleTestConnection = async () => {
    setConnectionResult(null);
    const result = await testConnection();
    setConnectionResult(result);
  };

  const handleSave = async () => {
    const ok = await saveSettings({
      prro_fn: prroFn.trim() || null,
      prro_tn: prroTn.trim() || null,
      prro_zn: prroZn.trim() || null,
      mode,
      key_password: keyPassword || null,
      key_file_name: keyFileName || null,
      key_file_base64: keyFileBase64,
    });
    if (ok) {
      // Скидаємо пароль після збереження
      setKeyPassword('');
      setKeyFileBase64(null);
    }
  };

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      {/* Заголовок */}
      <div className="flex items-center gap-4">
        <button
          onClick={goBack}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div>
          <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Налаштування ПРРО
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Програмний РРО — електронний ключ, реквізити та режим роботи
          </p>
        </div>
      </div>

      {loading && !settings ? (
        <div className="flex justify-center py-12">
          <Loader2 className="w-8 h-8 animate-spin text-primary-500" />
        </div>
      ) : (
        <>
          {/* ─── Електронний ключ ─────────────────────────────────────── */}
          <div className="card p-6 space-y-4">
            <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              Електронний ключ КЕП
            </h3>

            {/* Файл ключа */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
                Файл електронного ключа
              </label>
              <div className="flex gap-3 items-center">
                <label className="flex-1 flex items-center gap-3 px-4 py-2.5 rounded-lg border border-gray-300 dark:border-slate-600 cursor-pointer hover:border-primary-400 dark:hover:border-primary-500 transition-colors bg-white dark:bg-slate-800">
                  <FileKey className="w-5 h-5 text-gray-400 flex-shrink-0" />
                  <span className={`text-sm truncate ${keyFileName ? 'text-gray-900 dark:text-gray-100' : 'text-gray-400'}`}>
                    {keyFileName || 'Оберіть файл ключа (.dat, .pfx, .p12, .jks, .pem)'}
                  </span>
                  <input
                    type="file"
                    accept={ACCEPTED_KEY_EXTENSIONS}
                    onChange={handleFileChange}
                    className="hidden"
                    id="prro-key-file"
                    name="prro-key-file"
                  />
                </label>
                {keyFileName && (
                  <span className="px-2.5 py-1 text-xs font-medium rounded-full bg-primary-50 text-primary-600 dark:bg-primary-900/20 dark:text-primary-400">
                    {detectKeyFormat(keyFileName)}
                  </span>
                )}
              </div>
              <p className="mt-2 text-xs text-gray-400">
                Тестові ключі покладіть у <code className="text-primary-600 dark:text-primary-400 font-mono">certs/prro-test/</code>{' '}
                (див. README). Підтримуються формати: .dat, .pfx, .p12, .jks, .pem.
              </p>
            </div>

            {/* Пароль ключа */}
            <div className="relative">
              <Input
                label="Пароль ключа"
                type={showPassword ? 'text' : 'password'}
                value={keyPassword}
                onChange={(e) => setKeyPassword(e.target.value)}
                placeholder={
                  settings?.key_password_masked
                    ? 'Пароль збережено — введіть новий для зміни'
                    : 'Введіть пароль ключа КЕП'
                }
                autoComplete="new-password"
                id="prro-key-password"
                name="prro-key-password"
              />
              <button
                type="button"
                onClick={() => setShowPassword((v) => !v)}
                className="absolute right-3 top-[38px] text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                title={showPassword ? 'Сховати пароль' : 'Показати пароль'}
              >
                {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>
            {settings?.key_password_masked && !keyPassword && (
              <p className="text-xs text-success-600 dark:text-success-400">
                ✓ Пароль збережено ({settings.key_password_masked})
              </p>
            )}
          </div>

          {/* ─── Реквізити ПРРО ───────────────────────────────────────── */}
          <div className="card p-6 space-y-4">
            <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              Реквізити ПРРО
            </h3>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <Input
                label="Фіскальний номер (ФН)"
                value={prroFn}
                onChange={(e) => setPrroFn(e.target.value)}
                placeholder="3000XXXXXX"
                id="prro-fn"
                name="prro-fn"
              />
              <Input
                label="Податковий номер (ТН)"
                value={prroTn}
                onChange={(e) => setPrroTn(e.target.value)}
                placeholder="12345678"
                id="prro-tn"
                name="prro-tn"
              />
              <Input
                label="Заводський номер (ЗН)"
                value={prroZn}
                onChange={(e) => setPrroZn(e.target.value)}
                placeholder="4000XXXXXX"
                id="prro-zn"
                name="prro-zn"
              />
            </div>
          </div>

          {/* ─── Режим роботи ─────────────────────────────────────────── */}
          <div className="card p-6 space-y-4">
            <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
              Режим роботи
            </h3>
            <div className="flex gap-4">
              <label
                className={`
                  flex-1 flex items-center gap-3 px-4 py-3 rounded-xl border-2 cursor-pointer transition-all
                  ${mode === 'test'
                    ? 'border-warning-400 bg-warning-50 dark:bg-warning-900/20'
                    : 'border-gray-200 dark:border-slate-600 hover:border-gray-300 dark:hover:border-slate-500'
                  }
                `}
              >
                <input
                  type="radio"
                  name="prro-mode"
                  value="test"
                  checked={mode === 'test'}
                  onChange={() => setMode('test')}
                  className="w-4 h-4 text-warning-500 focus:ring-warning-500"
                />
                <div>
                  <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Тестовий</p>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Тестовий фіскальний сервер</p>
                </div>
              </label>
              <label
                className={`
                  flex-1 flex items-center gap-3 px-4 py-3 rounded-xl border-2 cursor-pointer transition-all
                  ${mode === 'prod'
                    ? 'border-success-500 bg-success-50 dark:bg-success-900/20'
                    : 'border-gray-200 dark:border-slate-600 hover:border-gray-300 dark:hover:border-slate-500'
                  }
                `}
              >
                <input
                  type="radio"
                  name="prro-mode"
                  value="prod"
                  checked={mode === 'prod'}
                  onChange={() => setMode('prod')}
                  className="w-4 h-4 text-success-500 focus:ring-success-500"
                />
                <div>
                  <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Бойовий</p>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Продуктивний сервер ДПС</p>
                </div>
              </label>
            </div>
            {settings?.url && (
              <p className="text-xs text-gray-400">
                Фіскальний сервер: <span className="font-mono">{settings.url}</span>
              </p>
            )}
          </div>

          {/* ─── Результат перевірки з'єднання ────────────────────────── */}
          {connectionResult && (
            <div
              className={`
                flex items-center gap-3 px-4 py-3 rounded-xl border
                ${connectionResult.ok
                  ? 'bg-success-50 dark:bg-success-900/20 border-success-200 dark:border-success-700'
                  : 'bg-danger-50 dark:bg-danger-900/20 border-danger-200 dark:border-danger-700'
                }
              `}
            >
              {connectionResult.ok
                ? <CheckCircle2 className="w-5 h-5 text-success-600 flex-shrink-0" />
                : <XCircle className="w-5 h-5 text-danger-600 flex-shrink-0" />}
              <p className={`text-sm font-medium ${connectionResult.ok ? 'text-success-700 dark:text-success-400' : 'text-danger-700 dark:text-danger-400'}`}>
                {connectionResult.message}
              </p>
            </div>
          )}

          {/* ─── Кнопки дій ───────────────────────────────────────────── */}
          <div className="flex justify-end gap-3 pt-2">
            <Button variant="secondary" onClick={() => navigate('/prro')}>
              Відкрити вікно ПРРО
            </Button>
            <Button
              variant="secondary"
              onClick={handleTestConnection}
              isLoading={testingConnection}
              icon={<Plug className="w-4 h-4" />}
            >
              Перевірити з'єднання
            </Button>
            <Button
              onClick={handleSave}
              isLoading={savingSettings}
              icon={<Save className="w-4 h-4" />}
            >
              Зберегти
            </Button>
          </div>
        </>
      )}

      {/* Підказка про тестові ключі */}
      <div className="bg-primary-50 dark:bg-primary-900/20 border border-primary-200 dark:border-primary-800 rounded-xl p-4 text-sm text-primary-700 dark:text-primary-400">
        <p className="flex items-center gap-2 font-medium mb-1">
          <Radio className="w-4 h-4" /> Підказка
        </p>
        <p>
          Тестові ключі КЕП покладіть у каталог{' '}
          <code className="font-mono bg-primary-100 dark:bg-primary-900/40 px-1.5 py-0.5 rounded">certs/prro-test/</code>{' '}
          проєкту. У режимі «Тестовий» ПРРО працює з тестовим фіскальним сервером ДПС —
          це безпечно для навчання та перевірки.
        </p>
      </div>
    </div>
  );
};

export default PrroSettings;
