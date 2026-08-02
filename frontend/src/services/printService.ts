import api from './api';
import type {
  PriceTagRenderRequest,
  PriceTagRenderResponse,
  LabelRenderRequest,
  LabelRenderResponse,
  InvoicePrintRequest,
  InvoicePrintResponse,
} from '@/types/print';

/**
 * API-клієнт для друку цінників та етикеток.
 *
 * Ендпоінти:
 *   POST /api/v1/print/price-tags/render      (модуль print — v1, окрема фаза)
 *   POST /api/v1/print/labels/render          (модуль print — v1, окрема фаза)
 *   POST /api/v2/invoices/{id}/print-items    (модуль invoices — v2 ✅)
 *
 * ⚠️ Патерн per-request baseURL — як у prroService (services/prroService.ts).
 */

// API_ROOT: у DEV лишається відносний шлях (dev-проксі Vite),
// у production (Tauri/desktop) — АБСОЛЮТНИЙ http://localhost:8000,
// щоб запити не йшли на tauri://localhost (SPA-fallback → HTML-рядок).
const API_ROOT = import.meta.env.DEV ? '' : 'http://localhost:8000';
const V2 = { baseURL: `${API_ROOT}/api/v2` } as const;

export const printService = {
  /**
   * Рендер цінників на A4 у вигляді HTML-сітки (звичайний принтер).
   * Модуль print — залишено на v1.
   */
  async renderPriceTags(data: PriceTagRenderRequest): Promise<PriceTagRenderResponse> {
    const res = await api.post<PriceTagRenderResponse>('/print/price-tags/render', data);
    return res.data;
  },

  /**
   * Рендер етикеток на термопринтер (одна за одною).
   * Модуль print — залишено на v1.
   */
  async renderLabels(data: LabelRenderRequest): Promise<LabelRenderResponse> {
    const res = await api.post<LabelRenderResponse>('/print/labels/render', data);
    return res.data;
  },

  /**
   * Рендер цінників/етикеток для товарів з прибуткової накладної.
   * Модуль invoices — v2 ✅ (InvoicePrintResponse сумісний).
   * Використовується зі сторінки перегляду накладної.
   */
  async renderInvoicePrintItems(
    invoiceId: string,
    data: InvoicePrintRequest,
  ): Promise<InvoicePrintResponse> {
    const res = await api.post<InvoicePrintResponse>(
      `/invoices/${invoiceId}/print-items`,
      data,
      V2,
    );
    return res.data;
  },
};
