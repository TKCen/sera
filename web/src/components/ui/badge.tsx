import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';

const badgeVariants = cva(
  'inline-flex items-center px-2 py-0.5 rounded-md text-[11px] font-medium tracking-wide uppercase',
  {
    variants: {
      variant: {
        default: 'bg-sera-surface-hover text-sera-text-muted',
        accent: 'bg-sera-accent-soft text-sera-accent',
        success: 'bg-sera-success/15 text-sera-success',
        warning: 'bg-sera-warning/15 text-sera-warning',
        error: 'bg-sera-error/15 text-sera-error',
        info: 'bg-sera-info/15 text-sera-info',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge };
