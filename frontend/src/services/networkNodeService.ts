import api from './api';

/**
 * Управління вузлами мережі (ЕТАП 15-17, plan-network-replication).
 *
 * Сервер: /api/v1/admin/network-nodes (JWT owner) та /api/v1/network-nodes (публічне join).
 * Бекенд: crates/torgashka-api/src/network_nodes.rs у src-tauri.
 *
 * Вузол (standby) — фізичний комп'ютер-копія primary (реплікація PostgreSQL).
 * Секрети (join_code, node_token, replication password) повертаються ОДИН раз.
 */

// ── Типи (відповідають Rust DTO: CreateNodeResponse, JoinResponse, NodeDto) ──

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

/** Запит приєднання нового пристрою до мережі. */
export interface NodeJoinRequest {
  join_code: string;
  node_fingerprint: string;
  requested_name: string;
}

/** Креденшл реплікації для standby (ОДИН раз). */
export interface ReplicationCreds {
  role: string;
  password: string;
  primary_host: string;
  primary_port: number;
  primary_database: string;
  slot_name: string;
}

/** Результат join: токен вузла + креденшл реплікації. */
export interface NodeJoinResult {
  node_id: string;
  node_token: string;
  replication: ReplicationCreds;
}

// ── Admin API (JWT owner) ─────────────────────────────────────────────────────

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

/** Архівувати вузол (вимкнути з мережі). */
export async function archiveNetworkNode(nodeId: string): Promise<void> {
  await api.post(`/admin/network-nodes/${nodeId}/archive`);
}

/** Примусовий ресинк вузла (новий basebackup). */
export async function forceResyncNode(nodeId: string): Promise<void> {
  await api.post(`/admin/network-nodes/${nodeId}/force-resync`);
}

// ── Публічний join (без JWT — новий пристрій) ───────────────────────────────

/**
 * Приєднати цей комп'ютер як вузол мережі.
 * Сервер створює роль реплікації + replication slot і повертає креденшл
 * (їх треба передати Rust-шару standby_provision для pg_basebackup).
 */
export async function joinNetworkNode(req: NodeJoinRequest): Promise<NodeJoinResult> {
  const { data } = await api.post<NodeJoinResult>('/network-nodes/join', req);
  return data;
}

/** Heartbeat вузла (Bearer node_token, ОКРЕМО від JWT). */
export async function heartbeatNode(
  nodeId: string,
  nodeToken: string,
  telemetry: { status?: NodeStatus; lagBytes?: number; dbSizeBytes?: number; appVersion?: string },
): Promise<void> {
  await api.put(
    `/network-nodes/${nodeId}/heartbeat`,
    {
      status: telemetry.status,
      lag_bytes: telemetry.lagBytes,
      db_size_bytes: telemetry.dbSizeBytes,
      app_version: telemetry.appVersion,
    },
    { headers: { Authorization: `Bearer ${nodeToken}` } },
  );
}
