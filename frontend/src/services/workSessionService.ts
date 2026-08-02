import api from './api';

export interface WorkSession {
  id: string;
  user_id: string;
  login_time: string;
  logout_time: string | null;
  duration_hours: number | null;
  is_active: boolean;
}

export interface WorkSessionDetail extends WorkSession {
  user_name?: string;
}

export interface UserHoursSummary {
  user_id: string;
  user_name: string;
  total_hours: number;
  hourly_rate: number | null;
  salary: number | null;
}

export interface WorkSessionReport {
  month: number;
  year: number;
  items: UserHoursSummary[];
}

export interface MySessionsResponse {
  sessions: WorkSession[];
  total_hours: number;
  hourly_rate: number | null;
}

/** Відповідь ендпоінта GET /work-sessions/user/{user_id} (адмін) */
export interface UserSessionsResponse {
  user_id: string;
  user_name: string;
  total_hours: number;
  sessions: WorkSession[];
}

export const workSessionService = {
  /** Отримати мої сесії (для касира) */
  getMySessions: async (month?: number, year?: number) => {
    const params: Record<string, number> = {};
    if (month) params.month = month;
    if (year) params.year = year;
    const res = await api.get<MySessionsResponse>('/work-sessions/my', { params });
    return res.data;
  },

  /** Отримати звіт по всіх (для адміна) */
  getReport: async (month: number, year: number) => {
    const res = await api.get<WorkSessionReport>('/work-sessions/report', {
      params: { month, year },
    });
    return res.data;
  },

  /** Отримати сесії конкретного користувача за місяць (адмін) */
  getUserSessions: async (userId: string, month: number, year: number) => {
    const res = await api.get<UserSessionsResponse>(`/work-sessions/user/${userId}`, {
      params: { month, year },
    });
    return res.data;
  },

  /** Встановити ставку (адмін) */
  setHourlyRate: async (userId: string, hourlyRate: number) => {
    const res = await api.put(`/users/${userId}/hourly-rate`, { hourly_rate: hourlyRate });
    return res.data;
  },
};
