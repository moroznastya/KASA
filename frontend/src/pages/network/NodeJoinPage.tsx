import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  ChevronDown,
  FileUp,
  HardDriveDownload,
  KeyRound,
  Laptop,
  Loader2,
  Minus,
  RefreshCw,
  Server,
  X,
} from 'lucide-react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Badge } from '@/components/ui/Badge';
import toast from 'react-hot-toast';
import {
  PROVISION_ERROR_LABELS,
  pickDumpFile,
  provisionNodeFromHub,
  restartApp,
  subscribeToProvisionProgress,
  type ProvisionProgressEvent,
  type ProvisionReport,
  type ProvisionStep,
} from '@/services/networkNodeService';
import { normalizeServerUrl } from '@/services/deviceActivationService';
import { isTauri } from '@/hooks/useTauri';
import { formatBytes } from '@/utils/format';

/**
 * «Приєднати як вузол мережі» (ADR-0008, Контракт 2 «Перший запуск каси
 * отримує актуальну БД»).
 *
 * Флоу (усе виконує ЛОКАЛЬНИЙ Rust-шар, не браузер і не HTTP-API сторінки):
 *   1. Хаб видає токен вузла (join-код створюється в адмінці хаба).
 *   2. Оператор вводить адресу хаба + токен; опційно вибирає локальний .dump
 *      (офлайн-варіант: знімок привозять файлом, хаба в мережі немає).
 *   3. `invoke('provision_node_from_hub', { hubUrl, token, localDumpPath })`:
 *      перевірка URL → доступність хаба → знімок (з хаба або з файлу) →
 *      відновлення БД → активація вузла (запис `sync.hub_url`/токена).
 *   4. Прогрес — реальні події `node-provision:progress` (без таймерів).
 *   5. Після `ok === true` — перезапуск застосунку (`invoke('restart_app')`),
 *      щоб фасад піднявся з налаштуваннями хаба.
 *
 * ⚠️ Старого флоу більше не існує (ADR-0008 §7.2 п.6, E7): публічний join,
 * node-heartbeat, force-resync і фізична реплікація БД видалені — цей екран
 * їх НЕ викликає (`network_nodes.rs:11-15`, `router_v1.rs:785-794`).
 *
 * Браузерний режим: провіжн неможливий (це локальні операції з БД і файлами) —
 * екран показує банер і повертає зрозуміле повідомлення замість падіння.
 */

/** Стан кроку в UI. */
type UiStepStatus = 'pending' | 'active' | 'ok' | 'failed' | 'skipped';

/** UI-крок: 4 кроки UI, що покривають 5 кроків подій Rust. */
interface UiStep {
  key: string;
  label: string;
  /** Підпис, коли знімок береться з локального файлу (офлайн-режим). */
  offlineLabel?: string;
  /** Кроки подій, які входять у цей UI-крок. */
  eventSteps: ProvisionStep[];
}

const UI_STEPS: UiStep[] = [
  {
    key: 'connect',
    label: 'Перевірка хаба',
    eventSteps: ['validate_url', 'hub_reachable'],
  },
  {
    key: 'transfer',
    label: 'Завантаження знімка',
    offlineLabel: 'Читання файлу',
    eventSteps: ['download'],
  },
  { key: 'restore', label: 'Відновлення БД', eventSteps: ['restore'] },
  { key: 'configure', label: 'Активація вузла', eventSteps: ['configure'] },
];

interface StepFact {
  status: 'started' | 'ok' | 'failed';
  message: string;
}

type Facts = Partial<Record<ProvisionStep, StepFact>>;

/** Факти кроків зі ЗВІТУ команди — авторитетніше за події (підсумок). */
function factsFromReport(report: ProvisionReport): Facts {
  const facts: Facts = {};
  for (const step of report.steps) {
    facts[step.step] = { status: step.ok ? 'ok' : 'failed', message: step.detail };
  }
  return facts;
}

/** Стан UI-кроку з фактів: без «успішно наперед» — лише те, що реально прийшло. */
function resolveStatus(step: UiStep, facts: Facts, offline: boolean): UiStepStatus {
  const own = step.eventSteps
    .map((key) => facts[key])
    .filter((fact): fact is StepFact => fact !== undefined);
  if (own.some((fact) => fact.status === 'failed')) return 'failed';
  if (own.length === step.eventSteps.length && own.every((fact) => fact.status === 'ok')) {
    return 'ok';
  }
  if (own.some((fact) => fact.status === 'started')) return 'active';
  // Офлайн з файлу: інтернет-кроки не виконуються — це «не потрібно», не «чекає».
  if (offline && step.key === 'connect' && own.length === 0) return 'skipped';
  return 'pending';
}

