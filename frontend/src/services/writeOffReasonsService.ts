import api from './api';

/** Причина списання з персистентного довідника (GET /write-off-reasons) */
export interface WriteOffReasonItem {
  id: string;
  name: string;
  is_active: boolean;
  created_at: string;
}

interface WriteOffReasonsResponse {
  items: WriteOffReasonItem[];
  total: number;
}

export const writeOffReasonsService = {
  /** Отримує список причин списання */
  async getWriteOffReasons(): Promise<WriteOffReasonItem[]> {
    const response = await api.get<WriteOffReasonsResponse>('/write-off-reasons');
    return response.data.items;
  },

  /** Створює нову причину списання (409, якщо така вже існує) */
  async createWriteOffReason(name: string): Promise<WriteOffReasonItem> {
    const response = await api.post<WriteOffReasonItem>('/write-off-reasons', { name });
    return response.data;
  },
};
