import React from 'react';
import { Navigate } from 'react-router-dom';
import { useAuthStore } from '@/store/authStore';

interface RoleRouteProps {
  children: React.ReactNode;
  roles: ('admin' | 'cashier')[];
}

export const RoleRoute: React.FC<RoleRouteProps> = ({ children, roles }) => {
  const user = useAuthStore((state) => state.user);

  if (!user) {
    return <Navigate to="/login" replace />;
  }

  if (!roles.includes(user.role as 'admin' | 'cashier')) {
    // Якщо касир намагається зайти на адмінську сторінку - редірект на POS
    return <Navigate to="/pos" replace />;
  }

  return <>{children}</>;
};
