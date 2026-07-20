import { create } from 'zustand';

type Theme = 'light' | 'dark';
type ActiveModule = 'pos' | 'products' | 'categories' | 'suppliers' | 'documents' | 'reports' | 'ledger' | 'dashboard' | 'receipts';

interface UIStore {
  sidebarOpen: boolean;
  theme: Theme;
  activeModule: ActiveModule;
  toggleSidebar: () => void;
  setSidebarOpen: (open: boolean) => void;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;
  setActiveModule: (module: ActiveModule) => void;
}

export const useUIStore = create<UIStore>((set) => ({
  sidebarOpen: true,
  theme: (localStorage.getItem('theme') as Theme) || 'light',
  activeModule: 'dashboard',

  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),

  setSidebarOpen: (sidebarOpen: boolean) => set({ sidebarOpen }),

  setTheme: (theme: Theme) => {
    localStorage.setItem('theme', theme);
    if (theme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
    set({ theme });
  },

  toggleTheme: () =>
    set((state) => {
      const newTheme = state.theme === 'light' ? 'dark' : 'light';
      localStorage.setItem('theme', newTheme);
      if (newTheme === 'dark') {
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.classList.remove('dark');
      }
      return { theme: newTheme };
    }),

  setActiveModule: (activeModule: ActiveModule) => set({ activeModule }),
}));
