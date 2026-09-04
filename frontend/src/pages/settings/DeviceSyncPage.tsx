import React, { useEffect, useState } from 'react';
import { Radio, CheckCircle2, XCircle, Loader2, MonitorSmartphone } from 'lucide-react';
import { isTauri } from '@/hooks/useTauri';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { getSetting, persistSyncDevice, clearSyncDevice } from '@/services/tauri/offline';
import {
  activateDevice,
  normalizeServerUrl,
  rememberDeviceActivation,
  forgetDeviceActivation,
  readLocalDeviceState,
} from '@/services/deviceActivationService';

/**
 * «Мережева каса» — активація каси як мережевого пристрою (device-режим
 * синхронізації, Етап 3).
 *
 * Флоу: код активації (з адмінки сервера) + адреса сервера →
 *   POST /api/v1/devices/activate (публічний) → device_token →
 *   persistSyncDevice(server_url + device_token у SQLite settings) →
 *   Rust сам (пере)запускає фонові push/pull-цикли.
 *
 * Стан активності: первинно — реальний Rust-стан (getSetting('device_token')
 * непустий); у браузері (без Tauri) — localStorage-прапор.
 */
const DeviceSyncPage: React.FC = () => {
  const isDesktop = isTauri();

  const [serverUrl, setServerUrl] = useState('');
  const [code, setCode] = useState('');
  const [deviceActive, setDeviceActive] = useState(false);
  const [storeName, setStoreName] = useState<string | null>(null);
  const [storeId, setStoreId] = useState<string | null>(null);
  const [activating, setActivating] = useState(false);
  const [deactivating, setDeactivating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // ── Первинний стан: префіл server_url + активність device-режиму ──
  useEffect(() => {
    let cancelled = false;
    (async () => {
      // getSetting — invoke у Tauri; у браузері падає → catch → null.
      let storedUrl: string | null = null;
      let storedToken: string | null = null;
      try {
        storedUrl = await getSetting('server_url');
      } catch {
        storedUrl = null;
      }
      try {
        storedToken = await getSetting('device_token');
      } catch {
        storedToken = null;
      }
      const local = readLocalDeviceState();
      if (cancelled) return;

      if (storedUrl) setServerUrl(normalizeServerUrl(storedUrl));

      // Rust-стан первинний (SQLite settings); у браузері — localStorage.
      const rustActive = storedToken !== null && storedToken.trim() !== '';
      const active = rustActive || (!isDesktop && local.active);
      setDeviceActive(active);
      if (active) {
        setStoreName(local.storeName);
        setStoreId(local.storeId);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [isDesktop]);

  const handleActivate = async () => {
    setError(null);
    setSuccess(null);
    const normUrl = normalizeServerUrl(serverUrl);
    if (!normUrl) {
      setError('Вкажіть адресу сервера (наприклад, http://192.168.1.10:8000)');
      return;
    }
    if (!code.trim()) {
      setError('Вкажіть код активації, виданий адміністратором');
      return;
    }
    setActivating(true);
    try {
      const result = await activateDevice(code, normUrl);
      // Збереження в SQLite settings (server_url + device_token) —
      // Rust побачить непустий device_token і (пере)запустить синки.
      const saved = await persistSyncDevice(normUrl, result.device_token);
      rememberDeviceActivation(result, normUrl);

      setDeviceActive(true);
      setStoreName(result.store_name);
      setStoreId(result.store_id);
      setCode('');
      if (saved) {
        setSuccess(
          `Пристрій активовано: «${result.store_name}». Device-режим увімкнено, фонові синки запущено.`
        );
      } else {
        // Браузер або стара версія без set_setting: сервер активацію підтвердив,
        // але локальний Rust-клієнт токен не отримав — синки не запустяться.
        setSuccess(
          `Сервер підтвердив активацію: «${result.store_name}». Але зберегти токен у налаштування каси не вдалося — повноцінний device-режим працює лише в десктоп-версії Torgashka.`
        );
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Помилка активації. Спробуйте ще раз.');
    } finally {
      setActivating(false);
    }
  };

  const handleDeactivate = async () => {
    setError(null);
    setSuccess(null);
    setDeactivating(true);
    try {
      const ok = await clearSyncDevice();
      if (!ok && isDesktop) {
        throw new Error('Не вдалося зберегти налаштування (команда Tauri недоступна).');
      }
      forgetDeviceActivation();
      setDeviceActive(false);
      setStoreName(null);
      setStoreId(null);
      setSuccess('Device-режим вимкнено. Каса повернулась до звичайного режиму синхронізації (JWT).');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Помилка деактивації');
    } finally {
      setDeactivating(false);
    }
  };

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6">
      {/* ── Заголовок ── */}
      <div className="flex items-center gap-3 mb-6">
        <div className="w-10 h-10 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
          <Radio className="w-5 h-5" />
        </div>
        <div>
          <h1 className="text-xl font-bold text-gray-900 dark:text-white">Мережева каса</h1>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Активація каси як мережевого пристрою (device-режим синхронізації)
          </p>
        </div>
      </div>

      {!isDesktop && (
        <div className="mb-6 rounded-xl border border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20 px-4 py-3 text-sm text-amber-700 dark:text-amber-300">
          <p className="flex items-center gap-2 font-medium">
            <MonitorSmartphone className="w-4 h-4 flex-shrink-0" />
            Повноцінний device-режим (фонові синки) працює у десктоп-версії Torgashka.
          </p>
          <p className="mt-1 text-amber-600 dark:text-amber-400">
            У браузері можна перевірити код активації та адресу сервера — але збереження токена
            в налаштування каси буде недоступне.
          </p>
        </div>
      )}

      <div className="max-w-2xl bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center text-primary-600 dark:text-primary-400">
            {deviceActive ? (
              <CheckCircle2 className="w-4 h-4" />
            ) : (
              <Radio className="w-4 h-4" />
            )}
          </div>
          <div className="flex-1">
            <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
              {deviceActive ? 'Device-режим активовано' : 'Активація пристрою'}
            </h3>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              {deviceActive
                ? 'Фонові push/pull-синки працюють від імені пристрою (без JWT-логіна)'
                : 'Отримайте код активації в адмінці сервера та введіть його тут'}
            </p>
          </div>
        </div>

        <div className="px-6 py-5 space-y-5">
          {error && (
            <div className="flex items-start gap-2 rounded-lg bg-danger-50 dark:bg-danger-900/20 px-4 py-3 text-sm text-danger-600 dark:text-danger-300">
              <XCircle className="w-4 h-4 mt-0.5 flex-shrink-0" />
              <span>{error}</span>
            </div>
          )}
          {success && (
            <div className="flex items-start gap-2 rounded-lg bg-success-50 dark:bg-success-900/20 px-4 py-3 text-sm text-success-600 dark:text-success-300">
              <CheckCircle2 className="w-4 h-4 mt-0.5 flex-shrink-0" />
              <span>{success}</span>
            </div>
          )}

          {deviceActive ? (
            <div className="space-y-5">
              <div className="rounded-lg border border-success-200 dark:border-success-800 bg-success-50/50 dark:bg-success-900/10 px-4 py-3">
                <p className="text-sm font-medium text-success-700 dark:text-success-300">
                  Активовано: {storeName || 'торгова точка'}
                  {storeId ? <span className="text-xs text-gray-400 ml-2">ID: {storeId.slice(0, 8)}…</span> : null}
                </p>
                {serverUrl && (
                  <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                    Сервер: {serverUrl}
                  </p>
                )}
              </div>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                Каса синхронізується як мережевий пристрій: точку визначає сервер з
                device_token, окремий JWT-вхід не потрібен. Для перемикання на іншу точку
                деактивуйте пристрій і активуйте заново новим кодом.
              </p>
              <Button
                type="button"
                variant="danger"
                onClick={handleDeactivate}
                isLoading={deactivating}
                className="flex items-center gap-2"
              >
                {deactivating ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
                Деактивувати пристрій
              </Button>
            </div>
          ) : (
            <div className="space-y-4">
              <Input
                label="Адреса сервера"
                value={serverUrl}
                onChange={(e) => setServerUrl(e.target.value)}
                placeholder="http://192.168.1.10:8000"
                helperText="Без /api/v1 — система додає шлях сама"
                inputClassName="w-full"
              />
              <Input
                label="Код активації"
                value={code}
                onChange={(e) => setCode(e.target.value.toUpperCase())}
                placeholder="XXXXXXX (A-Z, 0-9)"
                helperText="Видається адміністратором у панелі керування сервера"
                inputClassName="w-full"
                maxLength={9}
              />
              <div className="flex items-center justify-end gap-3 pt-2 border-t border-gray-200 dark:border-slate-700">
                <Button
                  type="button"
                  onClick={handleActivate}
                  isLoading={activating}
                  className="flex items-center gap-2"
                >
                  {activating ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
                  Активувати пристрій
                </Button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default DeviceSyncPage;
