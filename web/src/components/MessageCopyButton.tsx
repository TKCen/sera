import { useState } from 'react';
import { Check, Copy } from 'lucide-react';

interface MessageCopyButtonProps {
  text: string;
}

export function MessageCopyButton({ text }: MessageCopyButtonProps) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        });
      }}
      className="p-1 rounded text-sera-text-dim hover:text-sera-text transition-colors"
      title="Copy message"
      aria-label="Copy message"
    >
      {copied ? <Check size={12} className="text-sera-success" /> : <Copy size={12} />}
    </button>
  );
}
