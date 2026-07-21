import React, { useState, useEffect, useCallback } from 'react';
import { FolderOpen, Package, Loader2, Star, StarOff, ChevronRight, Grid3X3, ArrowLeft } from 'lucide-react';
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

/** Знайти вузол категорії за ID в дереві */
const findNodeById = (nodes: CategoryNode[], id: string): CategoryNode | null => {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (node.children) {
      const found = findNodeById(node.children, id);
      if (found) return found;
    }
  }
  return null;
};

/** Знайти шлях до категорії (для хлібних крихт) */
const findPathToNode = (nodes: CategoryNode[], targetId: string, path: {id: string; name: string}[] = []): {id: string; name: string}[] | null => {
  for (const node of nodes) {
    const newPath = [...path, {id: node.id, name: node.name}];
    if (node.id === targetId) return newPath;
    if (node.children) {
      const found = findPathToNode(node.children, targetId, newPath);
      if (found) return found;
    }
  }
  return null;
};

export const CategoryPanel: React.FC<CategoryPanelProps> = ({ onProductSelect }) => {
  const [categories, setCategories] = useState<CategoryNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [favoriteIds, setFavoriteIds] = useState<Set<string>>(getFavoriteIds);
  const [viewMode, setViewMode] = useState<'all' | 'favorites'>('favorites');

  // Навігація по дереву: стек відкритих категорій
  const [navStack, setNavStack] = useState<{id: string; name: string; children: CategoryNode[]}[]>([]);

  // Поточна "папка" — список категорій/товарів для показу
  const [currentChildren, setCurrentChildren] = useState<CategoryNode[]>([]);
  const [currentCategoryId, setCurrentCategoryId] = useState<string | null>(null);
  const [currentCategoryName, setCurrentCategoryName] = useState('');

  // Товари (коли дійшли до листка)
  const [products, setProducts] = useState<Product[]>([]);
  const [productsLoading, setProductsLoading] = useState(false);

  // Завантажуємо дерево категорій
  useEffect(() => {
    const load = async () => {
      setLoading(true);
      try {
        const tree = await categoryService.getCategoryTree();
        setCategories(tree as unknown as CategoryNode[]);
        // Початково показуємо кореневі категорії
        setCurrentChildren(tree as unknown as CategoryNode[]);
        setCurrentCategoryId(null);
        setCurrentCategoryName('');
        setNavStack([]);
      } catch (err) {
        console.error('Помилка завантаження категорій:', err);
      } finally {
        setLoading(false);
      }
    };
    load();
  }, []);

  // Завантажуємо товари коли вибрана категорія-листок (без дітей)
  useEffect(() => {
    if (!currentCategoryId) {
      setProducts([]);
      return;
    }
    // Перевіряємо чи є діти — якщо є, не завантажуємо товари
    const node = findNodeById(categories, currentCategoryId);
    if (node && node.children && node.children.length > 0) {
      setProducts([]);
      return;
    }
    const load = async () => {
      setProductsLoading(true);
      try {
        const response = await productService.getProducts({
          category_id: currentCategoryId,
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
  }, [currentCategoryId, categories]);

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

  /** Вхід в категорію */
  const enterCategory = useCallback((cat: CategoryNode) => {
    if (cat.children && cat.children.length > 0) {
      // Є підкатегорії — заходимо всередину
      setNavStack(prev => [...prev, {id: cat.id, name: cat.name, children: currentChildren}]);
      setCurrentChildren(cat.children);
      setCurrentCategoryId(cat.id);
      setCurrentCategoryName(cat.name);
      setProducts([]);
    } else {
      // Немає підкатегорій — показуємо товари
      setCurrentCategoryId(cat.id);
      setCurrentCategoryName(cat.name);
      setProducts([]); // тригер для useEffect
    }
  }, [currentChildren]);

  /** Назад */
  const goBack = useCallback(() => {
    if (navStack.length === 0) {
      // Повертаємось на кореневий список
      setCurrentCategoryId(null);
      setCurrentCategoryName('');
      setProducts([]);
      setCurrentChildren(categories);
    } else {
      const prev = navStack[navStack.length - 1];
      setNavStack(prev => prev.slice(0, -1));
      setCurrentChildren(prev.children);
      setCurrentCategoryId(prev.id);
      setCurrentCategoryName(prev.name);
      setProducts([]);
    }
  }, [navStack, categories]);

  /** Отримати хлібні крихти */
  const getBreadcrumbs = (): {id: string; name: string}[] => {
    if (!currentCategoryId) return [];
    const path = findPathToNode(categories, currentCategoryId);
    return path || [];
  };

  /** Отримати категорії для показу в корені */
  const getRootCategories = () => {
    if (viewMode === 'favorites' && favoriteIds.size > 0) {
      // Показуємо тільки обрані категорії (першого рівня)
      return categories.filter(cat => favoriteIds.has(cat.id));
    }
    return categories;
  };

  // Визначаємо, чи ми в корені
  const isRoot = navStack.length === 0 && !currentCategoryId;

  // Якщо ми не в корені — показуємо вміст поточної категорії
  if (!isRoot) {
    const breadcrumbs = getBreadcrumbs();
    const currentNode = findNodeById(categories, currentCategoryId || '');
    const hasSubcategories = currentNode && currentNode.children && currentNode.children.length > 0;

    return (
      <div className="flex flex-col h-full">
        {/* Хлібні крихти */}
        <div className="flex items-center gap-1 px-4 py-3 bg-gray-50 dark:bg-slate-800/50 border-b border-gray-200 dark:border-slate-700">
          <span
            onClick={goBack}
            className="text-sm text-primary-600 hover:text-primary-700 font-medium whitespace-nowrap cursor-pointer"
          >
            ← Назад
          </span>
          {breadcrumbs.map((cat, idx) => (
            <React.Fragment key={cat.id}>
              <ChevronRight className="w-3 h-3 text-gray-400 flex-shrink-0" />
              <span
                className={`text-sm whitespace-nowrap truncate max-w-[120px] ${
                  idx === breadcrumbs.length - 1
                    ? 'text-gray-900 dark:text-gray-100 font-semibold'
                    : 'text-gray-500'
                }`}
              >
                {cat.name}
              </span>
            </React.Fragment>
          ))}
        </div>

        {/* Контент: підкатегорії або товари */}
        <div className="flex-1 overflow-y-auto">
          {hasSubcategories ? (
            /* Показуємо підкатегорії */
            <div className="divide-y divide-gray-100 dark:divide-slate-700/50">
              {currentChildren.map((child) => {
                const isFavorite = favoriteIds.has(child.id);
                const hasGrandChildren = child.children && child.children.length > 0;

                return (
                  <div key={child.id} className="group">
                    <div
                      onClick={() => enterCategory(child)}
                      className="w-full flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-gray-50 dark:hover:bg-slate-700/50 active:bg-gray-100 dark:active:bg-slate-700 transition-all"
                    >
                      <FolderOpen className={`w-5 h-5 flex-shrink-0 ${
                        isFavorite ? 'text-amber-500' : 'text-gray-400'
                      }`} />
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                          {child.name}
                        </p>
                        {hasGrandChildren && (
                          <p className="text-xs text-gray-400 mt-0.5">
                            {child.children.length} підкатегорій
                          </p>
                        )}
                      </div>
                      <span
                        onClick={(e) => toggleFavorite(child.id, e)}
                        className="flex-shrink-0 p-1 rounded-md opacity-0 group-hover:opacity-100 hover:bg-gray-200 dark:hover:bg-slate-600 transition-all cursor-pointer"
                        title={isFavorite ? 'Прибрати з обраних' : 'Додати в обрані'}
                      >
                        {isFavorite ? (
                          <Star className="w-4 h-4 text-amber-500 fill-amber-500" />
                        ) : (
                          <StarOff className="w-4 h-4 text-gray-400" />
                        )}
                      </span>
                      <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            /* Показуємо товари */
            <>
              {productsLoading ? (
                <div className="flex items-center justify-center py-12">
                  <Loader2 className="w-6 h-6 text-primary-500 animate-spin" />
                </div>
              ) : products.length === 0 ? (
                <div className="text-center py-12 text-gray-400 text-sm">
                  У цій категорії немає товарів
                </div>
              ) : (
                <div className="divide-y divide-gray-100 dark:divide-slate-700/50">
                  {products.map((product) => {
                    const stock = parseFloat(product.stock) || 0;
                    const isOutOfStock = stock <= 0;

                    return (
                      <div
                        key={product.id}
                        onClick={() => !isOutOfStock && onProductSelect(product)}
                        className={`
                          w-full flex items-center justify-between px-4 py-3 cursor-pointer transition-all
                          ${isOutOfStock
                            ? 'opacity-50 cursor-not-allowed'
                            : 'hover:bg-gray-50 dark:hover:bg-slate-700/50 active:bg-gray-100 dark:active:bg-slate-700'
                          }
                        `}
                      >
                        <div className="flex items-center gap-3 min-w-0">
                          <Package className="w-5 h-5 text-gray-400 flex-shrink-0" />
                          <div className="min-w-0">
                            <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                              {product.title}
                            </p>
                            <p className="text-xs text-gray-400 mt-0.5">
                              {product.barcode || 'Без ШК'} · {product.stock} {formatUnit(product.unit)}
                              {isOutOfStock && (
                                <span className="ml-1 text-danger-500 font-medium">(немає)</span>
                              )}
                            </p>
                          </div>
                        </div>
                        <span className="font-bold text-primary-600 text-sm ml-3 flex-shrink-0">
                          {formatCurrency(product.price)}
                        </span>
                      </div>
                    );
                  })}
                </div>
              )}
            </>
          )}
        </div>
      </div>
    );
  }

  // Кореневий режим — список категорій першого рівня
  return (
    <div className="flex flex-col h-full">
      {/* Перемикач: Всі / Обрані */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-gray-200 dark:border-slate-700">
        <div className="flex bg-gray-100 dark:bg-slate-700 rounded-lg p-0.5">
          <span
            onClick={() => setViewMode('favorites')}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md cursor-pointer transition-all ${
              viewMode === 'favorites'
                ? 'bg-white dark:bg-slate-600 text-primary-600 dark:text-primary-400 shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
            }`}
          >
            <Star className="w-3.5 h-3.5" />
            Обрані
          </span>
          <span
            onClick={() => setViewMode('all')}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md cursor-pointer transition-all ${
              viewMode === 'all'
                ? 'bg-white dark:bg-slate-600 text-primary-600 dark:text-primary-400 shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
            }`}
          >
            <Grid3X3 className="w-3.5 h-3.5" />
            Всі
          </span>
        </div>
        {viewMode === 'favorites' && favoriteIds.size === 0 && (
          <span className="text-xs text-gray-400 ml-auto">Оберіть категорії ★</span>
        )}
      </div>

      {/* Список категорій */}
      <div className="flex-1 overflow-y-auto">
        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="w-6 h-6 text-primary-500 animate-spin" />
          </div>
        ) : getRootCategories().length === 0 ? (
          <div className="text-center py-12 text-gray-400 text-sm">
            {viewMode === 'favorites'
              ? 'Немає обраних категорій.\nНатисніть ★ біля категорії, щоб додати її сюди.'
              : 'Немає категорій'}
          </div>
        ) : (
          <div className="divide-y divide-gray-100 dark:divide-slate-700/50">
            {getRootCategories().map((node) => {
              const isFavorite = favoriteIds.has(node.id);
              const hasChildren = node.children && node.children.length > 0;

              return (
                <div key={node.id} className="group">
                  <div
                    onClick={() => enterCategory(node)}
                    className="w-full flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-gray-50 dark:hover:bg-slate-700/50 active:bg-gray-100 dark:active:bg-slate-700 transition-all"
                  >
                    <FolderOpen className={`w-5 h-5 flex-shrink-0 ${
                      isFavorite ? 'text-amber-500' : 'text-gray-400'
                    }`} />
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                        {node.name}
                      </p>
                      {hasChildren && (
                        <p className="text-xs text-gray-400 mt-0.5">
                          {node.children.length} підкатегорій
                        </p>
                      )}
                    </div>
                    <span
                      onClick={(e) => toggleFavorite(node.id, e)}
                      className="flex-shrink-0 p-1 rounded-md opacity-0 group-hover:opacity-100 hover:bg-gray-200 dark:hover:bg-slate-600 transition-all cursor-pointer"
                      title={isFavorite ? 'Прибрати з обраних' : 'Додати в обрані'}
                    >
                      {isFavorite ? (
                        <Star className="w-4 h-4 text-amber-500 fill-amber-500" />
                      ) : (
                        <StarOff className="w-4 h-4 text-gray-400" />
                      )}
                    </span>
                    <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
};

export default CategoryPanel;