function stepMessages(step: UiStep, facts: Facts): string {
  return step.eventSteps
    .map((key) => facts[key]?.message)
    .filter((msg): msg is string => Boolean(msg && msg.trim()))
    .join(' · ');
}

const StepIcon: React.FC<{ status: UiStepStatus }> = ({ status }) => {
  if (status === 'ok') return <CheckCircle2 className="w-4 h-4 text-success-500 shrink-0" />;
  if (status === 'active') {
    return <Loader2 className="w-4 h-4 animate-spin text-primary-500 shrink-0" />;
  }
  if (status === 'failed') return <AlertTriangle className="w-4 h-4 text-danger-500 shrink-0" />;
  if (status === 'skipped') return <Minus className="w-4 h-4 text-gray-400 shrink-0" />;
  return <span className="w-4 h-4 rounded-full border-2 border-gray-300 dark:border-slate-600 shrink-0" />;
};

const NodeJoinPage: React.FC = () => {
  const navigate = useNavigate();

  const [isDesktop] = useState<boolean>(() => isTauri());

  // ── Форма ────────────────────────────────────────────────────────────────
  const [hubUrl, setHubUrl] = useState('');
  const [token, setToken] = useState('');
  const [fileMode, setFileMode] = useState(false);
  const [dumpPath, setDumpPath] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);

  // ── Виконання ────────────────────────────────────────────────────────────
  const [running, setRunning] = useState(false);
  const [started, setStarted] = useState(false);
  const [facts, setFacts] = useState<Facts>({});
  const [report, setReport] = useState<ProvisionReport | null>(null);
  const [restarting, setRestarting] = useState(false);

  const offline = fileMode;
  const localDumpPath = offline && dumpPath ? dumpPath : null;

  // ── Реальний прогрес: події Rust (обов'язковий unlisten на розмонтуванні) ──
  useEffect(() => {
    if (!isDesktop) return;
    let unlisten: UnlistenFn | null = null;
    let disposed = false;

    void (async () => {
      const un = await subscribeToProvisionProgress((event: ProvisionProgressEvent) => {
        setFacts((prev) => ({
          ...prev,
          [event.step]: { status: event.status, message: event.message },
        }));
      });
      if (disposed) {
        un?.();
        return;
      }
      unlisten = un;
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isDesktop]);

  const handlePickFile = useCallback(async () => {
    setPicking(true);
    try {
      const path = await pickDumpFile();
      if (path) {
        setDumpPath(path);
        setFileMode(true);
      }
      // null = скасовано у діалозі (або браузерний режим) — мовчки.
    } finally {
      setPicking(false);
    }
  }, []);

  const handleRun = useCallback(async () => {
    const normalized = normalizeServerUrl(hubUrl);
    if (!normalized) {
      toast.error('Вкажіть адресу хаба (напр. http://192.168.1.10:8000)');
      return;
    }
    if (!token.trim()) {
      toast.error('Введіть токен вузла, виданий хабом');
      return;
    }
    if (offline && !dumpPath) {
      toast.error('Виберіть файл .dump або вимкніть режим «знімок із файлу»');
      return;
    }

    setRunning(true);
    setStarted(true);
    setReport(null);
    setFacts({});

    const result = await provisionNodeFromHub({
      hubUrl: normalized,
      token,
      localDumpPath,
    });

    // Підсумок команди — авторитетніший за події: дошльоповуємо ним факти.
    setFacts((prev) => ({ ...prev, ...factsFromReport(result) }));
    setReport(result);
    setRunning(false);

    if (result.ok) {
      toast.success('Вузол активовано');
    } else {
      toast.error(result.message);
    }
  }, [hubUrl, token, offline, dumpPath, localDumpPath]);

  const handleRestart = useCallback(async () => {
    setRestarting(true);
    try {
      await restartApp();
      // Успіх: процес перезапускається — подальший код не виконується.
    } catch (e) {
      toast.error(`Не досягти перезапуску: ${e instanceof Error ? e.message : String(e)}`);
      setRestarting(false);
    }
  }, []);

  const stepsView = useMemo(
    () =>
      UI_STEPS.map((step) => ({
        step,
        status: started
          ? resolveStatus(step, facts, offline)
          : offline && step.key === 'connect'
            ? ('skipped' as UiStepStatus)
            : ('pending' as UiStepStatus),
        message: stepMessages(step, facts),
      })),
    [facts, offline, started],
  );

  const failedClass = report && !report.ok ? report.class : null;

  return (
    <div className="min-h-screen bg-gradient-to-br from-primary-50 to-blue-100 dark:from-slate-900 dark:to-slate-800 flex items-center justify-center p-4">
      <div className="w-full max-w-xl card p-8">
        <button
          onClick={() => navigate('/')}
          className="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-800 dark:hover:text-gray-200 mb-6"
        >
          <ArrowLeft className="w-4 h-4" /> Назад
        </button>

        <div className="flex items-center gap-3 mb-6">
          <div className="p-3 rounded-xl bg-primary-50 dark:bg-primary-900/20 text-primary-600">
            <Laptop className="w-7 h-7" />
          </div>
          <div>
            <h1 className="text-xl font-bold">Приєднати як вузол мережі</h1>
            <p className="text-sm text-gray-500">Перший запуск: отримати актуальну БД із хаба</p>
          </div>
        </div>

        {!isDesktop && (
          <div className="mb-4 flex items-start gap-2 p-3 rounded-lg bg-amber-50 dark:bg-amber-900/10 text-amber-800 dark:text-amber-200 text-sm">
            <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
            <span>
              Провіжн виконує локальний Rust-шар (відновлення БД, запис налаштувань хаба) —
              у браузері він недоступний. Відкрийте цей екран у десктоп-застосунку Torgashka.
            </span>
          </div>
        )}

        {/* ── Форма ─────────────────────────────────────────────────────── */}
        <div className="space-y-4">
          <Input
            id="hub-url"
            label="Адреса хаба"
            placeholder="http://192.168.1.10:8000"
            value={hubUrl}
            onChange={(e) => setHubUrl(e.target.value)}
            disabled={running}
            icon={<Server className="w-4 h-4" />}
            inputClassName="pl-9"
            autoComplete="off"
          />

          <Input
            id="node-token"
            label="Токен вузла (з хаба)"
            placeholder="токен, виданий адміністратором хаба"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            disabled={running}
            icon={<KeyRound className="w-4 h-4" />}
            inputClassName="pl-9 font-mono"
            autoComplete="off"
          />

          {/* Офлайн-варіант: знімок привезли файлом */}
          <div className="rounded-xl border border-gray-200 dark:border-slate-700 p-4 space-y-3">
            <label className="flex items-start gap-3 cursor-pointer">
              <input
                type="checkbox"
                className="mt-1 w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
                checked={fileMode}
                onChange={(e) => setFileMode(e.target.checked)}
                disabled={running}
              />
              <span className="text-sm">
                <span className="font-medium text-gray-900 dark:text-gray-100">
                  Взяти знімок із файлу (офлайн)
                </span>
                <span className="block text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                  Хаб недосяжний, знімок БД привезли на носії як .dump. Кроки «Перевірка хаба»
                  та «Завантаження» тоді не виконуються.
                </span>
              </span>
            </label>

            {fileMode && (
              <div className="space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void handlePickFile()}
                    isLoading={picking}
                    disabled={running || !isDesktop}
                    icon={!picking ? <FileUp className="w-4 h-4" /> : undefined}
                  >
                    Вибрати файл .dump
                  </Button>
                  {dumpPath && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setDumpPath(null)}
                      disabled={running}
                      icon={<X className="w-4 h-4" />}
                    >
                      Очистити вибір
                    </Button>
                  )}
                </div>
                {dumpPath ? (
                  <p className="text-xs font-mono break-all rounded-lg bg-gray-50 dark:bg-slate-800/60 p-2 text-gray-700 dark:text-gray-200">
                    <HardDriveDownload className="w-3.5 h-3.5 inline mr-1" />
                    {dumpPath}
                  </p>
                ) : (
                  <p className="text-xs text-gray-400">Файл ще не вибрано</p>
                )}
              </div>
            )}
          </div>

          <Button
            className="w-full"
            onClick={() => void handleRun()}
            isLoading={running}
            disabled={running}
          >
            {running ? 'Виконується…' : 'Приєднати вузол'}
          </Button>
        </div>

        {/* ── Прогрес: реальні кроки з подій/звіту ──────────────────────── */}
        {started && (
          <div className="mt-6 space-y-3 p-4 rounded-lg border border-gray-200 dark:border-slate-700">
            <div className="flex items-center justify-between gap-3">
              <p className="text-sm font-medium text-gray-900 dark:text-gray-100">Прогрес провіжну</p>
              {running && <Loader2 className="w-4 h-4 animate-spin text-primary-500" />}
            </div>
            <ol className="space-y-3">
              {stepsView.map(({ step, status, message }) => (
                <li key={step.key} className="flex items-start gap-2 text-sm">
                  <StepIcon status={status} />
                  <div className="min-w-0">
                    <p
                      className={
                        status === 'pending' || status === 'skipped'
                          ? 'text-gray-400'
                          : 'text-gray-700 dark:text-gray-200'
                      }
                    >
                      {offline && step.offlineLabel ? step.offlineLabel : step.label}
                      {status === 'skipped' && (
                        <span className="ml-2 text-xs">не потрібно — знімок із файлу</span>
                      )}
                    </p>
                    {message && (
                      <p
                        className={`text-xs mt-0.5 break-words ${
                          status === 'failed'
                            ? 'text-danger-600 dark:text-danger-400'
                            : 'text-gray-500 dark:text-gray-400'
                        }`}
                      >
                        {message}
                      </p>
                    )}
                  </div>
                </li>
              ))}
            </ol>
          </div>
        )}

        {/* ── Помилка: клас + повідомлення + stderr ─────────────────────── */}
        {report && !report.ok && (
          <div className="mt-6 p-4 rounded-lg border border-danger-200 dark:border-danger-800 bg-danger-50 dark:bg-danger-900/10">
            <div className="flex items-start gap-2 text-danger-700 dark:text-danger-300">
              <AlertTriangle className="w-5 h-5 mt-0.5 shrink-0" />
              <div className="text-sm min-w-0 w-full">
                <div className="flex flex-wrap items-center gap-2">
                  <p className="font-semibold">Провіжн вузла не завершено</p>
                  {failedClass && (
                    <Badge variant="danger">{PROVISION_ERROR_LABELS[failedClass]}</Badge>
                  )}
                </div>
                <p className="mt-1 break-words">{report.message}</p>

                {report.stderrTail && (
                  <details className="mt-3 group">
                    <summary className="cursor-pointer text-xs font-medium inline-flex items-center gap-1">
                      <ChevronDown className="w-3.5 h-3.5 transition-transform group-open:rotate-180" />
                      Технічний вивід інструмента (stderr)
                    </summary>
                    <pre className="mt-2 max-h-64 overflow-auto rounded-lg bg-slate-900 dark:bg-slate-950 text-slate-100 text-[11px] leading-relaxed p-3 font-mono whitespace-pre-wrap break-words">{report.stderrTail}</pre>
                  </details>
                )}
              </div>
            </div>
            <div className="mt-4 flex flex-col gap-2">
              <Button className="w-full" onClick={() => void handleRun()} disabled={running}>
                Спробувати знову
              </Button>
              <Button variant="ghost" className="w-full" onClick={() => navigate('/login')}>
                До входу
              </Button>
            </div>
          </div>
        )}

        {/* ── Успіх: лише факти зі звіту ────────────────────────────────── */}
        {report && report.ok && (
          <div className="mt-6 p-4 rounded-lg border border-success-200 dark:border-success-800 bg-success-50 dark:bg-success-900/10">
            <div className="flex items-start gap-2 text-success-700 dark:text-success-300">
              <CheckCircle2 className="w-5 h-5 mt-0.5 shrink-0" />
              <div className="text-sm min-w-0">
                <p className="font-semibold">Вузол активовано</p>
                <p className="mt-1 break-words">{report.message}</p>
                <dl className="mt-3 space-y-1 text-xs">
                  <div className="flex gap-2">
                    <dt className="text-gray-500 dark:text-gray-400">Джерело знімка:</dt>
                    <dd className="font-medium">
                      {report.source === 'file'
                        ? 'локальний файл'
                        : report.source === 'hub'
                          ? 'хаб'
                          : '—'}
                    </dd>
                  </div>
                  <div className="flex gap-2">
                    <dt className="text-gray-500 dark:text-gray-400">Розмір знімка:</dt>
                    <dd className="font-medium">
                      {report.dumpBytes === null ? '—' : formatBytes(report.dumpBytes)}
                    </dd>
                  </div>
                  <div className="flex gap-2">
                    <dt className="text-gray-500 dark:text-gray-400">Хаб (sync.hub_url):</dt>
                    <dd className="font-mono break-all">{report.hubUrl ?? '—'}</dd>
                  </div>
                  {report.dumpSha256 && (
                    <div className="flex gap-2">
                      <dt className="text-gray-500 dark:text-gray-400">SHA-256 знімка:</dt>
                      <dd className="font-mono break-all">{report.dumpSha256}</dd>
                    </div>
                  )}
                </dl>
                <p className="mt-2 text-xs opacity-90">
                  Перезапустіть застосунок, щоб фасад піднявся з налаштуваннями хаба.
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
      </div>
    </div>
  );
};

export default NodeJoinPage;
