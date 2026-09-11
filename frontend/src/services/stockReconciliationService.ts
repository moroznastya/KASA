import { AxiosError } from 'axios';
import api from './api';

/**
 * Звірка залишків документа після повернення репліки (ADR-0007 §10.3, AT-15).
 *
 * Бекенд: `route_local.rs::local_stock_reconciliation` (GET
 * `/api/v1/local/stock-reconciliation?invoice_id=<uuid>`), рядок 519.
 * Шлях тут — `/local/stock-reconciliation` (без `/api/v1`), бо
 * `services/api.ts` має `baseURL = <host>/api/v1`.
 *
 * Семантика (§10.3, `route_local.rs:596-600`):
 *   • «локальний» залишок = `stock` SQLite каси → **оптимістична ОЦІНКА**,
 *     не істина (`local_is_estimate: true`);
 *   • «авторитетний» = `stock.quantity` репліки PostgreSQL (`replica_pg`);
 *   • `delta` = local − authoritative;
 *   • ВИРІВНЮВАННЯ робить НАЯВНИЙ механізм — інвентаризація; цей ендпоінт
 *     ЛИШЕ ЧИТАЄ (`infrastructure/src/offline/reconciliation.rs:1-12`).
 *
 * `invoice_id` — `client_uuid` локального агрегата каси (той самий id, що
 * повертає створення накладної: `outbox_invoices.rs:120-129`).
 * Маршрут `/local/*` існує лише на standby-вузлі (`route_local.rs:502`) →
 * поза standby 404 (не помилка, а «не застосовно»).
 */

/** `kind` документа в черзі каси (`reconciliation.rs:46-47`). */
export type StockReconciliationKind = 'invoice' | 'return_invoice';

/** Позиція звірки (`route_local.rs:579-586`) — поля 1:1 з бекендом. */
export interface StockReconciliationItem {
  product_id: string;
  /** Ім'я з локального каталогу; невідомий товар → сам `product_id`. */
  name: string;
  /** Локальний (оптимістичний) залишок точки — ОЦІНКА каси. */
  local_qty: number;
  /** Залишок репліки PG; `null` — товар не є UUID primary. */
  authoritative_qty: number | null;
  /** `local_qty − authoritative_qty`; `null` разом з `authoritative_qty`. */
  delta: number | null;
  /** `delta === 0`; для `authoritative_qty === null` → `false`. */
  matches: boolean;
}

/** Відповідь `GET /local/stock-reconciliation` (`route_local.rs:589-607`). */
export interface StockReconciliation {
  invoice_id: string;
  kind: StockReconciliationKind;
  /** Номер документа з payload; `null` у повернень (немає колонки number). */
  number: string | null;
  store_id: string | null;
  local_source: 'sqlite_cash_queue';
  authoritative_source: 'replica_pg';
  /** §10.3: локальне число завжди позначене оцінкою. */
  local_is_estimate: true;
  /** Текст пояснення від бекенду (не дублюємо своїм формулюванням). */
  note: string;
  items: StockReconciliationItem[];
  summary: { total: number; matching: number; mismatching: number };
}

/**
 * Прочитати звірку залишків документа.
 *
 * @returns дані звірки, або `null`, якщо звірка не застосовна: вузол не
 *          standby (404), документ відсутній у локальній черзі ЦІЄЇ каси
 *          (404 — `route_local.rs:539-543`), немає контексту точки (400),
 *          локальний API мовчить.
 * @throws ніколи — панель звірки не має права ламати сторінку документа.
 */
export async function getStockReconciliation(
  invoiceId: string
): Promise<StockReconciliation | null> {
  const id = invoiceId.trim();
  if (!id) return null;
  try {
    const response = await api.get<StockReconciliation>('/local/stock-reconciliation', {
      params: { invoice_id: id },
    });
    const data = response.data;
    if (!data || !Array.isArray(data.items)) return null;
    return data;
  } catch (error) {
    const status = (error as AxiosError | undefined)?.response?.status;
    // 4xx = «не застосовно» (404 немає документа в черзі / primary-вузол,
    // 400 немає X-Store-Id, 401/403 немає контексту точки) — тихо.
    if (status !== undefined && status >= 400 && status < 500) return null;
    console.warn('[stockReconciliation] ендпоінт недоступний:', error);
    return null;
  }
}
