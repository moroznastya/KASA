import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Laptop,
  KeyRound,
  Loader2,
  CheckCircle2,
  AlertTriangle,
  Server,
  ArrowLeft,
  RefreshCw,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import toast from 'react-hot-toast';
import api from '@/services/api';
import { joinNetworkNode, NodeJoinResult } from '@/services/networkNodeService';
import { getDeviceFingerprint, normalizeServerUrl } from '@/services/deviceActivationService';
import { isTauri } from '@/hooks/useTauri';

/**
 * «Приєднати як вузол мережі» — публічний екран (БЕЗ JWT) для нового комп'ютера.
 *
 * Флоу (ЕТАП 17 плану мережі):
 *   1. Власник на primary створює вузол → отримує одноразовий join-код.
 *   2. На новому комп'ютері вводять server_url + join-код + назву.
 *   3. POST /api/v1/network-nodes/join (публічний, rate-limit 5/60с) →
 *      node_id + node_token + replication creds (роль/пароль/slot).
 *   4. Десктоп (Tauri): креденшл → SQLite settings через invoke('set_setting')
 *      (ключі node_node_id, node_node_token, node_replication_*, node_role,
 *      node_server_url — саме їх читає standby_heartbeat::read_standby_settings),
 *      далі автоматично стартує invoke('start_standby_provision') — B1a:
 *      pg_basebackup з primary, [node] mode=standby, фоновий heartbeat.
 *   5. Після успіху — пропозиція invoke('restart_app'): лише після рестарту
 *      фасад піднімається в standby-режимі (/api/v1/local/* на локальній репліці).
 *
 * Браузерний режим (без Tauri): доступна ЛИШЕ реєстрація вузла — креденшл
 * зберігаються у localStorage, Rust-провіжн (pg_basebackup) працює лише в
 * десктоп-збірці.
 *
 * Увага: це standby-вузол (копія БД primary). Цей екран НЕ створює точку/касу —
 * він лише готує фізичну реплікацію. Після синхронізації вхід — як на primary.
 */

type ProvisionPhase = 'registered' | 'saving' | 'provisioning' | 'done' | 'error' | 'browser';

/** Кроки прогресу реплікації (join → basebackup → active). */
const PROVISION_STEPS = [
  { key: 'join', label: 'Приєднання до мережі (join)' },
  { key: 'basebackup', label: 'Копія БД primary (pg_basebackup)' },
  { key: 'standby', label: 'Режим standby + heartbeat' },
] as const;

function errMsg(e: unknown): string {
  if (typeof e === 'string' && e.trim()) return e;
  if (e instanceof Error && e.message) return e.message;
  return 'Невідома помилка';
}

