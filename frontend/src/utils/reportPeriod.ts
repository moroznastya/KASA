/**
 * Спільний селектор періоду звітів (як у ReportsPage):
 * today / week / month / custom → { from, to } у форматі YYYY-MM-DD.
 */

export type ReportPeriod = 'today' | 'week' | 'month' | 'custom';

export const REPORT_PERIOD_LABELS: { value: ReportPeriod; label: string }[] = [
  { value: 'today', label: 'Сьогодні' },
  { value: 'week', label: 'Тиждень' },
  { value: 'month', label: 'Місяць' },
  { value: 'custom', label: 'Період' },
];

export function getReportRange(
  period: ReportPeriod,
  customFrom?: string,
  customTo?: string,
): { from?: string; to?: string } {
  const now = new Date();
  const today = now.toISOString().split('T')[0];
  switch (period) {
    case 'today':
      return { from: today, to: today };
    case 'week': {
      const weekAgo = new Date(now);
      weekAgo.setDate(weekAgo.getDate() - 7);
      return { from: weekAgo.toISOString().split('T')[0], to: today };
    }
    case 'month': {
      const monthAgo = new Date(now);
      monthAgo.setMonth(monthAgo.getMonth() - 1);
      return { from: monthAgo.toISOString().split('T')[0], to: today };
    }
    case 'custom':
      if (customFrom && customTo) return { from: customFrom, to: customTo };
      return {};
    default:
      return {};
  }
}
