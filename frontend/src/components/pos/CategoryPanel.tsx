import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { Package, Loader2, Star, StarOff, ArrowLeft, ChevronRight, Grid3X3, Heart } from 'lucide-react';
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

/** Палітра кольорів для плиток категорій */
const CATEGORY_COLORS = [
  { bg: 'bg-blue-50 dark:bg-blue-900/20', border: 'border-blue-200 dark:border-blue-800', text: 'text-blue-700 dark:text-blue-300', icon: 'text-blue-500' },
  { bg: 'bg-emerald-50 dark:bg-emerald-900/20', border: 'border-emerald-200 dark:border-emerald-800', text: 'text-emerald-700 dark:text-emerald-300', icon: 'text-emerald-500' },
  { bg: 'bg-amber-50 dark:bg-amber-900/20', border: 'border-amber-200 dark:border-amber-800', text: 'text-amber-700 dark:text-amber-300', icon: 'text-amber-500' },
  { bg: 'bg-rose-50 dark:bg-rose-900/20', border: 'border-rose-200 dark:border-rose-800', text: 'text-rose-700 dark:text-rose-300', icon: 'text-rose-500' },
  { bg: 'bg-violet-50 dark:bg-violet-900/20', border: 'border-violet-200 dark:border-violet-800', text: 'text-violet-700 dark:text-violet-300', icon: 'text-violet-500' },
  { bg: 'bg-cyan-50 dark:bg-cyan-900/20', border: 'border-cyan-200 dark:border-cyan-800', text: 'text-cyan-700 dark:text-cyan-300', icon: 'text-cyan-500' },
  { bg: 'bg-orange-50 dark:bg-orange-900/20', border: 'border-orange-200 dark:border-orange-800', text: 'text-orange-700 dark:text-orange-300', icon: 'text-orange-500' },
  { bg: 'bg-teal-50 dark:bg-teal-900/20', border: 'border-teal-200 dark:border-teal-800', text: 'text-teal-700 dark:text-teal-300', icon: 'text-teal-500' },
  { bg: 'bg-pink-50 dark:bg-pink-900/20', border: 'border-pink-200 dark:border-pink-800', text: 'text-pink-700 dark:text-pink-300', icon: 'text-pink-500' },
  { bg: 'bg-indigo-50 dark:bg-indigo-900/20', border: 'border-indigo-200 dark:border-indigo-800', text: 'text-indigo-700 dark:text-indigo-300', icon: 'text-indigo-500' },
  { bg: 'bg-lime-50 dark:bg-lime-900/20', border: 'border-lime-200 dark:border-lime-800', text: 'text-lime-700 dark:text-lime-300', icon: 'text-lime-500' },
  { bg: 'bg-sky-50 dark:bg-sky-900/20', border: 'border-sky-200 dark:border-sky-800', text: 'text-sky-700 dark:text-sky-300', icon: 'text-sky-500' },
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

/** Знайти вузол категорії за ID в дереві (рекурсивно) */
const findNodeById = (nodes: CategoryNode[], id: string): CategoryNode | null => {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (node.children && node.children.length > 0) {
      const found = findNodeById(node.children, id);
      if (found) return found;
    }
  }
  return null;
};

/** Зібрати всі категорії з дерева в плоский список (рекурсивно) */
const flattenCategories = (nodes: CategoryNode[]): CategoryNode[] => {
  const result: CategoryNode[] = [];
  for (const node of nodes) {
    result.push(node);
    if (node.children && node.children.length > 0) {
      result.push(...flattenCategories(node.children));
    }
  }
  return result;
};

