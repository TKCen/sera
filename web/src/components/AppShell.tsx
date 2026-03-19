import { Outlet, useLocation } from 'react-router';
import { Sidebar } from '@/components/Sidebar';

export function AppShell() {
  const location = useLocation();

  const routeTitles: Record<string, string> = {
    '/chat': 'Chat',
    '/agents': 'Agents',
    '/agents/new': 'New Agent',
    '/circles': 'Circles',
    '/insights': 'Insights',
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

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <main className="flex-1 overflow-y-auto bg-sera-bg">
        <Outlet />
      </main>
    </div>
  );
}
