import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import api from './api';
import { isTauri } from '@/hooks/useTauri';

/**
 * Вузли мережі магазинів (ADR-0008 §7.1-A3) + приєднання нового вузла до хаба
 * (`provision_node_from_hub`).
 *
 * ЖИВЕ (перевірено в `src-tauri/crates/torgashka-api/src/router_v1.rs:785-794`):
 *   GET  /api/v1/admin/network-nodes                 → list_nodes
 *   POST /api/v1/admin/network-nodes                 → create_node (join-код, TTL)
 *   POST /api/v1/admin/network-nodes/:id/archive     → archive_node
 *   GET  /api/v1/admin/network-events                → журнал мережевих подій
 * Реєстр `network_nodes` лишається живим як сутність ролі вузла-клієнта хаба.
 *
 * ВИДАЛЕНО в E7 (ADR-0008 §7.2 п.6 — `network_nodes.rs:11-15`, `:376-377`):
 * публічний `POST /network-nodes/join`, `PUT /network-nodes/:id/heartbeat`,
 * `POST /admin/network-nodes/:id/force-resync`, а також Rust-команда провіжну
 * старого флоу (фізична реплікація). Вони обслуговували ФІЗИЧНУ копію БД
 * (pg_basebackup + replication-слоти + креденшли реплікації), якої більше
 * немає. Синхронізація вузла з хабом іде прикладним протоколом
 * (`/api/v1/sync/*`), токен вузла живе в налаштуваннях (`sync.hub_token`,
 * ключ `sync.hub_url` — пише команда провіжну).
 */

// ── Реєстр вузлів (живий контур) ─────────────────────────────────────────────

export type NodeRole = 'primary' | 'standby';
export type NodeStatus = 'provisioning' | 'syncing' | 'active' | 'lagging' | 'offline' | 'archived';

/** Вузол зі списку (без секретів). */
export interface NetworkNode {
  id: string;
  store_id: string | null;
  name: string;
  role: NodeRole;
  status: NodeStatus;
  host: string | null;
  app_version: string | null;
  last_seen_at: string | null;
  replication_lag_bytes: number | null;
  db_size_bytes: number | null;
  created_at: string;
}

/** Відповідь на створення вузла: одноразовий join-код. */
export interface NodeCreateResult {
  id: string;
  join_code: string;
  join_code_expires_at: string;
  primary_host_hint: string;
}

/** Список вузлів мережі. */
export async function listNetworkNodes(): Promise<NetworkNode[]> {
  const { data } = await api.get<NetworkNode[]>('/admin/network-nodes');
  return data;
}

/** Створити вузол → отримати одноразовий join-код (TTL 30 хв). */
export async function createNetworkNode(name: string, storeId?: string): Promise<NodeCreateResult> {
  const { data } = await api.post<NodeCreateResult>('/admin/network-nodes', {
    name,
    ...(storeId ? { store_id: storeId } : {}),
  });
  return data;
}

/** Архівувати вузол (вивести з мережі; ідемпотентно). */
export async function archiveNetworkNode(nodeId: string): Promise<void> {
  await api.post(`/admin/network-nodes/${nodeId}/archive`);
}

// ── Провіжн вузла з хаба (Rust-команда, ADR-0008 Контракт 2 / U1) ────────────
//
// Заморожений інтерфейс (узгоджено координатором; реалізує Rust-агент):
//   invoke<ProvisionReport>('provision_node_from_hub',
//                           { hubUrl, token, localDumpPath })
//   invoke<string | null>('pick_dump_file')
//   listen('node-provision:progress', { step, status, message })
//
// Команда ПОВЕРТАЄ звіт навіть на очікуваних помилках (не кидає виняток):
// очікувані класи — у `ProvisionReport.class`, людський текст — у `message`,
// технічний хвіст інструмента (pg_restore тощо) — у `stderrTail`.

/** Клас очікуваної помилки провіжну (`class: null` — не очікувана). */
export type ProvisionErrorClass =
  | 'BadUrl'
  | 'HubUnreachable'
  | 'DownloadFailed'
  | 'RestoreFailed'
  | 'DbUnavailable';

/** Крок провіжну в подіях прогресу / у звіті. */
export type ProvisionStep =
  | 'validate_url'
  | 'hub_reachable'
  | 'download'
  | 'restore'
  | 'configure';

/** Рядок покрокового звіту: `ok=false` + `detail` — реальна причина кроку. */
export interface ProvisionStepResult {
  step: ProvisionStep;
  ok: boolean;
  detail: string;
}

/** Підсумковий звіт команди `provision_node_from_hub` (camelCase). */
export interface ProvisionReport {
  ok: boolean;
  class: ProvisionErrorClass | null;
  /** Людське повідомлення (укр.). */
  message: string;
  /** Хвіст stderr інструмента (pg_restore тощо); `null` — виводу немає. */
  stderrTail: string | null;
  /** НОРМАЛІЗОВАНИЙ URL хаба — фактично записаний у `sync.hub_url`. */
  hubUrl: string | null;
  /** Звідки взято знімок: з хаба або з локального файлу. */
  source: 'hub' | 'file' | null;
  dumpBytes: number | null;
  dumpSha256: string | null;
  steps: ProvisionStepResult[];
}

/** Payload події `node-provision:progress`. */
export interface ProvisionProgressEvent {
  step: ProvisionStep;
  status: 'started' | 'ok' | 'failed';
  message: string;
}