const NodeJoinPage: React.FC = () => {
  const navigate = useNavigate();
  const [serverUrl, setServerUrl] = useState('');
  const [joinCode, setJoinCode] = useState('');
  const [requestedName, setRequestedName] = useState('');
  const [joining, setJoining] = useState(false);
  const [result, setResult] = useState<NodeJoinResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Tauri-десктоп vs браузер: провіжн репліки доступний лише в Tauri.
  const [isDesktop] = useState<boolean>(() => isTauri());

  // Фаза після успішного join (збереження → провіжн → done/error/browser).
  const [phase, setPhase] = useState<ProvisionPhase>('registered');
  const [phaseError, setPhaseError] = useState<string | null>(null);
  const [restarting, setRestarting] = useState(false);
  // Для повторної спроби (провіжн/збереження) тримаємо останній url.
  const [savedUrl, setSavedUrl] = useState('');

  // ── Крок 4a: збереження креденшлів вузла ────────────────────────────────
  // Ключі SQLite settings: node_server_url (БЕЗ подвійного node_), решта —
  // node_node_id, node_node_token, node_replication_*, node_role.
  const persistCredentials = async (res: NodeJoinResult, url: string): Promise<void> => {
    const kv: Record<string, string> = {
      server_url: url, // → node_server_url: адреса primary-API (heartbeat-цикл Rust)
      node_id: res.node_id, // → node_node_id: маркер «join виконано»
      node_token: res.node_token, // → node_node_token
      replication_role: res.replication.role,
      replication_password: res.replication.password,
      replication_host: res.replication.primary_host,
      replication_port: String(res.replication.primary_port),
      replication_database: res.replication.primary_database,
      replication_slot: res.replication.slot_name,
      node_role: 'standby',
    };
    if (isDesktop) {
      for (const [k, v] of Object.entries(kv)) {
        await invoke<void>('set_setting', { key: `node_${k}`, value: v });
      }
    } else {
      for (const [k, v] of Object.entries(kv)) {
        localStorage.setItem(`torgashka_node_${k}`, v);
      }
    }
  };

  // ── Крок 4b: Rust-провіжинінг репліки (B1a) ─────────────────────────────
  const runProvision = async (): Promise<void> => {
    setPhase('provisioning');
    setPhaseError(null);
    try {
      await invoke<void>('start_standby_provision');
      setPhase('done');
      toast.success('Репліку створено');
    } catch (e) {
      setPhaseError(errMsg(e));
      setPhase('error');
      toast.error('Провіжинінг репліки не вдався');
    }
  };

  // Повторна спроба (збереження ідемпотентне — креденшл не втрачаються,
  // повторний join НЕ робимо: join-код одноразовий).
  const retryProvision = async (): Promise<void> => {
    if (!result) return;
    setPhase('saving');
    setPhaseError(null);
    try {
      await persistCredentials(result, savedUrl);
    } catch (e) {
      setPhaseError(`Не вдалося зберегти налаштування вузла: ${errMsg(e)}`);
      setPhase('error');
      return;
    }
    await runProvision();
  };

  const handleJoin = async () => {
    const url = normalizeServerUrl(serverUrl);
    if (!url) {
      toast.error('Вкажіть адресу primary-сервера (напр. http://192.168.1.10:8000)');
      return;
    }
    const code = joinCode.trim();
    if (!code) {
      toast.error('Введіть join-код з адмінки primary');
      return;
    }
    const name = requestedName.trim();
    if (!name) {
      toast.error('Вкажіть назву цього вузла');
      return;
    }
    setJoining(true);
    setError(null);
    try {
      const fingerprint = await getDeviceFingerprint();
      // ⚠️ join МУСИТЬ іти на PRIMARY: тимчасово перемикаємо baseURL axios
      // на введений server_url. Без цього на standby-пристрої запит пішов би
      // на власний локальний фасад 127.0.0.1:8000 (де такого вузла немає).
      const prevBaseUrl = api.defaults.baseURL;
      let res: NodeJoinResult;
      try {
        api.defaults.baseURL = `${url}/api/v1`;
        res = await joinNetworkNode({
          join_code: code,
          node_fingerprint: fingerprint,
          requested_name: name,
        });
      } finally {
        api.defaults.baseURL = prevBaseUrl;
      }
      setResult(res);
      setSavedUrl(url);
      toast.success('Вузол зареєстровано — креденшл реплікації отримано');

      // Збереження креденшлів → автоматичний провіжн (десктоп) або
      // реєстрація без провіжна (браузер — лише localStorage).
      setPhase('saving');
      try {
        await persistCredentials(res, url);
      } catch (e) {
        setPhaseError(`Не вдалося зберегти налаштування вузла: ${errMsg(e)}`);
        setPhase('error');
        return;
      }
      if (isDesktop) {
        await runProvision();
      } else {
        setPhase('browser');
        toast.success('Реєстрація виконана — провіжинінг доступний у десктоп-збірці');
      }
    } catch (e) {
      const msg = errMsg(e);
      setError(msg);
      toast.error(msg);
    } finally {
      setJoining(false);
    }
  };

  // ── Перезапуск застосунку (B1c) — після успішного провіжна ─────────────
  const handleRestart = async () => {
    setRestarting(true);
    try {
      await invoke<void>('restart_app');
      // Успіх: процес перезапускається — наступний код не виконується.
    } catch (e) {
      toast.error(`Не вдалося перезапустити застосунок: ${errMsg(e)}`);
      setRestarting(false);
    }
  };

  /** Стан кроку прогресу залежно від фази. */
  const stepState = (idx: number): 'done' | 'active' | 'pending' | 'error' => {
    if (phase === 'error') return idx === 0 ? 'done' : idx === 1 ? 'error' : 'pending';
    if (phase === 'browser') return idx === 0 ? 'done' : 'pending';
    if (phase === 'done') return 'done';
    // registered | saving | provisioning
    if (idx === 0) return 'done';
    if (idx === 1) return phase === 'provisioning' ? 'active' : 'pending';
    return 'pending';
  };

  return (
    <div className="min-h-screen bg-gradient-to-br from-primary-50 to-blue-100 dark:from-slate-900 dark:to-slate-800 flex items-center justify-center p-4">
      <div className="w-full max-w-md card p-8">
        <button
          onClick={() => navigate('/')}
          className="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-800 mb-6"
        >
          <ArrowLeft className="w-4 h-4" /> Назад
        </button>

        <div className="flex items-center gap-3 mb-6">
          <div className="p-3 rounded-xl bg-primary-50 dark:bg-primary-900/20 text-primary-600">
            <Laptop className="w-7 h-7" />
          </div>
          <div>
            <h1 className="text-xl font-bold">Приєднати як вузол мережі</h1>
            <p className="text-sm text-gray-500">Standby-копія БД primary (реплікація)</p>
          </div>
        </div>

        {result ? (
          <div className="space-y-4">
            {/* Банер реєстрації (join виконано) */}
            <div className="flex items-start gap-2 p-4 rounded-lg bg-success-50 dark:bg-success-900/10 text-success-700">
              <CheckCircle2 className="w-5 h-5 mt-0.5 shrink-0" />
              <div className="text-sm">
                <p className="font-semibold">Вузол «{requestedName}» зареєстровано</p>
                <p className="mt-1 text-xs opacity-80">
                  ID: {result.node_id.slice(0, 8)}…
                  <br />
                  Реплікація: роль <b>{result.replication.role}</b>, слот <b>{result.replication.slot_name}</b>
                  <br />
                  Primary: <b>{result.replication.primary_host}:{result.replication.primary_port}</b>
                </p>
              </div>
            </div>

            {/* Фаза: прогрес збереження/провіжна */}
            {(phase === 'saving' || phase === 'provisioning') && (
              <div className="space-y-3 p-4 rounded-lg border border-primary-200 dark:border-primary-900/40 bg-primary-50/60 dark:bg-primary-900/10">
                <p className="text-sm text-primary-700 dark:text-primary-300 flex items-start gap-2">
                  <Loader2 className="w-4 h-4 animate-spin shrink-0 mt-0.5" />
                  <span>
                    {phase === 'saving'
                      ? 'Збереження налаштувань вузла…'
                      : 'Створення репліки (pg_basebackup)… Це може зайняти кілька хвилин.'}
                  </span>
                </p>
                <ol className="space-y-2">
                  {PROVISION_STEPS.map((step, i) => {
                    const s = stepState(i);
                    return (
                      <li key={step.key} className="flex items-center gap-2 text-sm">
                        {s === 'done' && <CheckCircle2 className="w-4 h-4 text-success-500 shrink-0" />}
                        {s === 'active' && <Loader2 className="w-4 h-4 animate-spin text-primary-500 shrink-0" />}
                        {s === 'pending' && <span className="w-4 h-4 rounded-full border-2 border-gray-300 dark:border-slate-600 shrink-0" />}
                        {s === 'error' && <AlertTriangle className="w-4 h-4 text-danger-500 shrink-0" />}
                        <span className={s === 'pending' ? 'text-gray-400' : 'text-gray-700 dark:text-gray-200'}>
                          {step.label}
                        </span>
                      </li>
                    );
                  })}
                </ol>
              </div>
            )}

            {/* Фаза: успіх → перезапуск */}
            {phase === 'done' && (
              <div className="p-4 rounded-lg border border-success-200 dark:border-success-800 bg-success-50 dark:bg-success-900/10">
                <div className="flex items-start gap-2 text-success-700">
                  <CheckCircle2 className="w-5 h-5 mt-0.5 shrink-0" />
                  <div className="text-sm">
                    <p className="font-semibold">Репліку створено</p>
                    <p className="mt-1 text-xs opacity-90">
                      Копія БД primary синхронізована, режим standby увімкнено, heartbeat запущено.
                      Перезапустіть застосунок, щоб увійти в режим standby (фасад змонтує /api/v1/local/*).
                    </p>
                  </div>
                </div>
                <div className="mt-4 flex flex-col gap-2">
                  <Button className="w-full" onClick={() => void handleRestart()} isLoading={restarting}>
                    {!restarting && <RefreshCw className="w-4 h-4 mr-2" />}
                    Перезапустити зараз
                  </Button>
                  <Button variant="ghost" className="w-full" onClick={() => navigate('/login')}>
                    Пізніше
                  </Button>
                </div>
              </div>
            )}

            {/* Фаза: помилка → текст + повтор (креденшл збережені) */}
            {phase === 'error' && (
              <div className="p-4 rounded-lg border border-danger-200 dark:border-danger-800 bg-danger-50 dark:bg-danger-900/10">
                <div className="flex items-start gap-2 text-danger-700">
                  <AlertTriangle className="w-5 h-5 mt-0.5 shrink-0" />
                  <div className="text-sm min-w-0">
                    <p className="font-semibold">Не вдалося створити репліку</p>
                    <p className="mt-1 text-xs break-words font-mono bg-danger-100/50 dark:bg-slate-900/40 rounded p-2">
                      {phaseError}
                    </p>
                    <p className="mt-2 text-xs opacity-80">
                      Креденшл вузла вже збережено — повторна спроба не зашкодить (join-код повторно не потрібен).
                    </p>
                  </div>
                </div>
                <div className="mt-4 flex flex-col gap-2">
                  <Button className="w-full" onClick={() => void retryProvision()}>
                    Спробувати знову
                  </Button>
                  <Button variant="ghost" className="w-full" onClick={() => navigate('/login')}>
                    До входу
                  </Button>
                </div>
              </div>
            )}

            {/* Фаза: браузер (без Tauri) — лише реєстрація */}
            {phase === 'browser' && (
              <div className="space-y-3 p-4 rounded-lg border border-amber-200 dark:border-amber-900/40 bg-amber-50 dark:bg-amber-900/10 text-amber-800 dark:text-amber-200">
                <div className="flex items-start gap-2 text-sm">
                  <Server className="w-4 h-4 mt-0.5 shrink-0" />
                  <div>
                    <p className="font-semibold">Вузол зареєстровано (браузерний режим)</p>
                    <p className="mt-1 text-xs opacity-90">
                      Реплікацію (pg_basebackup) запускає лише десктоп-збірка Torgashka. У браузері доступна
                      лише реєстрація вузла — креденшл збережено у localStorage.
                    </p>
                    <p className="mt-2 text-xs opacity-80">
                      Відкрийте цей екран у десктоп-застосунку — провіжинінг стартує автоматично після join.
                    </p>
                  </div>
                </div>
                <Button variant="secondary" className="w-full" onClick={() => navigate('/login')}>
                  До входу
                </Button>
              </div>
            )}

            {/* Проміжна фаза registered — fallback (зазвичай миттєво змінюється) */}
            {phase === 'registered' && (
              <div className="space-y-3">
                <div className="flex items-center gap-2 p-3 rounded-lg bg-amber-50 dark:bg-amber-900/10 text-amber-700 text-sm">
                  <Server className="w-4 h-4 shrink-0" />
                  Перший запуск реплікації (pg_basebackup) виконає Rust-шар автоматично.
                  Статус відстежуйте в адмінці primary → «Вузли мережі».
                </div>
                <Button className="w-full" onClick={() => navigate('/login')}>До входу</Button>
              </div>
            )}
          </div>
        ) : (
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium mb-1">Адреса primary-сервера</label>
              <Input
                placeholder="http://192.168.1.10:8000"
                value={serverUrl}
                onChange={(e) => setServerUrl(e.target.value)}
                autoFocus
              />
            </div>
            <div>
              <label className="block text-sm font-medium mb-1">Join-код</label>
              <div className="relative">
                <KeyRound className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
                <Input
                  className="pl-9 font-mono uppercase tracking-widest"
                  placeholder="XXXX-XXXX"
                  value={joinCode}
                  onChange={(e) => setJoinCode(e.target.value.toUpperCase())}
                />
              </div>
            </div>
            <div>
              <label className="block text-sm font-medium mb-1">Назва цього вузла</label>
              <Input
                placeholder="Магазин на Лівому березі"
                value={requestedName}
                onChange={(e) => setRequestedName(e.target.value)}
              />
            </div>

            {error && (
              <div className="flex items-start gap-2 p-3 rounded-lg bg-danger-50 dark:bg-danger-900/10 text-danger-700 text-sm">
                <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
                <span>{error}</span>
              </div>
            )}

            <Button className="w-full" onClick={() => void handleJoin()} disabled={joining}>
              {joining && <Loader2 className="w-4 h-4 mr-2 animate-spin" />}
              Приєднати вузол
            </Button>
          </div>
        )}
      </div>
    </div>
  );
};

export default NodeJoinPage;
