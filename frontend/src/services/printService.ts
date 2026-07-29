import api from './api';
import type {
  PriceTagRenderRequest,
  PriceTagRenderResponse,
  LabelRenderRequest,
  LabelRenderResponse,
} from '@/types/print';

/**
 * API-клієнт для друку цінників та етикеток.
 *
 * Ендпоінти:
 *   POST /api/v1/print/price-tags/render
 *   POST /api/v1/print/labels/render
 */
export const printService = {
  /**
   * Рендер цінників на A4 у вигляді HTML-сітки (звичайний принтер).
   */
  async renderPriceTags(data: PriceTagRenderRequest): Promise<PriceTagRenderResponse> {
    const res = await api.post<PriceTagRenderResponse>('/print/price-tags/render', data);
    return res.data;
  },

  /**
   * Рендер етикеток на термопринтер (одна за одною).
   */
  async renderLabels(data: LabelRenderRequest): Promise<LabelRenderResponse> {
    const res = await api.post<LabelRenderResponse>('/print/labels/render', data);
    return res.data;
  },
};
