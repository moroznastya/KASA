import { AxiosError } from 'axios';
import api from './api';

/**
 * Статус вузла (standby-режим, ADR-0007 §10.2/§11.3).
 *
 * Бекенд: `route_local.rs::local_status` (GET /api/v1/local/status).
 * Шлях саме `/local/status` (без `/api/v1`), бо `services/api.ts` має
 * `baseURL = <host>/api/v1` → фактичний URL `/api/v1/local/status`.
 *
 * Роутер `/api/v1/local/*` монтується ЛИШЕ коли вузол у standby і локальна
 * репліка підключена (`route_local.rs:502`) → на звичайному primary-вузлі
 * маршруту немає (404). Це НЕ помилка: індикатор просто не показується.
 *
 * Запит іде тим самим axios-інстансом, тому автоматично отримує JWT
 * (`Authorization`) і `X-Store-Id` — рівно ті самі шари, що й решта API
 * (`store_middleware` + `auth_middleware`, `route_local.rs:526-533`).
 */

/** `mode` вузла. `route_local.rs:337` повертає лише `standby`. */
export type NodeMode = 'standby' | 'primary';

/**
 * `effective` з `route_local.rs::effective_status(primary_up, pending)`:
 *  • `active`  — primary доступний, черга порожня;
 *  • `lagging` — primary доступний, черга > 0 → **НОРМА** (синк іде);
 *  • `offline` — primary недоступний → черга росте, **деградація**.
 */
export type NodeEffective = 'active' | 'lagging' | 'offline';

/** Відповідь `GET /local/status` (поля — 1:1 з `route_local.rs::local_status`). */
export interface NodeStatus {
  mode: NodeMode;
  degrade_to_local: boolean;
  local_port: number;
  primary_reachable: boolean;
  effective: NodeEffective;
  queue_pending: number;
}

/**
 * Прочитати стан вузла.
 *
 * @returns статус або `null`, якщо вузол не standby / немає локального API.
 * @throws ніколи — будь-яка помилка перетворюється на `null` (read-only
 *         індикатор не має права ламати UI і спамити помилками).
 */
export async function getNodeStatus(): Promise<NodeStatus | null> {
  try {
    const response = await api.get<NodeStatus>('/local/status');
    const data = response.data;
    if (!data || typeof data.effective !== 'string') return null;
    return data;
  } catch (error) {
    const status = (error as AxiosError | undefined)?.response?.status;
    // 4xx = «не застосовно», а не збій: 404 — вузол не standby (маршруту
    // немає), 400 — немає X-Store-Id, 401/403 — контекст точки відсутній.
    // Мовчимо, щоб не шуміти в консолі каси.
    if (status !== undefined && status >= 400 && status < 500) return null;
    // 5xx / мережа (локальний API впав) — діагностика в консоль, без тостів.
    console.warn('[nodeStatus] /local/status недоступний:', error);
    return null;
  }
}
