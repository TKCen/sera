import { Link, Outlet, useLocation } from 'react-router';
import { AlertTriangle } from 'lucide-react';
import { Sidebar } from '@/components/Sidebar';
import { useHealthDetail } from '@/hooks/useHealth';

export function AppShell() {
  const location = useLocation();
  const { data: health } = useHealthDetail();

  const routeTitles: Record<string, string> = {
    '/chat': 'Chat',
    '/agents': 'Agents',
    '/agents/new': 'New Agent',
    '/circles': 'Circles',
    '/insights': 'Insights',
    '/audit': 'Audit',
    '/health': 'Health',
    '/schedules': 'Schedules',
    '/settings': 'Settings',
    '/tools': 'Tools',
  };

  const baseTitle = 'SERA | Sandboxed Extensible Reasoning Agent';
  const pageTitle =
    Object.entries(routeTitles).find(([path]) =>
      location.pathname === path || location.pathname.startsWith(path + '/'),
    )?.[1] ?? null;

  if (pageTitle) {
    document.title = `${pageTitle} — SERA`;
  } else {
    document.title = baseTitle;
  }

  const unhealthy = health && health.status !== 'healthy';
  const unhealthyComponents = health?.components.filter((c) => c.status !== 'healthy') ?? [];

  return (
    <div className="flex h-screen overflow-hidden flex-col">
      {unhealthy && (
        <div className="flex items-center gap-2 px-4 py-2 bg-sera-warning/10 border-b border-sera-warning/30 text-sera-warning text-xs flex-shrink-0">
          <AlertTriangle size={13} />
          <span>
            {unhealthyComponents.length === 1
              ? `${unhealthyComponents[0].name} is ${unhealthyComponents[0].status}`
              : `${unhealthyComponents.length} components are unhealthy`}
            {' — '}
          </span>
          <Link to="/health" className="underline hover:no-underline">View system health</Link>
        </div>
      )}
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-y-auto bg-sera-bg">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
