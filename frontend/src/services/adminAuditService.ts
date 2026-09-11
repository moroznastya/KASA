import api from './api';
import {
  AuditLogFilters,
  AuditLogPage,
} from '@/types/adminAudit';

/**
 * «Аудит-лог» (Етап 5 адмін-панелі, ТЗ 5.9).
 *
 * Серверний endpoint (Rust, router_v1.rs — /api/v1/admin/audit-log поза
 * store_middleware; auth_middleware + require_admin(owner|store_manager|admin)):
 *   GET /admin/audit-log?from=&to=&actor=&author=&action=&store_id=&page=&size=
 *
 * Тільки перегляд. from/to — YYYY-MM-DD (created_at::date, наївні дати БД).
 * author — підрядок по імені користувача (ILIKE users.name).
 */

function qs(f: AuditLogFilters): string {
  const q = new URLSearchParams();
  if (f.from) q.set('from', f.from);
  if (f.to) q.set('to', f.to);
  if (f.actor) q.set('actor', f.actor);
  if (f.author) q.set('author', f.author);
  if (f.action) q.set('action', f.action);
  if (f.store_id) q.set('store_id', f.store_id);
  if (f.page) q.set('page', String(f.page));
  if (f.size) q.set('size', String(f.size));
  const s = q.toString();
  return s ? `?${s}` : '';
}

export const adminAuditService = {
  async list(filters: AuditLogFilters = {}): Promise<AuditLogPage> {
    const response = await api.get<AuditLogPage>(`/admin/audit-log${qs(filters)}`);
    return response.data;
  },
};
