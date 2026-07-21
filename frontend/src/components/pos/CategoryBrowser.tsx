import React, { useState, useEffect, useCallback } from 'react';
import { ChevronDown, ChevronRight, Package, Layers, Eye, EyeOff, Settings2 } from 'lucide-react';
import { useCategoryTree } from '@/hooks/useCategories';
import { productService } from '@/services/productService';
import { formatCurrency, formatUnit } from '@/utils/format';
import { Product } from '@/types/product';
import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { useAuthStore } from '@/store/authStore';

interface CategoryBrowserProps {
  onProductSelect: (product: Product) => void;
}

const STORAGE_PREFIX = 'pos_categories_';

interface StoredSettings {
  visibleIds: string[];
  isCollapsed: boolean;
}

function loadSettings(userId: string): StoredSettings | null {
  try {
    const raw = localStorage.getItem(STORAGE_PREFIX + userId);
    if (raw) return JSON.parse(raw);
  } catch { /* ignore */ }
  return null;
}

function saveSettings(userId: string, settings: StoredSettings) {
  try {
    localStorage.setItem(STORAGE_PREFIX + userId, JSON.stringify(settings));
  } catch { /* ignore */ }
}

export const CategoryBrowser: React.FC<CategoryBrowserProps> = ({ onProductSelect }) => {
  const user = useAuthStore((s) => s.user);
  const userId = user?.id || 'anonymous';

  const { data: categoryTree, isLoading } = useCategoryTree();
  const [expandedCategories, setExpandedCategories] = useState<Set<string>>(new Set());
  const [selectedCategoryId, setSelectedCategoryId] = useState<string | null>(null);
  const [products, setProducts] = useState<Product[]>([]);
  const [isLoadingProducts, setIsLoadingProducts] = useState(false);
  const [isCollapsed, setIsCollapsed] = useState(true); // <-- за замовчуванням приховано
  const [showSettings, setShowSettings] = useState(false);
  const [visibleCategories, setVisibleCategories] = useState<Set<string>>(new Set());
  const [isInitialized, setIsInitialized] = useState(false);

  // Завантаження збережених налаштувань для користувача
  useEffect(() => {
    if (!categoryTree || categoryTree.length === 0 || isInitialized) return;

    const saved = loadSettings(userId);
    const allIds = new Set<string>();
    const collectIds = (cats: any[]) => {
      cats.forEach((cat: any) => {
        allIds.add(cat.id);
        if (cat.children) collectIds(cat.children);
      });
    };
    collectIds(categoryTree);

    if (saved) {
      // Відновлюємо тільки ті ID, які досі існують
      const restored = new Set(saved.visibleIds.filter((id) => allIds.has(id)));
      setVisibleCategories(restored.size > 0 ? restored : allIds);
      setIsCollapsed(saved.isCollapsed ?? true);
    } else {
      setVisibleCategories(allIds);
      setIsCollapsed(true);
    }
    setIsInitialized(true);
  }, [categoryTree, userId, isInitialized]);

  // Автоматичне збереження при зміні налаштувань
  useEffect(() => {
    if (!isInitialized) return;
    saveSettings(userId, {
      visibleIds: Array.from(visibleCategories),
      isCollapsed,
    });
  }, [visibleCategories, isCollapsed, userId, isInitialized]);

  // Завантаження товарів при виборі категорії/підкатегорії
  useEffect(() => {
    if (!selectedCategoryId) {
      setProducts([]);
      return;
    }

    setIsLoadingProducts(true);
    productService.getProductsByCategory(selectedCategoryId)
      .then((res) => {
        setProducts(res.items || []);
      })
      .catch(() => {
        setProducts([]);
      })
      .finally(() => {
        setIsLoadingProducts(false);
      });
  }, [selectedCategoryId]);

  const toggleExpand = useCallback((categoryId: string) => {
    setExpandedCategories((prev) => {
      const next = new Set(prev);
      if (next.has(categoryId)) {
        next.delete(categoryId);
      } else {
        next.add(categoryId);
      }
      return next;
    });
  }, []);

  const handleCategoryClick = useCallback((categoryId: string, hasChildren: boolean) => {
    if (hasChildren) {
      toggleExpand(categoryId);
    }
    setSelectedCategoryId(categoryId);
  }, [toggleExpand]);

  const toggleCategoryVisibility = useCallback((categoryId: string) => {
    setVisibleCategories((prev) => {
      const next = new Set(prev);
      if (next.has(categoryId)) {
        next.delete(categoryId);
      } else {
        next.add(categoryId);
      }
      return next;
    });
  }, []);

  const selectAllCategories = useCallback(() => {
    if (!categoryTree) return;
    const allIds = new Set<string>();
    const collectIds = (cats: any[]) => {
      cats.forEach((cat: any) => {
        allIds.add(cat.id);
        if (cat.children) collectIds(cat.children);
      });
    };
    collectIds(categoryTree);
    setVisibleCategories(allIds);
  }, [categoryTree]);

  const deselectAllCategories = useCallback(() => {
    setVisibleCategories(new Set());
  }, []);

  const renderCategoryItem = (category: any, depth: number = 0) => {
    const hasChildren = category.children && category.children.length > 0;
    const isExpanded = expandedCategories.has(category.id);
    const isSelected = selectedCategoryId === category.id;
    const isVisible = visibleCategories.has(category.id);

    if (!isVisible) return null;

    return (
      <div key={category.id} className="flex-shrink-0">
        <button
          onClick={() => handleCategoryClick(category.id, hasChildren)}
          className={`
            flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-medium transition-all whitespace-nowrap
            ${isSelected
              ? 'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-400 ring-1 ring-primary-300 dark:ring-primary-700'
              : 'bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600'
            }
          `}
        >
          {hasChildren && (
            <span className="flex-shrink-0">
              {isExpanded ? (
                <ChevronDown className="w-3 h-3" />
              ) : (
                <ChevronRight className="w-3 h-3" />
              )}
            </span>
          )}
          <Layers className="w-3.5 h-3.5 flex-shrink-0" />
          <span>{category.name}</span>
        </button>

        {/* Підкатегорії */}
        {hasChildren && isExpanded && (
          <div className="flex gap-1.5 mt-1.5 ml-2">
            {category.children.map((child: any) => renderCategoryItem(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  const renderSettingsCategoryItem = (category: any, depth: number = 0) => {
    const isVisible = visibleCategories.has(category.id);
    const hasChildren = category.children && category.children.length > 0;

    return (
      <div key={category.id} className="select-none">
        <label
          className="flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-all hover:bg-gray-50 dark:hover:bg-slate-700"
          style={{ marginLeft: depth > 0 ? `${depth * 16}px` : '0' }}
        >
          <input
            type="checkbox"
            checked={isVisible}
            onChange={() => toggleCategoryVisibility(category.id)}
            className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
          />
          <span className={`text-sm ${isVisible ? 'text-gray-900 dark:text-gray-100 font-medium' : 'text-gray-400 dark:text-gray-500'}`}>
            {category.name}
          </span>
          {!isVisible && (
            <EyeOff className="w-3.5 h-3.5 text-gray-400 ml-auto" />
          )}
        </label>
        {hasChildren && (
          <div className="ml-2">
            {category.children.map((child: any) => renderSettingsCategoryItem(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  // --- СТАН ПРИХОВАНО ---
  if (isCollapsed) {
    return (
      <div className="flex items-center gap-2">
        <button
          onClick={() => setIsCollapsed(false)}
          className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-medium bg-gray-100 dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-slate-600 transition-all"
          title="Показати категорії"
        >
          <Layers className="w-3.5 h-3.5" />
          <span>Категорії</span>
        </button>
      </div>
    );
  }

  // --- СТАН РОЗГОРНУТО ---
  return (
    <div className="card p-3">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <Layers className="w-4 h-4 text-primary-600" />
          <span className="text-xs font-semibold text-gray-700 dark:text-gray-300">
            Категорії
          </span>
          {/* Кнопка приховати біля "Категорії" */}
          <button
            onClick={() => setIsCollapsed(true)}
            className="p-1 rounded-md text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-all"
            title="Приховати категорії"
          >
            <EyeOff className="w-3.5 h-3.5" />
          </button>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => setShowSettings(true)}
            className="p-1.5 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-all"
            title="Налаштування категорій"
          >
            <Settings2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {isLoading ? (
        <div className="flex gap-2 overflow-x-auto pb-1">
          {[1, 2, 3, 4].map((i) => (
            <div
              key={i}
              className="flex-shrink-0 h-8 w-24 bg-gray-100 dark:bg-slate-700 rounded-lg animate-pulse"
            />
          ))}
        </div>
      ) : categoryTree && categoryTree.length > 0 ? (
        <div className="flex gap-2 overflow-x-auto pb-1 scrollbar-thin">
          {categoryTree
            .filter((cat) => visibleCategories.has(cat.id))
            .map((category) => renderCategoryItem(category))}
        </div>
      ) : (
        <p className="text-xs text-gray-400 py-1">Немає категорій</p>
      )}

      {/* Товари вибраної категорії */}
      {selectedCategoryId && (
        <div className="mt-3 pt-3 border-t border-gray-100 dark:border-slate-700">
          <div className="flex items-center gap-1.5 mb-2">
            <Package className="w-3.5 h-3.5 text-gray-400" />
            <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
              Товари категорії
            </span>
            {isLoadingProducts && (
              <span className="text-xs text-gray-400">Завантаження...</span>
            )}
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-2">
            {isLoadingProducts ? (
              <>
                {[1, 2, 3, 4, 5, 6].map((i) => (
                  <div
                    key={i}
                    className="h-16 bg-gray-100 dark:bg-slate-700 rounded-lg animate-pulse"
                  />
                ))}
              </>
            ) : products.length > 0 ? (
              products.map((product) => {
                const stock = parseFloat(product.stock) || 0;
                const isOutOfStock = stock <= 0;
                return (
                  <button
                    key={product.id}
                    onClick={() => onProductSelect(product)}
                    disabled={isOutOfStock}
                    className={`
                      text-left p-2 rounded-lg border transition-all
                      ${isOutOfStock
                        ? 'opacity-50 cursor-not-allowed border-gray-100 dark:border-slate-700'
                        : 'border-gray-100 dark:border-slate-700 hover:border-primary-300 dark:hover:border-primary-600 hover:shadow-sm'
                      }
                    `}
                  >
                    <p className={`text-xs font-medium truncate ${isOutOfStock ? 'text-gray-400' : 'text-gray-900 dark:text-gray-100'}`}>
                      {product.title}
                    </p>
                    <div className="flex items-center justify-between mt-1">
                      <span className="text-xs text-gray-400">
                        {product.stock} {formatUnit(product.unit)}
                      </span>
                      <span className="text-xs font-bold text-primary-600">
                        {formatCurrency(product.price)}
                      </span>
                    </div>
                  </button>
                );
              })
            ) : (
              <p className="text-xs text-gray-400 col-span-full py-2 text-center">
                Немає товарів у цій категорії
              </p>
            )}
          </div>
        </div>
      )}

      {/* Modal for category visibility settings */}
      <Modal
        isOpen={showSettings}
        onClose={() => setShowSettings(false)}
        title="Налаштування категорій"
        size="md"
      >
        <div className="space-y-4">
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Оберіть категорії, які будуть відображатись на панелі каси
          </p>

          <div className="flex items-center justify-between px-3 py-2 bg-gray-50 dark:bg-slate-700 rounded-lg">
            <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
              {visibleCategories.size} категорій вибрано
            </span>
            <div className="flex gap-2">
              <button
                onClick={selectAllCategories}
                className="text-xs text-primary-600 hover:text-primary-700 font-medium"
              >
                Вибрати всі
              </button>
              <button
                onClick={deselectAllCategories}
                className="text-xs text-gray-500 hover:text-gray-700 font-medium"
              >
                Скасувати всі
              </button>
            </div>
          </div>

          <div className="max-h-64 overflow-y-auto border border-gray-200 dark:border-slate-600 rounded-lg divide-y divide-gray-100 dark:divide-slate-700">
            {categoryTree && categoryTree.length > 0 ? (
              categoryTree.map((category) => renderSettingsCategoryItem(category))
            ) : (
              <p className="text-sm text-gray-400 text-center py-4">Немає категорій</p>
            )}
          </div>

          <div className="flex justify-end gap-3 pt-2">
            <Button variant="secondary" onClick={() => setShowSettings(false)}>
              Готово
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
};
