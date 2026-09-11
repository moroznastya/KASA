/**
 * «Аудит-лог» (Етап 5 адмін-панелі, ТЗ 5.9) — типи відповіді
 * GET /api/v1/admin/audit-log.
 */

export interface AuditLogItem {
  id: string;
  /** created_at (наївний timestamp БД) у форматі YYYY-MM-DDTHH:MM:SS. */
  created_at: string;
  action: string;
  actor_user_id: string | null;
  actor_name: string | null;
  actor_login: string | null;
  entity_type: string | null;
  entity_id: string | null;
  store_id: string | null;
  store_name: string | null;
  payload: Record<string, unknown> | null;
}

export interface AuditLogPage {
  items: AuditLogItem[];
  total: number;
  page: number;
  size: number;
  pages: number;
}

export interface AuditLogFilters {
  from?: string;
  to?: string;
  actor?: string;
  author?: string;
  action?: string;
  store_id?: string;
  page?: number;
  size?: number;
}

/** Типи дій, які сервер записує в audit_log (admin.rs / network.rs). */
export const AUDIT_ACTIONS: { value: string; label: string }[] = [
  { value: 'store_updated', label: 'Точка: оновлення' },
  { value: 'store_archived', label: 'Точка: архівація' },
  { value: 'worker_created', label: 'Працівник: створення' },
  { value: 'activation_code_generated', label: 'Точка: код активації' },
  { value: 'device_blocked', label: 'Каса: блокування' },
  { value: 'device_unblocked', label: 'Каса: розблокування' },
  { value: 'device_archived', label: 'Каса: архівація' },
];

export const AUDIT_ACTION_LABELS: Record<string, string> = Object.fromEntries(
  AUDIT_ACTIONS.map((a) => [a.value, a.label]),
);
