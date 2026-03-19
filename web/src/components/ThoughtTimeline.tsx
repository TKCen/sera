import { useEffect, useRef, useState } from 'react';
import { Eye, Map, Zap, RefreshCw, Wrench, CheckCircle, Brain } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { ThoughtEvent } from '@/lib/api/types';

interface ThoughtTimelineProps {
  thoughts: ThoughtEvent[];
  className?: string;
}

type StepType = ThoughtEvent['stepType'];

const STEP_META: Record<StepType, { label: string; icon: React.ReactNode; color: string; bg: string }> = {
  observe: {
    label: 'Observe',
    icon: <Eye size={12} />,
    color: 'text-sera-info',
    bg: 'bg-sera-info/15 border-sera-info/30',
  },
  plan: {
    label: 'Plan',
    icon: <Map size={12} />,
    color: 'text-sera-warning',
    bg: 'bg-sera-warning/15 border-sera-warning/30',
  },
  act: {
    label: 'Act',
    icon: <Zap size={12} />,
    color: 'text-sera-success',
    bg: 'bg-sera-success/15 border-sera-success/30',
  },
  reflect: {
    label: 'Reflect',
    icon: <RefreshCw size={12} />,
    color: 'text-[#c084fc]',
    bg: 'bg-[#c084fc]/15 border-[#c084fc]/30',
  },
  'tool-call': {
    label: 'Tool',
    icon: <Wrench size={12} />,
    color: 'text-sera-warning',
    bg: 'bg-sera-warning/15 border-sera-warning/30',
  },
  'tool-result': {
    label: 'Result',
    icon: <CheckCircle size={12} />,
    color: 'text-sera-success',
    bg: 'bg-sera-success/10 border-sera-success/20',
  },
  reasoning: {
    label: 'Reason',
    icon: <Brain size={12} />,
    color: 'text-sera-text-muted',
    bg: 'bg-sera-surface-hover border-sera-border',
  },
};

function formatTime(ts: string): string {
  const d = new Date(ts);
  return isNaN(d.getTime()) ? '' : d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export function ThoughtTimeline({ thoughts, className }: ThoughtTimelineProps) {
  const [collapsed, setCollapsed] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const userScrolledRef = useRef(false);
  const prevLengthRef = useRef(thoughts.length);

  const displayed = collapsed
    ? thoughts.filter((t) => t.stepType === 'act' || t.stepType === 'tool-call')
    : thoughts;

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    if (thoughts.length !== prevLengthRef.current) {
      prevLengthRef.current = thoughts.length;
      if (!userScrolledRef.current) {
        el.scrollTop = el.scrollHeight;
      }
    }
  }, [thoughts]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    function onScroll() {
      if (!el) return;
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      userScrolledRef.current = !atBottom;
    }

    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  return (
    <div className={cn('flex flex-col h-full', className)}>
      <div className="flex items-center justify-between px-3 py-2 border-b border-sera-border flex-shrink-0">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-sera-text-dim">
          Thoughts
          {thoughts.length > 0 && (
            <span className="ml-1.5 text-sera-text-muted">{thoughts.length}</span>
          )}
        </span>
        <button
          onClick={() => setCollapsed((c) => !c)}
          className="text-[11px] text-sera-text-muted hover:text-sera-text transition-colors"
        >
          {collapsed ? 'All steps' : 'Key events'}
        </button>
      </div>

      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto px-3 py-2 space-y-1.5 min-h-0"
      >
        {displayed.length === 0 ? (
          <p className="text-xs text-sera-text-dim text-center py-6">
            {thoughts.length === 0 ? 'No thoughts yet' : 'No key events'}
          </p>
        ) : (
          displayed.map((t, i) => {
            const meta = STEP_META[t.stepType] ?? STEP_META.reasoning;
            return (
              <div
                key={i}
                className={cn(
                  'flex gap-2 p-2 rounded-lg border text-xs',
                  meta.bg,
                  'animate-in fade-in-0 slide-in-from-bottom-1 duration-200',
                )}
              >
                <div className={cn('flex-shrink-0 mt-0.5', meta.color)}>
                  {meta.icon}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 mb-0.5">
                    <span className={cn('font-semibold uppercase tracking-wide text-[10px]', meta.color)}>
                      {meta.label}
                    </span>
                    {t.timestamp && (
                      <span className="text-sera-text-dim text-[10px]">
                        {formatTime(t.timestamp)}
                      </span>
                    )}
                  </div>
                  <p className="text-sera-text-muted leading-relaxed whitespace-pre-wrap break-words">
                    {t.content}
                  </p>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
