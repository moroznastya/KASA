import { create } from 'zustand';
import { storeService } from '@/services/storeService';
import { Store } from '@/types/store';

const STORAGE_KEY = 'activeStoreId';

interface StoreState {
  /** Точки поточного користувача (RLS). */
  stores: Store[];
  /** Активна точка — додається в X-Store-Id кожного бізнес-запиту. */
  activeStoreId: string | null;
  /** Чи завершено первинне завантаження (gate для AppLayout). */
  storesLoaded: boolean;
  /** Завантажити точки + автовибір (localStorage → is_default → перша). */
  loadStores: () => Promise<void>;
  /** Змінити активну точку + зберегти в localStorage. */
  setActiveStore: (storeId: string) => void;
}

export const useStoreStore = create<StoreState>((set, get) => ({
  stores: [],
  activeStoreId: (() => {
    try {
      return localStorage.getItem(STORAGE_KEY);
    } catch {
      return null;
    }
  })(),
  storesLoaded: false,

  loadStores: async () => {
    try {
      const stores = await storeService.list();
      // Автовибір: збережений id (якщо ще в списку) → default → перша точка.
      let activeStoreId = get().activeStoreId;
      if (!stores.some((s) => s.id === activeStoreId)) {
        const fallback = stores.find((s) => s.is_default) || stores[0] || null;
        activeStoreId = fallback?.id ?? null;
        try {
          if (activeStoreId) localStorage.setItem(STORAGE_KEY, activeStoreId);
          else localStorage.removeItem(STORAGE_KEY);
        } catch {
          /* ignore */
        }
      }
      set({ stores, activeStoreId, storesLoaded: true });
    } catch {
      // Помилка мережі: не блокуємо застосунок — зберігаємо наявний стан.
      // Якщо користувач має збережену точку, він продовжить працювати.
      set({ storesLoaded: true });
    }
  },

  setActiveStore: (storeId: string) => {
    try {
      localStorage.setItem(STORAGE_KEY, storeId);
    } catch {
      /* ignore */
    }
    set({ activeStoreId: storeId });
  },
}));
