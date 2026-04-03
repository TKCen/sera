import { NavLink, useLocation } from 'react-router';
import {
  LayoutDashboard,
  MessageSquare,
  Bot,
  CalendarClock,
  BarChart3,
  Settings,
  CircleIcon,
  Users,
  Wrench,
  LayoutTemplate,
  ChevronLeft,
  LogOut,
  Shield,
  ScrollText,
  HeartPulse,
  Radio,
  Server,
  Brain,
  Puzzle,
} from 'lucide-react';
import { useState, useEffect } from 'react';
import { request } from '@/lib/api/client';
import type { HealthResponse } from '@/lib/api/types';
import { useAuth } from '@/hooks/useAuth';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';
import { cn } from '@/lib/utils';

interface NavItem {
  label: string;
  href: string;
  icon: React.ReactNode;
  /** Roles that can see this item. Omit to show to all authenticated users. */
  requireRoles?: string[];
}

interface NavGroup {
  title: string;
  items: NavItem[];
}

const navGroups: NavGroup[] = [
  {
    title: 'Workspace',
    items: [
      { label: 'Dashboard', href: '/', icon: <LayoutDashboard size={16} /> },
      { label: 'Chat', href: '/chat', icon: <MessageSquare size={16} /> },
      { label: 'Agents', href: '/agents', icon: <Bot size={16} /> },
      { label: 'Templates', href: '/templates', icon: <LayoutTemplate size={16} /> },
      { label: 'Circles', href: '/circles', icon: <Users size={16} /> },
      { label: 'Tools', href: '/tools', icon: <Wrench size={16} /> },
      { label: 'MCP Servers', href: '/mcp-servers', icon: <Puzzle size={16} /> },
      { label: 'Memory', href: '/memory', icon: <Brain size={16} /> },
    ],
  },
  {
    title: 'Automation',
    items: [
      { label: 'Schedules', href: '/schedules', icon: <CalendarClock size={16} /> },
      { label: 'Channels', href: '/channels', icon: <Radio size={16} />, requireRoles: ['admin'] },
    ],
  },
  {
    title: 'Analytics',
    items: [
      { label: 'Insights', href: '/insights', icon: <BarChart3 size={16} /> },
      { label: 'Audit', href: '/audit', icon: <ScrollText size={16} /> },
    ],
  },
  {
    title: 'System',
    items: [
      { label: 'Health', href: '/health', icon: <HeartPulse size={16} /> },
      { label: 'Providers', href: '/providers', icon: <Server size={16} /> },
      {
        label: 'Settings',
        href: '/settings',
        icon: <Settings size={16} />,
        requireRoles: ['admin', 'operator'],
      },
    ],
  },
];

function WsIndicator({
  state,
  onReconnect,
}: {
  state: 'connecting' | 'connected' | 'disconnected' | 'error';
  onReconnect: () => void;
}) {
  const color =
    state === 'connected'
      ? 'text-sera-success'
      : state === 'connecting'
        ? 'text-sera-warning'
        : 'text-sera-error';
  const label =
    state === 'connected' ? 'Connected' : state === 'connecting' ? 'Connecting…' : 'Disconnected';
  const isDown = state === 'disconnected' || state === 'error';

  return (
    <div className="flex items-center gap-2 text-xs">
      <CircleIcon
        size={8}
        className={cn('fill-current', color, state === 'connecting' && 'animate-pulse')}
      />
      <span className="text-sera-text-dim">Live:</span>
      {isDown ? (
        <button
          onClick={onReconnect}
          className={cn(color, 'hover:underline cursor-pointer')}
          title="Click to reconnect"
        >
          {label}
        </button>
      ) : (
        <span className={color}>{label}</span>
      )}
    </div>
  );
}

