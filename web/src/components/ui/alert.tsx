import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';
import { AlertCircle, CheckCircle2, Info, AlertTriangle } from 'lucide-react';

const alertVariants = cva(
  'flex gap-3 items-start rounded-lg border p-4 text-sm',
  {
    variants: {
      variant: {
        info: 'bg-sera-info/10 border-sera-info/30 text-sera-info',
        success: 'bg-sera-success/10 border-sera-success/30 text-sera-success',
        warning: 'bg-sera-warning/10 border-sera-warning/30 text-sera-warning',
        error: 'bg-sera-error/10 border-sera-error/30 text-sera-error',
      },
    },
    defaultVariants: {
      variant: 'info',
    },
  },
);

const icons = {
  info: Info,
  success: CheckCircle2,
  warning: AlertTriangle,
  error: AlertCircle,
};

interface AlertProps extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof alertVariants> {
  title?: string;
}

export function Alert({ className, variant = 'info', title, children, ...props }: AlertProps) {
  const Icon = icons[variant ?? 'info'];
  return (
    <div className={cn(alertVariants({ variant }), className)} role="alert" {...props}>
      <Icon size={16} className="mt-0.5 flex-shrink-0" />
      <div>
        {title && <p className="font-semibold mb-0.5">{title}</p>}
        <div>{children}</div>
      </div>
    </div>
  );
}
