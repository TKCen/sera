import { useState, useEffect } from 'react';
import { Link, Outlet, useLocation } from 'react-router';
import { AlertTriangle, X } from 'lucide-react';
import { Sidebar } from '@/components/Sidebar';
import { useHealthDetail } from '@/hooks/useHealth';

const CRITICAL_COMPONENTS = new Set(['database', 'docker', 'sera-core']);

export function AppShell() {
  const location = useLocation();
  const { data: health } = useHealthDetail();
  const [dismissed, setDismissed] = useState(false);

  const routeTitles: Record<string, string> = {
    '/': 'Dashboard',
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
    Object.entries(routeTitles).find(
      ([path]) => location.pathname === path || location.pathname.startsWith(path + '/')
    )?.[1] ?? null;

  if (pageTitle) {
    document.title = `${pageTitle} — SERA`;
  } else {
    document.title = baseTitle;
  }

  const unhealthyComponents = health?.components.filter((c) => c.status !== 'healthy') ?? [];
  const hasCritical = unhealthyComponents.some((c) => CRITICAL_COMPONENTS.has(c.name));
  const showBanner = unhealthyComponents.length > 0 && !dismissed;

  // Re-show banner when the set of unhealthy components changes
  const bannerKey = unhealthyComponents
    .map((c) => c.name)
    .sort()
    .join(',');
  useEffect(() => {
    setDismissed(false);
  }, [bannerKey]);

  return (
    <div className="flex h-screen overflow-hidden flex-col">
      {showBanner && (
        <div
          className={`flex items-center gap-2 px-4 py-2 border-b text-xs flex-shrink-0 ${
            hasCritical
              ? 'bg-sera-error/10 border-sera-error/30 text-sera-error'
              : 'bg-yellow-500/10 border-yellow-500/30 text-yellow-400'
          }`}
        >
          <AlertTriangle size={13} className="flex-shrink-0" />
          <span className="flex-1">
            {unhealthyComponents.map((c) => c.name).join(', ')}
            {unhealthyComponents.length === 1 ? ' is ' : ' are '}
            {hasCritical ? 'unhealthy' : 'degraded'}
            {' — '}
            <Link to="/health" className="underline hover:no-underline">
              View system health
            </Link>
          </span>
          <button
            onClick={() => setDismissed(true)}
            className="p-0.5 rounded hover:bg-white/10 transition-colors flex-shrink-0"
            title="Dismiss"
          >
            <X size={12} />
          </button>
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
