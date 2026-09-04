import api from './api';
import {
  ActivateResult,
  DbSourceCreate,
  DbSourcesList,
  DbSourceUpdate,
  DbSourceView,
  DumpInfo,
  ExportResult,
  ImportBody,
} from '@/types/dbSources';

/**
 * «Джерело даних» (Етап 3 адмін-панелі, ТЗ 2.4/5.8).
 *
 * Rust endpoints (router_v1.rs — /api/v1/admin/db-sources, під require_admin
 * owner|store_manager|admin; поза store_middleware):
 *   GET    /admin/db-sources                 → { active, config_path, sources[] }
 *   POST   /admin/db-sources                 → створити джерело
 *   PUT    /admin/db-sources/:id             → редагування (пароль — опційно)
 *   DELETE /admin/db-sources/:id             → видалити (НЕ активне)
 *   POST   /admin/db-sources/:id/test        → реальний пінг (TCP + SELECT 1)
 *   POST   /admin/db-sources/:id/activate    → test + active у db_sources.toml
 *   POST   /admin/db-sources/export-dump     → pg_dump активної БД (plain .sql)
 *   GET    /admin/db-sources/dumps           → список дампів
 *   POST   /admin/db-sources/import-dump     → psql-імпорт у вибране джерело
 *
 * ⚠️ Активація — stability_first: сервер зберігає active у db_sources.toml і
 * повертає applied_immediately=false («застосується після перезапуску»);
 * гарячого перепідключення пулів немає.
 */

/** Виймає detail з помилки API (як deviceAdminService.extractApiError). */
export function extractDetail(err: unknown): string {
  if (typeof err === 'object' && err !== null) {
    const detail = (err as { response?: { data?: { detail?: string } } }).response?.data?.detail;
    if (detail) return detail;
    const msg = (err as { message?: string }).message;
    if (msg) return msg;
  }
  return 'Невідома помилка';
}

export const dbSourcesService = {
  async list(): Promise<DbSourcesList> {
    const response = await api.get<DbSourcesList>('/admin/db-sources');
    return response.data;
  },

  async create(body: DbSourceCreate): Promise<DbSourceView> {
    const response = await api.post<DbSourceView>('/admin/db-sources', body);
    return response.data;
  },

  async update(id: string, body: DbSourceUpdate): Promise<DbSourceView> {
    const response = await api.put<DbSourceView>(`/admin/db-sources/${id}`, body);
    return response.data;
  },

  async remove(id: string): Promise<void> {
    await api.delete(`/admin/db-sources/${id}`);
  },

  async test(id: string): Promise<{ ok: boolean; latency_ms: number }> {
    const response = await api.post(`/admin/db-sources/${id}/test`);
    return response.data;
  },

  async activate(id: string): Promise<ActivateResult> {
    const response = await api.post<ActivateResult>(`/admin/db-sources/${id}/activate`);
    return response.data;
  },

  async exportDump(): Promise<ExportResult> {
    const response = await api.post<ExportResult>('/admin/db-sources/export-dump', {});
    return response.data;
  },

  async listDumps(): Promise<DumpInfo[]> {
    const response = await api.get<DumpInfo[]>('/admin/db-sources/dumps');
    return response.data;
  },

  async importDump(body: ImportBody): Promise<{ ok: boolean; source_id: string; file: string }> {
    const response = await api.post('/admin/db-sources/import-dump', body);
    return response.data;
  },
};
