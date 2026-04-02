import { useState } from 'react';
import { Loader2, CheckCircle2, XCircle, Zap } from 'lucide-react';
import { request } from '@/lib/api/client';

export function TestConnectionButton({ modelName }: { modelName: string }) {
  const [status, setStatus] = useState<'idle' | 'testing' | 'ok' | 'fail'>('idle');

  const handleTest = async () => {
    setStatus('testing');
    try {
      await request<{ ok: boolean }>(`/providers/${encodeURIComponent(modelName)}/test`, {
        method: 'POST',
      });
      setStatus('ok');
    } catch {
      setStatus('fail');
    }
    setTimeout(() => setStatus('idle'), 4000);
  };

  if (status === 'testing')
    return <Loader2 size={12} className="animate-spin text-sera-text-muted" />;
  if (status === 'ok') return <CheckCircle2 size={12} className="text-sera-success" />;
  if (status === 'fail') return <XCircle size={12} className="text-sera-error" />;
  return (
    <button
      onClick={() => void handleTest()}
      className="p-1 text-sera-text-dim hover:text-sera-accent transition-colors"
      title="Test connection"
    >
      <Zap size={12} />
    </button>
  );
}
