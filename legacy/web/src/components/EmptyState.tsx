import type { ReactNode } from 'react';

interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
}

export function EmptyState({ icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center gap-4 py-16 px-8 text-center">
      {icon && (
        <div className="flex h-14 w-14 items-center justify-center rounded-full bg-sera-surface-active text-sera-text-muted">
          {icon}
        </div>
      )}
      <div className="space-y-1">
        <h3 className="text-base font-semibold text-sera-text">{title}</h3>
        {description && <p className="text-sm text-sera-text-muted max-w-xs">{description}</p>}
      </div>
      {action && <div>{action}</div>}
    </div>
  );
}
