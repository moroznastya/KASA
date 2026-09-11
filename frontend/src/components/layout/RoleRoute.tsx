import React from 'react';
import { Navigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';

interface RoleRouteProps {
  children: React.ReactNode;
  roles: ('admin' | 'cashier' | 'owner' | 'store_manager')[];
}

export const RoleRoute: React.FC<RoleRouteProps> = ({ children, roles }) => {
  const user = useAuthStore((state) => state.user);

  if (!user) {
    return <Navigate to="/login" replace />;
  }

  // Owner/store_manager/manager — привілейовані ролі: доступ до всіх адмін-сторінок
  // (аналог Sidebar.tsx). store_manager — «керуючий мережею» (Етап 1 адмін-панелі).
  if (user.role === 'owner' || user.role === 'store_manager' || user.role === 'manager') {
    return <>{children}</>;
  }

  if (!roles.includes(user.role as 'admin' | 'cashier' | 'owner' | 'store_manager')) {
    // Якщо касир намагається зайти на адмінську сторінку - редірект на POS
    return <Navigate to="/pos" replace />;
  }

  return <>{children}</>;
};
