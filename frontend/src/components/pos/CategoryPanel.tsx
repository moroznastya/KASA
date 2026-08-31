import React, { useState, useEffect, useCallback, useMemo } from 'react';
import {Package, Loader2, Star, StarOff, ArrowLeft, ChevronRight, Grid3X3, Heart, Tag, Layers} from 'lucide-react';
import { categoryService } from '@/services/categoryService';
import { productService } from '@/services/productService';
import { Product } from '@/types/product';
import { formatCurrency, formatUnit } from '@/utils/format';

// Ключ для збереження обраних категорій в localStorage
const FAVORITE_CATEGORIES_KEY = 'pos_favorite_categories';

interface CategoryPanelProps {
  onProductSelect: (product: Product) => void;
}

interface CategoryNode {
  id: string;
  name: string;
  parent_id: string | null;
  children: CategoryNode[];
}

/** Палітра кольорів для плиток категорій — більш насичені та сучасні */
const CATEGORY_COLORS = [
  { bg: 'bg-gradient-to-br from-blue-50 to-blue-100 dark:from-blue-950/40 dark:to-blue-900/30', border: 'border-blue-300 dark:border-blue-700', text: 'text-blue-800 dark:text-blue-200', icon: 'text-blue-500', badge: 'bg-blue-500/10 text-blue-600 dark:text-blue-400' },
  { bg: 'bg-gradient-to-br from-emerald-50 to-emerald-100 dark:from-emerald-950/40 dark:to-emerald-900/30', border: 'border-emerald-300 dark:border-emerald-700', text: 'text-emerald-800 dark:text-emerald-200', icon: 'text-emerald-500', badge: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400' },
  { bg: 'bg-gradient-to-br from-amber-50 to-amber-100 dark:from-amber-950/40 dark:to-amber-900/30', border: 'border-amber-300 dark:border-amber-700', text: 'text-amber-800 dark:text-amber-200', icon: 'text-amber-500', badge: 'bg-amber-500/10 text-amber-600 dark:text-amber-400' },
  { bg: 'bg-gradient-to-br from-rose-50 to-rose-100 dark:from-rose-950/40 dark:to-rose-900/30', border: 'border-rose-300 dark:border-rose-700', text: 'text-rose-800 dark:text-rose-200', icon: 'text-rose-500', badge: 'bg-rose-500/10 text-rose-600 dark:text-rose-400' },
  { bg: 'bg-gradient-to-br from-violet-50 to-violet-100 dark:from-violet-950/40 dark:to-violet-900/30', border: 'border-violet-300 dark:border-violet-700', text: 'text-violet-800 dark:text-violet-200', icon: 'text-violet-500', badge: 'bg-violet-500/10 text-violet-600 dark:text-violet-400' },
  { bg: 'bg-gradient-to-br from-cyan-50 to-cyan-100 dark:from-cyan-950/40 dark:to-cyan-900/30', border: 'border-cyan-300 dark:border-cyan-700', text: 'text-cyan-800 dark:text-cyan-200', icon: 'text-cyan-500', badge: 'bg-cyan-500/10 text-cyan-600 dark:text-cyan-400' },
  { bg: 'bg-gradient-to-br from-orange-50 to-orange-100 dark:from-orange-950/40 dark:to-orange-900/30', border: 'border-orange-300 dark:border-orange-700', text: 'text-orange-800 dark:text-orange-200', icon: 'text-orange-500', badge: 'bg-orange-500/10 text-orange-600 dark:text-orange-400' },
  { bg: 'bg-gradient-to-br from-teal-50 to-teal-100 dark:from-teal-950/40 dark:to-teal-900/30', border: 'border-teal-300 dark:border-teal-700', text: 'text-teal-800 dark:text-teal-200', icon: 'text-teal-500', badge: 'bg-teal-500/10 text-teal-600 dark:text-teal-400' },
  { bg: 'bg-gradient-to-br from-pink-50 to-pink-100 dark:from-pink-950/40 dark:to-pink-900/30', border: 'border-pink-300 dark:border-pink-700', text: 'text-pink-800 dark:text-pink-200', icon: 'text-pink-500', badge: 'bg-pink-500/10 text-pink-600 dark:text-pink-400' },
  { bg: 'bg-gradient-to-br from-indigo-50 to-indigo-100 dark:from-indigo-950/40 dark:to-indigo-900/30', border: 'border-indigo-300 dark:border-indigo-700', text: 'text-indigo-800 dark:text-indigo-200', icon: 'text-indigo-500', badge: 'bg-indigo-500/10 text-indigo-600 dark:text-indigo-400' },
  { bg: 'bg-gradient-to-br from-lime-50 to-lime-100 dark:from-lime-950/40 dark:to-lime-900/30', border: 'border-lime-300 dark:border-lime-700', text: 'text-lime-800 dark:text-lime-200', icon: 'text-lime-500', badge: 'bg-lime-500/10 text-lime-600 dark:text-lime-400' },
  { bg: 'bg-gradient-to-br from-sky-50 to-sky-100 dark:from-sky-950/40 dark:to-sky-900/30', border: 'border-sky-300 dark:border-sky-700', text: 'text-sky-800 dark:text-sky-200', icon: 'text-sky-500', badge: 'bg-sky-500/10 text-sky-600 dark:text-sky-400' },
];

/** Отримати колір за індексом */
const getColor = (index: number) => CATEGORY_COLORS[index % CATEGORY_COLORS.length];

/** Отримати улюблені категорії з localStorage */
const getFavoriteIds = (): Set<string> => {
  try {
    const stored = localStorage.getItem(FAVORITE_CATEGORIES_KEY);
    return stored ? new Set(JSON.parse(stored)) : new Set();
  } catch {
    return new Set();
  }
};

/** Зберегти улюблені категорії */
const saveFavoriteIds = (ids: Set<string>) => {
  localStorage.setItem(FAVORITE_CATEGORIES_KEY, JSON.stringify([...ids]));
};

/** Зібрати всі категорії з дерева в плоский список (рекурсивно) */
const flattenCategories = (nodes: CategoryNode[]): CategoryNode[] => {
  const result: CategoryNode[] = [];
  for (const node of nodes) {
    result.push(node);
    if (Array.isArray(node.children) && node.children.length > 0) {
      result.push(...flattenCategories(node.children));
    }
  }
  return result;
};

/** Отримати емодзі для категорії на основі назви */
const getCategoryEmoji = (name: string): string => {
  const lower = name.toLowerCase();
  if (lower.includes('хліб') || lower.includes('булк') || lower.includes('випічк') || lower.includes('батон')) return '🍞';
  if (lower.includes('молок') || lower.includes('сир') || lower.includes('йогурт') || lower.includes('кефір') || lower.includes('сметан')) return '🥛';
  if (lower.includes('м\'яс') || lower.includes('ковбас') || lower.includes('сосис') || lower.includes('фарш') || lower.includes('сало')) return '🥩';
  if (lower.includes('овоч') || lower.includes('помідор') || lower.includes('огірок') || lower.includes('салат') || lower.includes('капуст')) return '🥦';
  if (lower.includes('фрукт') || lower.includes('яблук') || lower.includes('банан') || lower.includes('апельсин') || lower.includes('виноград')) return '🍎';
  if (lower.includes('напо') || lower.includes('вода') || lower.includes('сік') || lower.includes('лимонад') || lower.includes('газов')) return '🥤';
  if (lower.includes('алког') || lower.includes('пив') || lower.includes('вино') || lower.includes('горілк') || lower.includes('коньяк')) return '🍷';
  if (lower.includes('солод') || lower.includes('цукер') || lower.includes('шоколад') || lower.includes('печив') || lower.includes('вафл')) return '🍬';
  if (lower.includes('консер') || lower.includes('риб') || lower.includes('паштет') || lower.includes('тушк')) return '🥫';
  if (lower.includes('круп') || lower.includes('рис') || lower.includes('гречк') || lower.includes('макар') || lower.includes('борош')) return '🌾';
  if (lower.includes('соус') || lower.includes('кетчуп') || lower.includes('майонез') || lower.includes('олі') || lower.includes('оцет')) return '🧂';
  if (lower.includes('замороз') || lower.includes('морозив') || lower.includes('пельмен') || lower.includes('вареник')) return '❄️';
  if (lower.includes('чай') || lower.includes('кав') || lower.includes('какао')) return '☕';
  if (lower.includes('побут') || lower.includes('миюч') || lower.includes('чист') || lower.includes('порош') || lower.includes('засіб')) return '🧹';
  if (lower.includes('космет') || lower.includes('шампун') || lower.includes('мил') || lower.includes('крем') || lower.includes('зуб')) return '🧴';
  if (lower.includes('дитяч') || lower.includes('пампер') || lower.includes('підгуз') || lower.includes('іграшк')) return '🍼';
  if (lower.includes('морозив')) return '🍦';
  if (lower.includes('чипс') || lower.includes('сухар') || lower.includes('горішк') || lower.includes('насін')) return '🥨';
  return '📦';
};

export const CategoryPanel: React.FC<CategoryPanelProps> = ({ onProductSelect }) => {
  const [categories, setCategories] = useState<CategoryNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [favoriteIds, setFavoriteIds] = useState<Set<string>>(getFavoriteIds);

  // Навігація
  const [selectedCategoryId, setSelectedCategoryId] = useState<string | null>(null);
  const [selectedCategoryName, setSelectedCategoryName] = useState('');

  // Товари
  const [products, setProducts] = useState<Product[]>([]);
  const [productsLoading, setProductsLoading] = useState(false);

  // Завантажуємо дерево категорій
  useEffect(() => {
    const load = async () => {
      setLoading(true);
      try {
        const tree = await categoryService.getCategoryTree();
        if (!Array.isArray(tree)) {
          console.error('Категорії: некоректна відповідь API (не масив)');
          setCategories([]);
          return;
        }
        setCategories(tree as unknown as CategoryNode[]);
      } catch (err) {
        console.error('Помилка завантаження категорій:', err);
      } finally {
        setLoading(false);
      }
    };
    load();
  }, []);

  // Завантажуємо товари при виборі категорії
  useEffect(() => {
    if (!selectedCategoryId) {
      setProducts([]);
      return;
    }
    const load = async () => {
      setProductsLoading(true);
      try {
        const response = await productService.getProducts({
          // '' (порожній рядок) не передаємо — інакше backend відповість 422
          category_id: selectedCategoryId || undefined,
          size: 100,
        });
        setProducts(response.items);
      } catch (err) {
        console.error('Помилка завантаження товарів:', err);
        setProducts([]);
      } finally {
        setProductsLoading(false);
      }
    };
    load();
  }, [selectedCategoryId]);

  const toggleFavorite = useCallback((catId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setFavoriteIds(prev => {
      const next = new Set(prev);
      if (next.has(catId)) {
        next.delete(catId);
      } else {
        next.add(catId);
      }
      saveFavoriteIds(next);
      return next;
    });
  }, []);

  const enterCategory = useCallback((cat: CategoryNode) => {
    setSelectedCategoryId(cat.id);
    setSelectedCategoryName(cat.name);
  }, []);

  const goBackToCategories = useCallback(() => {
    setSelectedCategoryId(null);
    setSelectedCategoryName('');
    setProducts([]);
  }, []);

  /** Отримати список обраних категорій (з усіх рівнів) */
  const favoriteCategories = useMemo((): CategoryNode[] => {
    if (favoriteIds.size === 0) return [];
    const all = flattenCategories(categories);
    return all.filter(cat => favoriteIds.has(cat.id));
  }, [categories, favoriteIds]);

  /** Отримати категорії першого рівня */
  const rootCategories = useMemo((): CategoryNode[] => {
    return categories;
  }, [categories]);

  // ========== Рівень 2: ПОКАЗ ТОВАРІВ ==========
  if (selectedCategoryId) {
    return (
      <div className="flex flex-col h-full">
        {/* Шапка з кнопкою назад — більш сучасний дизайн */}
        <div className="px-4 py-3 bg-white dark:bg-slate-800 border-b border-gray-200 dark:border-slate-700">
          <button
            onClick={goBackToCategories}
            className="group flex items-center gap-2 w-full px-4 py-3 bg-gradient-to-r from-primary-500 to-primary-600 hover:from-primary-600 hover:to-primary-700 text-white font-semibold rounded-2xl transition-all shadow-md hover:shadow-lg active:scale-[0.98]"
          >
            <ArrowLeft className="w-5 h-5 transition-transform group-hover:-translate-x-0.5" />
            <span>Назад до категорій</span>
          </button>
          <div className="flex items-center gap-1.5 mt-2.5 px-1">
            <span className="text-xs font-medium text-gray-400 bg-gray-100 dark:bg-slate-700 px-2 py-0.5 rounded-full">Категорії</span>
            <ChevronRight className="w-3 h-3 text-gray-400" />
            <span className="text-sm font-bold text-gray-800 dark:text-gray-200 truncate flex items-center gap-1.5">
              <Layers className="w-3.5 h-3.5 text-primary-500" />
              {selectedCategoryName}
            </span>
          </div>
        </div>

        {/* Товари — покращені плитки */}
        <div className="flex-1 overflow-y-auto p-3">
          {productsLoading ? (
            <div className="flex items-center justify-center py-20">
              <div className="flex flex-col items-center gap-3">
                <Loader2 className="w-10 h-10 text-primary-500 animate-spin" />
                <p className="text-sm text-gray-400">Завантаження товарів...</p>
              </div>
            </div>
          ) : products.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-20 text-gray-400">
              <div className="w-20 h-20 rounded-full bg-gray-100 dark:bg-slate-700 flex items-center justify-center mb-4">
                <Package className="w-10 h-10 text-gray-300 dark:text-gray-500" />
              </div>
              <p className="text-base font-semibold text-gray-500 dark:text-gray-400">Товарів немає</p>
              <p className="text-sm text-gray-400 mt-1">У цій категорії поки що нічого немає</p>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3">
              {products.map((product, idx) => {
                const stock = parseFloat(product.stock) || 0;
                const isOutOfStock = stock <= 0;
                const color = CATEGORY_COLORS[idx % CATEGORY_COLORS.length];

                return (
                  <button
                    key={product.id}
                    onClick={() => !isOutOfStock && onProductSelect(product)}
                    disabled={isOutOfStock}
                    className={`
                      relative flex flex-col items-start p-4 rounded-2xl border-2 transition-all text-left
                      ${isOutOfStock
                        ? 'border-gray-100 dark:border-slate-700 bg-gray-50 dark:bg-slate-800/30 opacity-55 cursor-not-allowed'
                        : `${color.bg} ${color.border} hover:shadow-lg active:scale-[0.97] cursor-pointer hover:-translate-y-0.5`
                      }
                    `}
                  >
                    {/* Бейдж ціни */}
                    <div className={`absolute top-3 right-3 px-2.5 py-1 rounded-full text-xs font-bold ${isOutOfStock ? 'bg-gray-100 text-gray-300' : color.badge}`}>
                      {formatCurrency(product.price)}
                    </div>

                    {/* Іконка */}
                    <div className={`w-10 h-10 rounded-xl flex items-center justify-center mb-3 ${isOutOfStock ? 'bg-gray-100 dark:bg-slate-700' : 'bg-white/60 dark:bg-white/10'}`}>
                      <Package className={`w-5 h-5 ${isOutOfStock ? 'text-gray-300' : color.icon}`} />
                    </div>

                    {/* Назва */}
                    <p className={`text-sm font-bold leading-tight mb-1.5 ${isOutOfStock ? 'text-gray-400' : 'text-gray-900 dark:text-gray-100'}`}>
                      {product.title}
                    </p>

                    {/* Залишок */}
                    <div className={`flex items-center gap-1.5 text-xs ${isOutOfStock ? 'text-gray-300' : 'text-gray-500 dark:text-gray-400'}`}>
                      <Tag className="w-3 h-3" />
                      <span>{product.stock} {formatUnit(product.unit)}</span>
                      {isOutOfStock && (
                        <span className="ml-1 text-[10px] font-bold text-danger-500 bg-danger-50 dark:bg-danger-900/30 px-1.5 py-0.5 rounded-full">
                          немає в наявності
                        </span>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>
    );
  }

  // ========== Рівень 1: ПОКАЗ КАТЕГОРІЙ ==========
  return (
    <div className="flex flex-col h-full">
      {loading ? (
        <div className="flex items-center justify-center py-20">
          <div className="flex flex-col items-center gap-3">
            <Loader2 className="w-10 h-10 text-primary-500 animate-spin" />
            <p className="text-sm text-gray-400">Завантаження категорій...</p>
          </div>
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto p-3 space-y-6">
          {/* ===== БЛОК "ВИБРАНЕ" — зі спеціальним оформленням ===== */}
          {favoriteCategories.length > 0 && (
            <div>
              <div className="flex items-center gap-2 mb-3 px-1">
                <div className="w-8 h-8 rounded-xl bg-gradient-to-br from-amber-400 to-amber-500 flex items-center justify-center shadow-sm">
                  <Heart className="w-4 h-4 text-white fill-white" />
                </div>
                <div>
                  <h3 className="text-base font-bold text-gray-900 dark:text-gray-100">
                    Вибране
                  </h3>
                  <p className="text-[11px] text-gray-400">{favoriteCategories.length} категорій</p>
                </div>
              </div>
              <div className="grid grid-cols-2 gap-3">
                {favoriteCategories.map((cat, idx) => {
                  const color = getColor(idx);
                  const hasChildren = cat.children && cat.children.length > 0;
                  const emoji = getCategoryEmoji(cat.name);

                  return (
                    <button
                      key={cat.id}
                      onClick={() => enterCategory(cat)}
                      className={`
                        relative flex flex-col items-center justify-center p-5 rounded-2xl border-2 transition-all text-center min-h-[130px]
                        ${color.bg} ${color.border}
                        hover:shadow-lg active:scale-[0.96] cursor-pointer hover:-translate-y-0.5
                      `}
                    >
                      {/* Зірочка зверху-праворуч */}
                      <span
                        onClick={(e) => toggleFavorite(cat.id, e)}
                        className="absolute top-2.5 right-2.5 w-7 h-7 rounded-full bg-white/70 dark:bg-black/30 flex items-center justify-center hover:bg-white dark:hover:bg-black/50 transition-colors shadow-sm"
                        title="Прибрати з вибраного"
                      >
                        <Star className="w-3.5 h-3.5 text-amber-500 fill-amber-500" />
                      </span>

                      {/* Емодзі */}
                      <span className="text-3xl mb-2">{emoji}</span>

                      {/* Назва */}
                      <p className={`text-sm font-bold ${color.text} leading-tight`}>
                        {cat.name}
                      </p>

                      {/* Підкатегорії */}
                      {hasChildren && (
                        <span className={`text-[10px] font-medium mt-1.5 px-2 py-0.5 rounded-full ${color.badge}`}>
                          {cat.children.length} підкатегорій
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {/* ===== ВСІ КАТЕГОРІЇ ===== */}
          <div>
            <div className="flex items-center gap-2 mb-3 px-1">
              <div className="w-8 h-8 rounded-xl bg-gradient-to-br from-primary-400 to-primary-500 flex items-center justify-center shadow-sm">
                <Grid3X3 className="w-4 h-4 text-white" />
              </div>
              <div>
                <h3 className="text-base font-bold text-gray-900 dark:text-gray-100">
                  {favoriteCategories.length > 0 ? 'Всі категорії' : 'Категорії'}
                </h3>
                <p className="text-[11px] text-gray-400">{rootCategories.length} розділів</p>
              </div>
            </div>

            {rootCategories.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 text-gray-400">
                <div className="w-16 h-16 rounded-full bg-gray-100 dark:bg-slate-700 flex items-center justify-center mb-3">
                  <Grid3X3 className="w-8 h-8 text-gray-300 dark:text-gray-500" />
                </div>
                <p className="text-sm font-medium">Немає категорій</p>
              </div>
            ) : (
              <div className="grid grid-cols-2 gap-3">
                {rootCategories.map((cat, idx) => {
                  const color = getColor(idx);
                  const isFavorite = favoriteIds.has(cat.id);
                  const hasChildren = cat.children && cat.children.length > 0;
                  const emoji = getCategoryEmoji(cat.name);

                  return (
                    <button
                      key={cat.id}
                      onClick={() => enterCategory(cat)}
                      className={`
                        relative flex flex-col items-center justify-center p-5 rounded-2xl border-2 transition-all text-center min-h-[130px] group
                        ${isFavorite ? color.bg : 'bg-white dark:bg-slate-800/60'}
                        ${isFavorite ? color.border : 'border-gray-200 dark:border-slate-700'}
                        ${isFavorite ? 'hover:shadow-lg' : 'hover:border-gray-300 dark:hover:border-slate-600 hover:shadow-md'}
                        active:scale-[0.96] cursor-pointer hover:-translate-y-0.5
                      `}
                    >
                      {/* Зірочка для додавання в обране */}
                      <span
                        onClick={(e) => toggleFavorite(cat.id, e)}
                        className={`absolute top-2.5 right-2.5 w-7 h-7 rounded-full flex items-center justify-center transition-all shadow-sm ${
                          isFavorite
                            ? 'bg-white/70 dark:bg-black/30 opacity-100'
                            : 'bg-white/0 dark:bg-black/0 opacity-0 group-hover:opacity-100 group-hover:bg-white/70 dark:group-hover:bg-black/30'
                        }`}
                        title={isFavorite ? 'Прибрати з вибраного' : 'Додати у вибране'}
                      >
                        {isFavorite ? (
                          <Star className="w-3.5 h-3.5 text-amber-500 fill-amber-500" />
                        ) : (
                          <StarOff className="w-3.5 h-3.5 text-gray-400" />
                        )}
                      </span>

                      {/* Емодзі */}
                      <span className="text-3xl mb-2">{emoji}</span>

                      {/* Назва */}
                      <p className={`text-sm font-bold leading-tight ${isFavorite ? color.text : 'text-gray-800 dark:text-gray-200'}`}>
                        {cat.name}
                      </p>

                      {/* Підкатегорії */}
                      {hasChildren && (
                        <span className={`text-[10px] font-medium mt-1.5 px-2 py-0.5 rounded-full ${
                          isFavorite ? color.badge : 'bg-gray-100 dark:bg-slate-700 text-gray-500 dark:text-gray-400'
                        }`}>
                          {cat.children.length} підкатегорій
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};

export default CategoryPanel;
