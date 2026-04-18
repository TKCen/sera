import { useEffect, useRef, useState } from 'react';
import {
  Eye,
  Map,
  Zap,
  RotateCcw,
  Wrench,
  CheckCircle2,
  Brain,
  AlertTriangle,
  type LucideIcon,
} from 'lucide-react';
import { cn, formatTime } from '@/lib/utils';
import { getStepMeta } from '@/lib/step-metadata';
import type { ThoughtEvent } from '@/lib/api/types';

interface ThoughtTimelineProps {
  thoughts: ThoughtEvent[];
  className?: string;
}

const STEP_ICONS: Record<string, LucideIcon> = {
  Eye,
  Map,
  Zap,
  RotateCcw,
  Wrench,
  CheckCircle2,
  Brain,
  AlertTriangle,
};

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

      <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 py-2 space-y-1.5 min-h-0">
        {displayed.length === 0 ? (
          <p className="text-xs text-sera-text-dim text-center py-6">
            {thoughts.length === 0 ? 'No thoughts yet' : 'No key events'}
          </p>
        ) : (
          displayed.map((t, i) => {
            const meta = getStepMeta(t.stepType);
            const Icon = STEP_ICONS[meta.icon] ?? Brain;
            return (
              <div
                key={i}
                className={cn(
                  'flex gap-2 p-2 rounded-lg border text-xs',
                  meta.bg,
                  'animate-in fade-in-0 slide-in-from-bottom-1 duration-200'
                )}
              >
                <div className={cn('flex-shrink-0 mt-0.5', meta.color)}>
                  <Icon size={12} />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 mb-0.5">
                    <span
                      className={cn(
                        'font-semibold uppercase tracking-wide text-[10px]',
                        meta.color
                      )}
                    >
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