/** Людські назви класів помилок (для бейджа в UI). */
export const PROVISION_ERROR_LABELS: Record<ProvisionErrorClass, string> = {
  BadUrl: 'Некоректна адреса хаба',
  HubUnreachable: 'Хаб недоступний',
  DownloadFailed: 'Не вдалося завантажити знімок',
  RestoreFailed: 'Відновлення БД не вдалося',
  DbUnavailable: 'Локальна БД недоступна',
};

/** Звіт-заглушка для браузерного режиму: провіжн — локальна Rust-операція. */
function desktopOnly(): ProvisionReport {
  return {
    ok: false,
    class: null,
    message:
      'Провіжн вузла виконує локальний Rust-шар (відновлення БД + налаштування хаба) — ' +
      'він доступний лише в десктоп-застосунку Torgashka. У браузері цю операцію виконати неможливо.',
    stderrTail: null,
    hubUrl: null,
    source: null,
    dumpBytes: null,
    dumpSha256: null,
    steps: [],
  };
}

export interface ProvisionNodeInput {
  /** Адреса хаба (буде нормалізована й записана Rust-командою в `sync.hub_url`). */
  hubUrl: string;
  /** Токен вузла, виданий хабом. */
  token: string;
  /** Шлях до локального .dump (офлайн-варіант); `null` — тягнути знімок із хаба. */
  localDumpPath: string | null;
}

/**
 * Провіжн цього комп'ютера як вузла хаба: перевірка URL → доступність хаба →
 * знімок (з хаба або з локального файлу) → відновлення БД → активація вузла.
 *
 * Прогрес — події `node-provision:progress` (`subscribeToProvisionProgress`).
 *
 * @returns звіт команди. Очікувані помилки — у звіті (`ok: false`, `class`,
 *          `message`, `stderrTail`). НЕ кидає на очікуваних помилках; якщо
 *          IPC виклик недоступний (команда не зареєстрована в цій збірці,
 *          браузер) — повертає звіт з `class: null` і текстом помилки.
 */
export async function provisionNodeFromHub(input: ProvisionNodeInput): Promise<ProvisionReport> {
  if (!isTauri()) return desktopOnly();

  const hubUrl = input.hubUrl.trim();
  const token = input.token.trim();
  const localDumpPath = input.localDumpPath?.trim() ? input.localDumpPath.trim() : null;

  if (!hubUrl) {
    return {
      ...desktopOnly(),
      message: 'Не вказано адресу хаба — провіжн не запускався.',
      class: 'BadUrl',
    };
  }

  try {
    return await invoke<ProvisionReport>('provision_node_from_hub', {
      hubUrl,
      token,
      localDumpPath,
    });
  } catch (error) {
    // IPC-помилка (команда не зареєстрована / збій мосту) — це НЕ очікувана
    // помилка провіжну, але й не привід кидати на очі оператору: показуємо
    // сирий текст як є, клас не вигадуємо.
    const raw = error instanceof Error ? error.message : String(error);
    console.warn('[networkNodeService] provision_node_from_hub недоступна:', error);
    return {
      ok: false,
      class: null,
      message: `Команда провіжну недоступна в цій збірці: ${raw}`,
      stderrTail: null,
      hubUrl: null,
      source: null,
      dumpBytes: null,
      dumpSha256: null,
      steps: [],
    };
  }
}

/**
 * Нативний вибір файлу `.dump` (Rust-команда через `tauri_plugin_dialog`).
 *
 * Викликається через `invoke`, а не `@tauri-apps/plugin-dialog`: npm-пакета
 * плагіна в node_modules немає (свідомо — без нових залежностей).
 *
 * @returns абсолютний шлях до файлу або `null`, якщо вибір скасовано
 *          (у браузері — теж `null`: діалог недоступний).
 */
export async function pickDumpFile(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    const path = await invoke<string | null>('pick_dump_file');
    return path && path.trim() ? path : null;
  } catch (error) {
    console.warn('[networkNodeService] pick_dump_file недоступна:', error);
    return null;
  }
}

/**
 * Підписка на події прогресу провіжну.
 *
 * Використання: зберегти `unlisten` і викликати його на розмонтуванні —
 * інакше слухач лишається живим після виходу зі сторінки.
 *
 * @returns `unlisten` або `null`, якщо події недоступні (браузер / стара збірка).
 */
export async function subscribeToProvisionProgress(
  onEvent: (event: ProvisionProgressEvent) => void,
): Promise<UnlistenFn | null> {
  if (!isTauri()) return null;
  try {
    return await listen<ProvisionProgressEvent>('node-provision:progress', (event) =>
      onEvent(event.payload),
    );
  } catch (error) {
    console.warn('[networkNodeService] подія node-provision:progress недоступна:', error);
    return null;
  }
}

/**
 * Перезапустити застосунок — після успішного провіжну фасад має піднятися з
 * новими налаштуваннями хаба.
 *
 * Rust: `commands/system.rs:148` (зареєстровано в `src/main.rs`-модулі
 * `lib.rs:372` як `commands::system::restart_app`).
 */
export async function restartApp(): Promise<void> {
  if (!isTauri()) {
    throw new Error('Перезапуск застосунку доступний лише в десктоп-збірці');
  }
  await invoke<void>('restart_app');
}
