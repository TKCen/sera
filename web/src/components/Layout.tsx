import { NavLink, Outlet } from 'react-router';
import {
  LayoutDashboard,
  MessageSquare,
  Bot,
  Activity,
  Webhook,
  LogOut,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useAuth } from '@/contexts/auth-context';

const NAV = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard, end: true },
  { to: '/chat', label: 'Chat', icon: MessageSquare },
  { to: '/agents', label: 'Agents', icon: Bot },
  { to: '/sessions', label: 'Sessions', icon: Activity },
  { to: '/hooks', label: 'Hooks', icon: Webhook },
];

export function Layout() {
  const { identity, signOut } = useAuth();

  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 flex-col border-r border-zinc-800 bg-zinc-950 p-3">
        <div className="px-2 py-3">
          <div className="text-sm font-semibold tracking-tight">SERA</div>
          <div className="text-xs text-zinc-500">Operator Console</div>
        </div>
        <nav className="mt-2 space-y-0.5">
          {NAV.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-2 rounded px-2 py-1.5 text-sm',
                  isActive
                    ? 'bg-zinc-800 text-zinc-100'
                    : 'text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200',
                )
              }
            >
              <Icon className="h-4 w-4" />
              {label}
            </NavLink>
          ))}
        </nav>
        <div className="mt-auto border-t border-zinc-800 pt-2">
          <div className="px-2 py-1 text-xs text-zinc-500">{identity?.sub ?? '—'}</div>
          <button
            type="button"
            onClick={signOut}
            className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-sm text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200"
          >
            <LogOut className="h-4 w-4" />
            Sign out
          </button>
        </div>
      </aside>
      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>
    </div>
  );
}