export const CategoryPanel: React.FC<CategoryPanelProps> = ({ onProductSelect }) => {
  const [categories, setCategories] = useState<CategoryNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [favoriteIds, setFavoriteIds] = useState<Set<string>>(getFavoriteIds);

  // Навігація: Рівень 1 (категорії) або Рівень 2 (товари)
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
          category_id: selectedCategoryId,
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

  /** Вхід в категорію (Рівень 2 — показуємо товари) */
  const enterCategory = useCallback((cat: CategoryNode) => {
    setSelectedCategoryId(cat.id);
    setSelectedCategoryName(cat.name);
  }, []);

  /** Назад до категорій (Рівень 1) */
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
        {/* Кнопка "← Назад до категорій" — завжди зверху, помітна */}
        <div className="px-4 py-3 bg-white dark:bg-slate-800 border-b-2 border-primary-200 dark:border-primary-800">
          <button
            onClick={goBackToCategories}
            className="flex items-center gap-2 w-full px-4 py-2.5 bg-primary-50 dark:bg-primary-900/30 hover:bg-primary-100 dark:hover:bg-primary-900/50 text-primary-700 dark:text-primary-300 font-semibold rounded-xl transition-all border border-primary-200 dark:border-primary-700 shadow-sm"
          >
            <ArrowLeft className="w-5 h-5" />
            <span>← Назад до категорій</span>
          </button>
          <div className="flex items-center gap-1 mt-2 px-1">
            <span className="text-xs text-gray-400">Категорії</span>
            <ChevronRight className="w-3 h-3 text-gray-400" />
            <span className="text-xs font-semibold text-gray-700 dark:text-gray-300 truncate">
              {selectedCategoryName}
            </span>
          </div>
        </div>

        {/* Товари — плитки (grid) */}
        <div className="flex-1 overflow-y-auto p-3">
          {productsLoading ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="w-8 h-8 text-primary-500 animate-spin" />
            </div>
          ) : products.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-16 text-gray-400">
              <Package className="w-16 h-16 mb-3 opacity-30" />
              <p className="text-sm font-medium">У цій категорії немає товарів</p>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3">
              {products.map((product) => {
                const stock = parseFloat(product.stock) || 0;
                const isOutOfStock = stock <= 0;

                return (
                  <button
                    key={product.id}
                    onClick={() => !isOutOfStock && onProductSelect(product)}
                    disabled={isOutOfStock}
                    className={`
                      relative flex flex-col items-center justify-center p-4 rounded-xl border-2 transition-all text-center min-h-[120px]
                      ${isOutOfStock
                        ? 'border-gray-100 dark:border-slate-700 bg-gray-50 dark:bg-slate-800/50 opacity-50 cursor-not-allowed'
                        : 'border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:border-primary-300 dark:hover:border-primary-600 hover:shadow-md active:scale-[0.98] cursor-pointer'
                      }
                    `}
                  >
                    <Package className={`w-8 h-8 mb-2 ${isOutOfStock ? 'text-gray-300' : 'text-primary-500'}`} />
                    <p className={`text-sm font-semibold leading-tight mb-1 ${isOutOfStock ? 'text-gray-400' : 'text-gray-900 dark:text-gray-100'}`}>
                      {product.title}
                    </p>
                    <p className={`text-xs ${isOutOfStock ? 'text-gray-300' : 'text-gray-400'}`}>
                      {product.stock} {formatUnit(product.unit)}
                    </p>
                    <p className={`text-sm font-bold mt-1 ${isOutOfStock ? 'text-gray-300' : 'text-primary-600 dark:text-primary-400'}`}>
                      {formatCurrency(product.price)}
                    </p>
                    {isOutOfStock && (
                      <span className="absolute top-2 right-2 text-[10px] font-bold text-danger-500 bg-danger-50 dark:bg-danger-900/30 px-2 py-0.5 rounded-full">
                        немає
                      </span>
                    )}
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
        <div className="flex items-center justify-center py-16">
          <Loader2 className="w-8 h-8 text-primary-500 animate-spin" />
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto p-3 space-y-5">
          {/* ===== БЛОК "ВИБРАНЕ" (закріплений зверху) ===== */}
          {favoriteCategories.length > 0 && (
            <div>
              <div className="flex items-center gap-2 mb-3">
                <Heart className="w-4 h-4 text-amber-500 fill-amber-500" />
                <h3 className="text-sm font-bold text-gray-700 dark:text-gray-300 uppercase tracking-wider">
                  Вибране
                </h3>
                <span className="text-xs text-gray-400">({favoriteCategories.length})</span>
              </div>
              <div className="grid grid-cols-2 gap-3">
                {favoriteCategories.map((cat, idx) => {
                  const color = getColor(idx);
                  const hasChildren = cat.children && cat.children.length > 0;

                  return (
                    <button
                      key={cat.id}
                      onClick={() => enterCategory(cat)}
                      className={`
                        relative flex flex-col items-center justify-center p-4 rounded-xl border-2 transition-all text-center min-h-[100px]
                        ${color.bg} ${color.border}
                        hover:shadow-md active:scale-[0.97] cursor-pointer
                      `}
                    >
                      {/* Зірочка зверху-праворуч */}
                      <span
                        onClick={(e) => toggleFavorite(cat.id, e)}
                        className="absolute top-2 right-2 p-1 rounded-full hover:bg-white/50 dark:hover:bg-black/20 transition-colors"
                        title="Прибрати з вибраного"
                      >
                        <Star className="w-4 h-4 text-amber-500 fill-amber-500" />
                      </span>
                      <p className={`text-sm font-bold ${color.text} leading-tight`}>
                        {cat.name}
                      </p>
                      {hasChildren && (
                        <p className={`text-xs mt-1 opacity-70 ${color.text}`}>
                          {cat.children.length} підкатегорій
                        </p>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {/* ===== ВСІ КАТЕГОРІЇ ===== */}
          <div>
            <div className="flex items-center gap-2 mb-3">
              <Grid3X3 className="w-4 h-4 text-gray-500" />
              <h3 className="text-sm font-bold text-gray-700 dark:text-gray-300 uppercase tracking-wider">
                {favoriteCategories.length > 0 ? 'Всі категорії' : 'Категорії'}
              </h3>
            </div>
            {rootCategories.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-12 text-gray-400">
                <Grid3X3 className="w-12 h-12 mb-2 opacity-30" />
                <p className="text-sm">Немає категорій</p>
              </div>
            ) : (
              <div className="grid grid-cols-2 gap-3">
                {rootCategories.map((cat, idx) => {
                  const color = getColor(idx);
                  const isFavorite = favoriteIds.has(cat.id);
                  const hasChildren = cat.children && cat.children.length > 0;

                  return (
                    <button
                      key={cat.id}
                      onClick={() => enterCategory(cat)}
                      className={`
                        relative flex flex-col items-center justify-center p-4 rounded-xl border-2 transition-all text-center min-h-[100px]
                        ${isFavorite ? color.bg : 'bg-gray-50 dark:bg-slate-800/50'}
                        ${isFavorite ? color.border : 'border-gray-200 dark:border-slate-700'}
                        hover:shadow-md active:scale-[0.97] cursor-pointer
                        ${isFavorite ? '' : 'hover:border-gray-300 dark:hover:border-slate-600'}
                      `}
                    >
                      {/* Зірочка для додавання в обране */}
                      <span
                        onClick={(e) => toggleFavorite(cat.id, e)}
                        className={`absolute top-2 right-2 p-1 rounded-full transition-colors ${
                          isFavorite
                            ? 'opacity-100'
                            : 'opacity-0 group-hover:opacity-100 hover:bg-gray-200 dark:hover:bg-slate-600'
                        }`}
                        title={isFavorite ? 'Прибрати з вибраного' : 'Додати у вибране'}
                      >
                        {isFavorite ? (
                          <Star className="w-4 h-4 text-amber-500 fill-amber-500" />
                        ) : (
                          <StarOff className="w-4 h-4 text-gray-400" />
                        )}
                      </span>
                      <p className={`text-sm font-bold leading-tight ${isFavorite ? color.text : 'text-gray-800 dark:text-gray-200'}`}>
                        {cat.name}
                      </p>
                      {hasChildren && (
                        <p className={`text-xs mt-1 ${isFavorite ? `opacity-70 ${color.text}` : 'text-gray-400'}`}>
                          {cat.children.length} підкатегорій
                        </p>
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
