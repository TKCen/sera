import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface CopyButtonProps {
  value: string;
  className?: string;
  /** 'default' wraps in a tooltip; 'inline' renders a plain button (for chat messages etc.) */
  variant?: 'default' | 'inline';
}

export function CopyButton({ value, className, variant = 'default' }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  const btn = (
    <button
      onClick={handleCopy}
      className={cn(
        'p-1 rounded text-sera-text-dim hover:text-sera-text transition-colors',
        variant === 'default' && 'hover:bg-sera-surface-hover',
        className
      )}
      title={variant === 'inline' ? 'Copy message' : undefined}
      aria-label={copied ? 'Copied to clipboard' : 'Copy to clipboard'}
    >
      {copied ? <Check size={12} className="text-sera-success" /> : <Copy size={12} />}
    </button>
  );

  if (variant === 'inline') return btn;

  return <Tooltip content={copied ? 'Copied!' : 'Copy to clipboard'}>{btn}</Tooltip>;
}