export function Sidebar() {
  const location = useLocation();
  const { user, roles, logout } = useAuth();
  const [collapsed, setCollapsed] = useState(false);
  const [coreStatus, setCoreStatus] = useState<'checking' | 'online' | 'offline'>('checking');
  const { client: centrifugoClient, connectionState: wsState } = useCentrifugoContext();

  useEffect(() => {
    const check = () => {
      request<HealthResponse>('/health')
        .then(() => setCoreStatus('online'))
        .catch(() => setCoreStatus('offline'));
    };
    check();
    const interval = setInterval(check, 30_000);
    return () => clearInterval(interval);
  }, []);

  const isActive = (href: string) =>
    location.pathname === href || location.pathname.startsWith(href + '/');

  const statusColor =
    coreStatus === 'online'
      ? 'text-sera-success'
      : coreStatus === 'offline'
        ? 'text-sera-error'
        : 'text-sera-warning';

  const statusLabel =
    coreStatus === 'online' ? 'Online' : coreStatus === 'offline' ? 'Offline' : 'Checking…';

  function canSee(item: NavItem): boolean {
    if (!item.requireRoles) return true;
    return item.requireRoles.some((r) => roles.includes(r));
  }

  const primaryRole = roles.includes('admin')
    ? 'Admin'
    : roles.includes('operator')
      ? 'Operator'
      : roles.includes('agent-runner')
        ? 'Agent Runner'
        : roles.includes('viewer')
          ? 'Viewer'
          : null;

  return (
    <aside
      className={cn(
        'flex h-screen flex-col bg-sera-surface border-r border-sera-border flex-shrink-0 transition-all duration-200',
        collapsed ? 'w-14' : 'w-60'
      )}
    >
      {/* Brand */}
      <div className={cn('flex items-center gap-3 px-4 py-5', collapsed && 'justify-center px-2')}>
        <NavLink to="/chat" className="flex items-center gap-3 min-w-0">
          <div className="w-8 h-8 rounded-lg bg-sera-accent flex items-center justify-center flex-shrink-0">
            <span className="text-sera-bg font-bold text-sm">S</span>
          </div>
          {!collapsed && (
            <div className="min-w-0">
              <h1 className="text-sm font-semibold text-sera-text tracking-tight">SERA</h1>
              <p className="text-[10px] text-sera-text-dim truncate">Agentic Platform</p>
            </div>
          )}
        </NavLink>
      </div>

      {/* Navigation */}
      <nav className="flex-1 overflow-y-auto px-2 py-2 space-y-4">
        {navGroups.map((group) => {
          const visibleItems = group.items.filter(canSee);
          if (visibleItems.length === 0) return null;
          return (
            <div key={group.title}>
              {!collapsed && <div className="sera-section-label">{group.title}</div>}
              <div className="space-y-0.5">
                {visibleItems.map((item) => {
                  const active = isActive(item.href);
                  return (
                    <NavLink
                      key={item.href}
                      to={item.href}
                      title={collapsed ? item.label : undefined}
                      className={cn(
                        'flex items-center gap-3 px-3 py-2 rounded-lg text-[13px] font-medium transition-all duration-150',
                        collapsed && 'justify-center px-0',
                        active
                          ? 'bg-sera-accent-soft text-sera-accent border-l-[3px] border-sera-accent'
                          : 'text-sera-text-muted hover:text-sera-text hover:bg-sera-surface-hover border-l-[3px] border-transparent'
                      )}
                    >
                      <span className={active ? 'text-sera-accent' : ''}>{item.icon}</span>
                      {!collapsed && item.label}
                    </NavLink>
                  );
                })}
              </div>
            </div>
          );
        })}
      </nav>

      {/* Footer */}
      <div className="border-t border-sera-border">
        {/* Logged-in operator (hidden when collapsed) */}
        {!collapsed && user && (
          <div className="px-4 pt-3 pb-1">
            <div className="flex items-center gap-2 min-w-0">
              <div className="w-6 h-6 rounded-full bg-sera-accent flex items-center justify-center flex-shrink-0">
                <span className="text-[10px] font-bold text-sera-bg">
                  {(user.name ?? user.email ?? user.sub).charAt(0).toUpperCase()}
                </span>
              </div>
              <div className="min-w-0 flex-1">
                <p className="text-[12px] font-medium text-sera-text truncate">
                  {user.name ?? user.email ?? user.sub}
                </p>
                {primaryRole && (
                  <div className="flex items-center gap-1">
                    <Shield size={9} className="text-sera-accent flex-shrink-0" />
                    <p className="text-[10px] text-sera-text-dim truncate">{primaryRole}</p>
                  </div>
                )}
              </div>
              <button
                onClick={() => {
                  void logout();
                }}
                title="Sign out"
                aria-label="Sign out"
                className="flex-shrink-0 text-sera-text-muted hover:text-sera-error transition-colors"
              >
                <LogOut size={14} />
              </button>
            </div>
          </div>
        )}

        {/* Collapse toggle */}
        <button
          onClick={() => setCollapsed((c) => !c)}
          className="w-full flex items-center justify-center px-4 py-3 text-sera-text-muted hover:text-sera-text hover:bg-sera-surface-hover transition-colors duration-150"
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          <ChevronLeft
            size={16}
            className={cn('transition-transform duration-200', collapsed && 'rotate-180')}
          />
          {!collapsed && <span className="ml-2 text-xs">Collapse</span>}
        </button>

        {!collapsed && (
          <div className="px-5 py-3 space-y-1">
            <div className="flex items-center gap-2 text-xs">
              <CircleIcon size={8} className={cn('fill-current', statusColor)} />
              <span className="text-sera-text-dim">Core:</span>
              <span className={statusColor}>{statusLabel}</span>
            </div>
            <WsIndicator state={wsState} onReconnect={() => centrifugoClient?.connect()} />
          </div>
        )}
      </div>
    </aside>
  );
}
