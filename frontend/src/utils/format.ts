/**
 * Format a number as currency (UAH)
 */
export function formatCurrency(amount: string | number): string {
  const num = typeof amount === 'string' ? parseFloat(amount) : amount;
  if (isNaN(num)) return '0,00 ₴';
  return `${num.toLocaleString('uk-UA', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })} ₴`;
}

/**
 * Format a date string to Ukrainian locale
 */
export function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString('uk-UA', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  });
}

/**
 * Format a date string to Ukrainian locale with time
 */
export function formatDateTime(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString('uk-UA', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * Format a date as relative time (today, yesterday, etc.)
 */
export function formatRelativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays === 0) {
    return `Сьогодні ${date.toLocaleTimeString('uk-UA', {
      hour: '2-digit',
      minute: '2-digit',
    })}`;
  }
  if (diffDays === 1) {
    return `Вчора ${date.toLocaleTimeString('uk-UA', {
      hour: '2-digit',
      minute: '2-digit',
    })}`;
  }
  if (diffDays < 7) {
    return `${diffDays} днів тому`;
  }
  return formatDate(dateStr);
}

/**
 * Format unit of measure to Ukrainian
 */
export function formatUnit(unit: string): string {
  const units: Record<string, string> = {
    pcs: 'шт',
    kg: 'кг',
    l: 'л',
    m: 'м',
    box: 'кор',
    pack: 'уп',
  };
  return units[unit] || unit;
}

/**
 * Format document type to Ukrainian
 */
export function formatDocumentType(type: string): string {
  const types: Record<string, string> = {
    invoice: 'Прибуткова накладна',
    transfer: 'Переміщення',
    write_off: 'Списання',
    return_invoice: 'Повернення постачальнику',
  };
  return types[type] || type;
}

/**
 * Format document status to Ukrainian
 */
export function formatDocumentStatus(status: string): string {
  const statuses: Record<string, string> = {
    draft: 'Чернетка',
    confirmed: 'Підтверджено',
    cancelled: 'Скасовано',
  };
  return statuses[status] || status;
}

/**
 * Format payment method to Ukrainian
 */
export function formatPaymentMethod(method: string): string {
  const methods: Record<string, string> = {
    cash: 'Готівка',
    card: 'Картка',
    mixed: 'Змішаний',
    bank_transfer: 'Банківський переказ',
  };
  return methods[method] || method;
}

/**
 * Format VAT rate
 */
export function formatVatRate(rate: number): string {
  if (rate === 0) return '0%';
  return `${rate}%`;
}
