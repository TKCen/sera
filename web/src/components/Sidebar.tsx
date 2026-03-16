'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import {
  MessageSquare,
  Bot,
  CalendarClock,
  BarChart3,
  Settings,
  Plus,
  Circle,
} from 'lucide-react';
import { useState, useEffect } from 'react';

interface NavItem {
  label: string;
  href: string;
  icon: React.ReactNode;
}

interface NavGroup {
  title: string;
  items: NavItem[];
}

const navGroups: NavGroup[] = [
  {
    title: 'Workspace',
    items: [
      { label: 'New Chat', href: '/chat', icon: <Plus size={16} /> },
      { label: 'Agents', href: '/agents', icon: <Bot size={16} /> },
    ],
  },
  {
    title: 'Automation',
    items: [
      { label: 'Schedules', href: '/schedules', icon: <CalendarClock size={16} /> },
    ],
  },
  {
    title: 'Analytics',
    items: [
      { label: 'Insights', href: '/insights', icon: <BarChart3 size={16} /> },
    ],
  },
  {
    title: 'System',
    items: [
      { label: 'Settings', href: '/settings', icon: <Settings size={16} /> },
    ],
  },
];

export default function Sidebar() {
  const pathname = usePathname();
  const [coreStatus, setCoreStatus] = useState<'checking' | 'online' | 'offline'>('checking');

  useEffect(() => {
    fetch('/api/core/health')
      .then(res => res.ok ? setCoreStatus('online') : setCoreStatus('offline'))
      .catch(() => setCoreStatus('offline'));

    const interval = setInterval(() => {
      fetch('/api/core/health')
        .then(res => res.ok ? setCoreStatus('online') : setCoreStatus('offline'))
        .catch(() => setCoreStatus('offline'));
    }, 30000);

    return () => clearInterval(interval);
  }, []);

  const isActive = (href: string) => {
    if (href === '/chat') return pathname === '/chat' || pathname.startsWith('/chat/');
    if (href === '/agents') return pathname === '/agents' || pathname.startsWith('/agents/');
    return pathname === href || pathname.startsWith(href + '/');
  };

  const statusColor = coreStatus === 'online'
    ? 'text-sera-success'
    : coreStatus === 'offline'
      ? 'text-sera-error'
      : 'text-sera-warning';

  const statusLabel = coreStatus === 'online'
    ? 'Online'
    : coreStatus === 'offline'
      ? 'Offline'
      : 'Checking…';

  return (
    <aside className="w-60 h-screen flex flex-col bg-sera-surface border-r border-sera-border flex-shrink-0">
      {/* Brand */}
      <div className="px-5 py-5 flex items-center gap-3">
        <div className="w-8 h-8 rounded-lg bg-sera-accent flex items-center justify-center">
          <span className="text-sera-bg font-bold text-sm">S</span>
        </div>
        <div>
          <h1 className="text-sm font-semibold text-sera-text tracking-tight">SERA</h1>
          <p className="text-[10px] text-sera-text-dim">v1.0 — Agentic Platform</p>
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 overflow-y-auto px-3 py-2 space-y-5">
        {navGroups.map((group) => (
          <div key={group.title}>
            <div className="sera-section-label">{group.title}</div>
            <div className="space-y-0.5">
              {group.items.map((item) => {
                const active = isActive(item.href);
                return (
                  <Link
                    key={item.href}
                    href={item.href}
                    className={`
                      flex items-center gap-3 px-3 py-2 rounded-lg text-[13px] font-medium
                      transition-all duration-150 ease-out
                      ${active
                        ? 'bg-sera-accent-soft text-sera-accent border-l-[3px] border-sera-accent ml-0'
                        : 'text-sera-text-muted hover:text-sera-text hover:bg-sera-surface-hover'
                      }
                    `}
                  >
                    <span className={active ? 'text-sera-accent' : ''}>{item.icon}</span>
                    {item.label}
                  </Link>
                );
              })}
            </div>
          </div>
        ))}
      </nav>

      {/* Footer — Core Status */}
      <div className="px-5 py-4 border-t border-sera-border">
        <div className="flex items-center gap-2 text-xs">
          <Circle size={8} className={`${statusColor} fill-current`} />
          <span className="text-sera-text-dim">Core:</span>
          <span className={statusColor}>{statusLabel}</span>
        </div>
      </div>
    </aside>
  );
}
