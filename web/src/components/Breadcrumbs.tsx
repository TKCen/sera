import { Link } from 'react-router';
import { ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';

interface BreadcrumbItem {
  label: string;
  href?: string;
}

interface BreadcrumbsProps {
  items: BreadcrumbItem[];
  className?: string;
}

export function Breadcrumbs({ items, className }: BreadcrumbsProps) {
  return (
    <nav aria-label="Breadcrumb" className={cn('mb-4', className)}>
      <ol className="flex items-center gap-1.5 text-xs text-sera-text-muted">
        {items.map((item, index) => {
          const isLast = index === items.length - 1;

          return (
            <li key={index} className="flex items-center gap-1.5">
              {item.href && !isLast ? (
                <Link to={item.href} className="hover:text-sera-text transition-colors">
                  {item.label}
                </Link>
              ) : (
                <span className={cn(isLast && 'text-sera-text font-medium')}>{item.label}</span>
              )}

              {!isLast && <ChevronRight size={12} className="text-sera-text-dim/50" />}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
