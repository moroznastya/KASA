import { Category, VatRate, UnitOfMeasure } from '@/types/product';
import { SelectOption } from '@/components/ui/Select';

/**
 * Спільні довідники та розрахунки картки товару.
 * Єдине джерело істини для ProductFormPage та модалок створення товару
 * (інвентаризація, накладна). НЕ дублювати ці константи у сторінках.
 */

/** Стан форми товару — спільний для всіх місць редагування. */
export interface ProductFormState {
  title: string;
  barcode: string;
  sku: string;
  uktzed: string;
  price: number;
  cost_price: number | null;
  markup: number | null;
  stock: number;
  recommended_qty: number;
  category_id: string | null;
  supplier_id: string | null;
  tax_rate: VatRate;
  unit: UnitOfMeasure;
  is_weight: boolean;
  scan_excise: boolean;
}

export const EMPTY_PRODUCT_FORM: ProductFormState = {
  title: '',
  barcode: '',
  sku: '',
  uktzed: '',
  price: 0,
  cost_price: null,
  markup: null,
  stock: 0,
  recommended_qty: 0,
  category_id: null,
  supplier_id: null,
  tax_rate: 0,
  unit: 'pcs',
  is_weight: false,
  scan_excise: false,
};

export const TAX_RATE_OPTIONS: SelectOption[] = [
  { value: 0, label: '0%' },
  { value: 5, label: '5%' },
  { value: 7, label: '7%' },
  { value: 20, label: '20%' },
];

export const UNIT_OPTIONS: SelectOption[] = [
  { value: 'pcs', label: 'шт' },
  { value: 'kg', label: 'кг' },
  { value: 'l', label: 'л' },
  { value: 'm', label: 'м' },
  { value: 'box', label: 'кор' },
  { value: 'pack', label: 'уп' },
];

/**
 * Розраховує ціну на основі собівартості та націнки.
 * Формула: Ціна = Собівартість × (1 + Націнка/100), округлення до гривні.
 */
export function calcPriceFromCostAndMarkup(
  cost: number | null,
  markup: number | null,
): number {
  if (cost === null || cost <= 0 || markup === null || markup <= 0) return 0;
  return Math.round(cost * (1 + markup / 100));
}

/**
 * Розраховує націнку на основі собівартості та ціни.
 * Формула: Націнка = (Ціна / Собівартість - 1) × 100, округлення до сотих.
 */
export function calcMarkupFromCostAndPrice(
  cost: number | null,
  price: number,
): number | null {
  if (cost === null || cost <= 0 || price <= 0) return null;
  const markup = (price / cost - 1) * 100;
  return Math.round(markup * 100) / 100;
}

/**
 * Рекурсивно будує список SelectOption для випадаючого списку категорій.
 * Основні категорії — жирним шрифтом, не вибираються (disabled).
 * Підкатегорії — з відступом, вибираються.
 */
export function buildCategoryOptions(
  categories: Category[],
  depth: number = 0,
): SelectOption[] {
  const options: SelectOption[] = [];

  for (const cat of categories) {
    const hasChildren = Array.isArray(cat.children) && cat.children.length > 0;

    if (hasChildren) {
      options.push({
        value: cat.id,
        label: `${'  '.repeat(depth)}▶ ${cat.name}`,
        disabled: true,
      });
      options.push(...buildCategoryOptions(cat.children ?? [], depth + 1));
    } else {
      options.push({
        value: cat.id,
        label: `${'  '.repeat(depth)}└── ${cat.name}`,
        disabled: false,
      });
    }
  }

  return options;
}

/** Опції категорій разом із порожнім варіантом «Без категорії». */
export function buildCategorySelectOptions(
  categoryTree: Category[] | undefined,
): SelectOption[] {
  return [
    { value: '', label: 'Без категорії' },
    ...(Array.isArray(categoryTree) ? buildCategoryOptions(categoryTree) : []),
  ];
}

/** Опції постачальників разом із порожнім варіантом «Без постачальника». */
export function buildSupplierSelectOptions(
  suppliers: { id: string | number; name: string }[] | undefined,
): SelectOption[] {
  return [
    { value: '', label: 'Без постачальника' },
    ...(suppliers?.map((sup) => ({ value: String(sup.id), label: sup.name })) || []),
  ];
}
