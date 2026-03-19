import { type ReactNode } from 'react';
import { Navigate, Outlet } from 'react-router';
import { useAuth } from '@/contexts/AuthContext';

interface ProtectedRouteProps {
  children?: ReactNode;
  requiredRole?: string;
}

export function ProtectedRoute({ children, requiredRole }: ProtectedRouteProps) {
  const { isAuthenticated, isLoading, roles } = useAuth();

  if (isLoading) {
    return (
      <div className="flex h-screen items-center justify-center bg-sera-bg">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-sera-border border-t-sera-accent" />
      </div>
    );
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  if (requiredRole && !roles.includes(requiredRole)) {
    return <ForbiddenInline />;
  }

  return children ?? <Outlet />;
}

function ForbiddenInline() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
      <div className="text-5xl">🚫</div>
      <h2 className="text-xl font-semibold text-sera-text">Access Denied</h2>
      <p className="max-w-md text-sm text-sera-text-muted">
        You don&apos;t have the required role to view this page.
      </p>
    </div>
  );
}
