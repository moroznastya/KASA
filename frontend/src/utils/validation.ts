/**
 * Validate Ukrainian phone number
 */
export function isValidPhone(phone: string): boolean {
  const phoneRegex = /^(\+?38)?0\d{9}$/;
  return phoneRegex.test(phone.replace(/[\s\-()]/g, ''));
}

/**
 * Validate EDRPOU code
 */
export function isValidEdrpou(edrpou: string): boolean {
  return /^\d{8}$/.test(edrpou);
}

/**
 * Validate email
 */
export function isValidEmail(email: string): boolean {
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
  return emailRegex.test(email);
}

/**
 * Validate barcode (EAN-13)
 */
export function isValidBarcode(barcode: string): boolean {
  return /^\d{8,13}$/.test(barcode);
}

/**
 * Validate price (positive number with up to 2 decimal places)
 */
export function isValidPrice(price: string): boolean {
  return /^\d+(\.\d{1,2})?$/.test(price) && parseFloat(price) >= 0;
}

/**
 * Validate quantity (positive integer)
 */
export function isValidQuantity(qty: string): boolean {
  return /^\d+$/.test(qty) && parseInt(qty) > 0;
}
