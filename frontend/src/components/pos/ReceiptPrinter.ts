/**
 * Генератор HTML-шаблонів для друку чеків
 *
 * Використовується з Tauri (системний друк) або window.print()
 * Форматування для термічних принтерів (58мм / 80мм)
 */

interface ReceiptItem {
  name: string;
  quantity: number;
  price: number;
  total: number;
}

interface ReceiptData {
  /** Номер чека */
  number: string | number;
  /** Дата створення */
  date: string;
  /** Час створення */
  time?: string;
  /** Назва магазину */
  shopName?: string;
  /** Адреса магазину */
  shopAddress?: string;
  /** ІПН/ЄДРПОУ */
  taxId?: string;
  /** ПІБ касира */
  cashier?: string;
  /** Товари */
  items: ReceiptItem[];
  /** Загальна сума */
  total: number;
  /** Сума ПДВ */
  tax?: number;
  /** Сума без ПДВ */
  subtotal?: number;
  /** Валюта */
  currency?: string;
  /** Оплачено */
  paid?: number;
  /** Решта */
  change?: number;
  /** Спосіб оплати */
  paymentMethod?: string;
  /** QR-код (наприклад, link для перевірки) */
  qrCode?: string;
  /** Додатковий текст внизу */
  footer?: string;
}

/**
 * Стилі для термічного принтера (58мм)
 */
const THERMAL_STYLES = `
  @page {
    width: 58mm;
    margin: 0;
    padding: 0;
  }
  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }
  body {
    font-family: 'Courier New', 'Consolas', monospace;
    font-size: 10px;
    line-height: 1.2;
    color: #000;
    width: 58mm;
    padding: 1mm 2mm;
  }
  .header {
    text-align: center;
    margin-bottom: 4px;
  }
  .shop-name {
    font-size: 12px;
    font-weight: bold;
    text-transform: uppercase;
  }
  .shop-address {
    font-size: 9px;
    color: #333;
  }
  .divider {
    border-top: 1px dashed #000;
    margin: 4px 0;
  }
  .receipt-info {
    font-size: 9px;
    margin-bottom: 4px;
  }
  .receipt-info td {
    padding: 1px 0;
  }
  .items-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 9px;
  }
  .items-table th {
    text-align: left;
    border-bottom: 1px solid #000;
    padding: 2px 0;
    font-size: 9px;
  }
  .items-table td {
    padding: 1px 0;
    vertical-align: top;
  }
  .item-name {
    max-width: 30mm;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-qty { text-align: center; }
  .item-price { text-align: right; }
  .item-total { text-align: right; }
  .totals {
    width: 100%;
    margin-top: 4px;
    font-size: 10px;
  }
  .totals td {
    padding: 1px 0;
  }
  .totals .label { text-align: left; }
  .totals .value { text-align: right; }
  .grand-total {
    font-size: 14px;
    font-weight: bold;
  }
  .payment-info {
    font-size: 9px;
    margin-top: 4px;
  }
  .footer {
    text-align: center;
    font-size: 9px;
    margin-top: 6px;
    color: #555;
  }
  .qr-code {
    text-align: center;
    margin: 4px 0;
    font-size: 8px;
  }
  .bold { font-weight: bold; }
  .text-center { text-align: center; }
  .text-right { text-align: right; }
  @media print {
    .print-media-type {
      box-shadow: none !important;
      background: none !important;
      background-color: transparent !important;
      background-image: none !important;
      text-shadow: none !important;
      border-color: #000 !important;
    }
  }
`;

/**
 * Згенерувати HTML для друку чека
 *
 * @example
 * ```tsx
 * const html = generateReceiptHtml({
 *   number: 123,
 *   date: '2026-07-26',
 *   items: [{ name: 'Товар', quantity: 1, price: 100, total: 100 }],
 *   total: 100,
 * });
 * ```
 */
export function generateReceiptHtml(data: ReceiptData): string {
  const {
    number,
    date,
    time,
    shopName = 'Kasa POS',
    shopAddress = '',
    taxId = '',
    cashier = '',
    items,
    total,
    tax = 0,
    subtotal = total - tax,
    currency = '₴',
    paid,
    change,
    paymentMethod = 'Готівка',
    qrCode,
    footer = 'Дякуємо за покупку!',
  } = data;

  const now = time || new Date().toLocaleTimeString('uk-UA', { hour: '2-digit', minute: '2-digit' });
  const dateTime = `${date} ${now}`;

  const itemsRows = items
    .map(
      (item) => `
    <tr>
      <td class="item-name">${escapeHtml(item.name)}</td>
      <td class="item-qty">${formatNumber(item.quantity)}</td>
      <td class="item-price">${formatMoney(item.price)}</td>
      <td class="item-total">${formatMoney(item.total)}</td>
    </tr>`,
    )
    .join('');

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <title>Чек #${number}</title>
  <style>${THERMAL_STYLES}</style>
</head>
<body>
  <div class="header">
    <div class="shop-name">${escapeHtml(shopName)}</div>
    ${shopAddress ? `<div class="shop-address">${escapeHtml(shopAddress)}</div>` : ''}

  </div>

  <div class="divider"></div>

  <table class="receipt-info">
    <tr><td>Чек #${escapeHtml(String(number))}</td><td class="text-right">${escapeHtml(dateTime)}</td></tr>
    ${cashier ? `<tr><td>Касир: ${escapeHtml(cashier)}</td></tr>` : ''}
  </table>

  <div class="divider"></div>

  <table class="items-table">
    <thead>
      <tr>
        <th>Товар</th>
        <th class="text-center">К-сть</th>
        <th class="text-right">Ціна</th>
        <th class="text-right">Сума</th>
      </tr>
    </thead>
    <tbody>
      ${itemsRows}
    </tbody>
  </table>

  <div class="divider"></div>

  <table class="totals">
    <tr>
      <td class="label">Сума без ПДВ:</td>
      <td class="value">${formatMoney(subtotal)}</td>
    </tr>
    <tr>
      <td class="label">ПДВ (20%):</td>
      <td class="value">${formatMoney(tax)}</td>
    </tr>
    <tr class="grand-total">
      <td class="label">До сплати:</td>
      <td class="value">${formatMoney(total)} ${currency}</td>
    </tr>
  </table>

  <div class="divider"></div>

  <table class="payment-info">
    <tr>
      <td>Оплата: ${escapeHtml(paymentMethod)}</td>
      <td class="text-right">${formatMoney(paid ?? total)}</td>
    </tr>
    ${change !== undefined ? `<tr><td>Решта:</td><td class="text-right">${formatMoney(change)}</td></tr>` : ''}
  </table>

  ${qrCode ? `<div class="qr-code">${escapeHtml(qrCode)}</div>` : ''}

  <div class="divider"></div>

  <div class="footer">
    ${escapeHtml(footer)}
    <br/>
    Kasa POS v1.0
  </div>
</body>
</html>`;
}

// ─────────────────────────────────────────────────────────────────────────────
// Допоміжні функції
// ─────────────────────────────────────────────────────────────────────────────

function formatMoney(value: number): string {
  return value.toFixed(2);
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? value.toString() : value.toFixed(3);
}

function escapeHtml(text: string): string {
  const map: Record<string, string> = {
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#039;',
  };
  return text.replace(/[&<>"']/g, (c) => map[c] || c);
}
