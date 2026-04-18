import { useState } from 'react';
import { cn } from '@/lib/utils';

interface CodeBlockProps {
  children?: React.ReactNode;
  className?: string;
}

export function CodeBlock({ children, className }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);
  const code = String(children ?? '').trim();

  function handleCopy() {
    void navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  return (
    <div className="relative group my-2">
      <pre
        className={cn(
          'bg-sera-bg border border-sera-border rounded-lg px-4 py-3 overflow-x-auto text-[0.8em] leading-relaxed',
          className
        )}
      >
        <code>{children}</code>
      </pre>
      <button
        onClick={handleCopy}
        className="absolute top-2 right-2 px-2 py-0.5 rounded text-[10px] bg-sera-surface text-sera-text-muted opacity-0 group-hover:opacity-100 transition-opacity hover:text-sera-text"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
    </div>
  );
}
