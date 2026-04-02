import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface CopyButtonProps {
  value: string;
  className?: string;
}

export function CopyButton({ value, className }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <Tooltip content={copied ? 'Copied!' : 'Copy to clipboard'}>
      <button
        onClick={handleCopy}
        className={cn(
          'p-1 rounded text-sera-text-dim hover:text-sera-text hover:bg-sera-surface-hover transition-colors',
          className
        )}
        aria-label={copied ? 'Copied to clipboard' : 'Copy to clipboard'}
      >
        {copied ? <Check size={12} className="text-sera-success" /> : <Copy size={12} />}
      </button>
    </Tooltip>
  );
}
