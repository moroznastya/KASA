import React, { useEffect, useRef, useState } from 'react';
import { Radio, CheckCircle2, XCircle, Loader2, MonitorSmartphone, KeyRound, FileCode2, FileUp } from 'lucide-react';
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
import { parseNetworkConfigFile } from '@/services/networkConfigService';
import type { NetworkConfigFile } from '@/types/networkConfig';

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

  // ── Спосіб активації (неактивний стан) ──────
  const [mode, setMode] = useState<'code' | 'file'>('code');
  const [importedConfig, setImportedConfig] = useState<NetworkConfigFile | null>(null);
  const [localName, setLocalName] = useState('');
  const fileInputRef = useRef<HTMLInputElement>(null);

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

  // Локальний імпорт конфіг-файлу мережі (schema_version=1): лише парсинг,
  // HTTP не викликаємо — заповнюємо serverUrl/code для існуючого activateDevice.
  const handleFileSelected = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = ''; // дозволяє повторно обрати той самий файл
    if (!file) return;
    setError(null);
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const cfg = parseNetworkConfigFile(String(reader.result ?? ''));
        setImportedConfig(cfg);
        setServerUrl(normalizeServerUrl(cfg.server_url));
        setCode(cfg.store.activation_code);
        setLocalName(cfg.store.name);
      } catch (err) {
        setImportedConfig(null);
        setError(err instanceof Error ? err.message : 'Не вдалося прочитати конфігурацію');
      }
    };
    reader.onerror = () => {
      setImportedConfig(null);
      setError('Не вдалося прочитати файл');
    };
    reader.readAsText(file);
  };

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
      // Імпорт-режим: дозволяємо локальну назву каси (інакше — серверна).
      const chosenName = mode === 'file' && localName.trim() ? localName.trim() : result.store_name;
      setStoreName(chosenName);
      setStoreId(result.store_id);
      setCode('');
      if (saved) {
        setSuccess(
          mode === 'file'
            ? 'Точку додано в мережу, синхронізацію запущено'
            : `Пристрій активовано: «${result.store_name}». Device-режим увімкнено, фонові синки запущено.`
        );
      } else {
        // Браузер або стара версія без set_setting: сервер активацію підтвердив,
        // але локальний Rust-клієнт токен не отримав — синки не запустяться.
        setSuccess(
          `Сервер підтвердив активацію: «${result.store_name}». Але зберегти токен у налаштування каси не вдалося — повноцінний device-режим працює лише в десктоп-версії Torgashka.`
        );
      }
      // Повертаємось до стандартного способу (код) на випадок повторної активації.
      setMode('code');
      setImportedConfig(null);
      setLocalName('');
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
      setMode('code');
      setImportedConfig(null);
      setLocalName('');
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
            <div className="space-y-5">
              {/* Вибір способу активації */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <button
                  type="button"
                  onClick={() => setMode('code')}
                  className={`text-left p-4 rounded-xl border-2 transition-colors ${
                    mode === 'code'
                      ? 'border-primary-500 bg-primary-50/60 dark:bg-primary-900/10'
                      : 'border-gray-200 dark:border-slate-700 hover:border-gray-300 dark:hover:border-slate-600'
                  }`}
                >
                  <KeyRound
                    className={`w-5 h-5 mb-2 ${mode === 'code' ? 'text-primary-600 dark:text-primary-400' : 'text-gray-400'}`}
                  />
                  <div className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                    Код активації
                  </div>
                  <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                    Введіть код, виданий адміністратором сервера
                  </div>
                </button>
                <button
                  type="button"
                  onClick={() => setMode('file')}
                  className={`text-left p-4 rounded-xl border-2 transition-colors ${
                    mode === 'file'
                      ? 'border-primary-500 bg-primary-50/60 dark:bg-primary-900/10'
                      : 'border-gray-200 dark:border-slate-700 hover:border-gray-300 dark:hover:border-slate-600'
                  }`}
                >
                  <FileCode2
                    className={`w-5 h-5 mb-2 ${mode === 'file' ? 'text-primary-600 dark:text-primary-400' : 'text-gray-400'}`}
                  />
                  <div className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                    Файл конфігурації мережі
                  </div>
                  <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                    Імпортуйте .json, отриманий від власника мережі
                  </div>
                </button>
              </div>

              {mode === 'code' ? (
                /* ── Спосіб 1: ручний код (існуюча форма, без змін) ── */
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
              ) : (
                /* ── Спосіб 2: конфіг-файл мережі (локальний імпорт) ── */
                <div className="space-y-4">
                  <div className="flex flex-col sm:flex-row sm:items-center gap-3 p-4 rounded-xl border border-dashed border-gray-300 dark:border-slate-600 bg-gray-50/60 dark:bg-slate-900/30">
                    <FileUp className="w-5 h-5 text-gray-400 flex-shrink-0" />
                    <div className="flex-1 text-sm text-gray-600 dark:text-gray-300">
                      Конфіг-файл мережі (.json) містить адресу сервера та код активації
                      точки — назва та мережа підставляться автоматично.
                    </div>
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      onClick={() => fileInputRef.current?.click()}
                      icon={<FileUp className="w-3.5 h-3.5" />}
                    >
                      {importedConfig ? 'Замінити файл' : 'Імпортувати конфігурацію'}
                    </Button>
                    <input
                      ref={fileInputRef}
                      type="file"
                      accept=".json,application/json"
                      className="hidden"
                      onChange={handleFileSelected}
                    />
                  </div>

                  {importedConfig && (
                    <div className="space-y-4 p-4 rounded-xl border border-success-200 dark:border-success-800 bg-success-50/40 dark:bg-success-900/10">
                      <div className="text-sm text-gray-700 dark:text-gray-200 space-y-1">
                        <p>
                          <span className="text-xs text-gray-400 dark:text-gray-500">Точка: </span>
                          <b>{importedConfig.store.name}</b>
                        </p>
                        <p>
                          <span className="text-xs text-gray-400 dark:text-gray-500">Мережа: </span>
                          {importedConfig.network_id.slice(0, 8)}…
                        </p>
                      </div>
                      <Input
                        label="Адреса сервера"
                        value={serverUrl}
                        onChange={(e) => setServerUrl(e.target.value)}
                        placeholder="http://192.168.1.10:8000"
                        helperText="З файлу; можна змінити (без /api/v1)"
                        inputClassName="w-full"
                      />
                      <Input
                        label="Назва точки (локальна)"
                        value={localName}
                        onChange={(e) => setLocalName(e.target.value)}
                        placeholder={importedConfig.store.name}
                        helperText="Опційно; за замовчуванням — назва з файлу. Показується локально на цій касі."
                        inputClassName="w-full"
                      />
                      <div className="flex items-center justify-end gap-3 pt-2 border-t border-success-200 dark:border-success-800">
                        <Button
                          type="button"
                          onClick={handleActivate}
                          isLoading={activating}
                          className="flex items-center gap-2"
                        >
                          {activating ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
                          Активувати
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default DeviceSyncPage;
