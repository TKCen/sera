import React, { useMemo } from 'react';
import {
  AlertTriangle,
  ChevronDown,
  Sparkles,
  Brain,
  Loader2,
  Eye,
  Map,
  Zap,
  RotateCcw,
  Wrench,
  CheckCircle2,
  type LucideIcon,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { getStepMeta } from '@/lib/step-metadata';

import type { Message, MessageThought } from '@/lib/api/types';
export type { Message, MessageThought };

// ── Icon lookup (maps metadata icon name → component) ────────────────────────

const ICON_MAP: Record<string, LucideIcon> = {
  Eye,
  Map,
  Zap,
  RotateCcw,
  Wrench,
  CheckCircle2,
  Brain,
  AlertTriangle,
};

/** Derive iteration count from observe/reflect cycles in the thought array */
function getIterationInfo(thoughts: MessageThought[]): { current: number; label: string } | null {
  const observeCount = thoughts.filter(
    (t) => t.stepType === 'observe' || t.stepType === 'reflect'
  ).length;
  if (observeCount === 0) return null;
  // Each full cycle is observe→plan→act→reflect; count observe steps as iteration number
  const iterations = thoughts.filter((t) => t.stepType === 'observe').length;
  return { current: iterations, label: `iteration ${iterations}` };
}

/** Resolve step icon component from metadata */
function stepIcon(stepType: string, size = 11): React.ReactNode {
  const meta = getStepMeta(stepType);
  const Icon = ICON_MAP[meta.icon] ?? Brain;
  return <Icon size={size} />;
}

/** Resolve step text-color class from metadata */
function stepColor(stepType: string): string {
  return getStepMeta(stepType).color;
}

interface ChatThoughtPanelProps {
  msg: Message;
  showThinking: boolean;
  isExpanded: boolean;
  onToggleThoughts: (id: string) => void;
}

// Context assembly events that should be collapsed into a single summary line
const CONTEXT_EVENTS = new Set([
  'context.assembly_started',
  'context.assembly_completed',
  'context.circle_constitution_skipped',
  'context.circle_constitution_injected',
  'context.skills_injected',
  'context.tools_resolved',
  'context.memory_retrieved',
  'context.token_budget',
]);

type ContextSummaryStep = { type: 'context_summary'; count: number; key: string };
type DisplayStep = MessageThought | ContextSummaryStep;

function isContextEvent(thought: MessageThought): boolean {
  return CONTEXT_EVENTS.has(thought.stepType) || CONTEXT_EVENTS.has(thought.content ?? '');
}

export function ChatThoughtPanel({
  msg,
  showThinking,
  isExpanded,
  onToggleThoughts,
}: ChatThoughtPanelProps) {
  // Collapse context assembly events into a single summary line
  const displaySteps = useMemo((): DisplayStep[] => {
    const result: DisplayStep[] = [];
    let contextGroup: MessageThought[] = [];
    let contextGroupStartKey = '';

    for (let i = 0; i < msg.thoughts.length; i++) {
      const thought = msg.thoughts[i]!;
      if (isContextEvent(thought)) {
        if (contextGroup.length === 0) contextGroupStartKey = `${thought.timestamp}-${i}`;
        contextGroup.push(thought);
      } else {
        if (contextGroup.length > 0) {
          result.push({
            type: 'context_summary',
            count: contextGroup.length,
            key: contextGroupStartKey,
          });
          contextGroup = [];
        }
        result.push(thought);
      }
    }
    if (contextGroup.length > 0) {
      result.push({
        type: 'context_summary',
        count: contextGroup.length,
        key: contextGroupStartKey,
      });
    }
    return result;
  }, [msg.thoughts]);

  if (msg.role !== 'agent') return null;
  if (!showThinking) return null;
  if (msg.thoughts.length === 0 && !msg.streaming) return null;

  return (
    <div className="mb-2">
      <button
        onClick={() => onToggleThoughts(msg.id)}
        className={cn(
          'flex items-center gap-1.5 text-[12px] font-medium transition-colors duration-200',
          msg.streaming && msg.thoughts.length > 0
            ? 'text-sera-accent'
            : 'text-sera-text-muted hover:text-sera-text'
        )}
      >
        <Sparkles
          size={13}
          className={
            msg.streaming && msg.thoughts.length > 0 ? 'animate-pulse text-sera-accent' : ''
          }
        />
        <span>
          {msg.streaming
            ? `Thinking…${(() => {
                const iter = getIterationInfo(msg.thoughts);
                return iter ? ` (${iter.label})` : '';
              })()}`
            : `Thought process (${msg.thoughts.length} steps)`}
        </span>
        <ChevronDown
          size={12}
          className={cn('transition-transform duration-200', isExpanded && 'rotate-180')}
        />
      </button>

      <div
        className={cn(
          'overflow-hidden transition-all duration-300',
          isExpanded ? 'max-h-[400px] opacity-100 mt-2 overflow-y-auto' : 'max-h-0 opacity-0'
        )}
      >
        <div
          className={cn(
            'pl-3 border-l-2 py-1 space-y-2.5 transition-colors duration-300',
            msg.streaming ? 'border-sera-accent/50' : 'border-sera-border'
          )}
        >
          {displaySteps.map((step, i) => {
            // ── Context summary block ────────────────────────────────────────
            if ('type' in step && step.type === 'context_summary') {
              return (
                <div
                  key={step.key}
                  className="flex items-center gap-2 text-xs text-sera-text-muted py-0.5 animate-in fade-in duration-200"
                >
                  <span className="text-sera-text-dim flex-shrink-0">⚙</span>
                  <span>
                    Context prepared ({step.count} step{step.count !== 1 ? 's' : ''})
                  </span>
                </div>
              );
            }

            const thought = step as MessageThought;

            // ── Reasoning block ──────────────────────────────────────────────
            if (thought.stepType === 'reasoning') {
              const isLast = i === displaySteps.length - 1;
              const reasonColor = stepColor('reasoning');
              return (
                <details
                  key={`${thought.timestamp}-${i}`}
                  className="group animate-in fade-in duration-300"
                  open
                >
                  <summary className="flex items-center gap-1.5 cursor-pointer list-none select-none mb-2">
                    <span
                      className={cn(
                        reasonColor,
                        'flex-shrink-0',
                        msg.streaming && isLast && 'animate-pulse'
                      )}
                    >
                      {stepIcon('reasoning')}
                    </span>
                    <span className={cn('text-[11px] font-semibold', reasonColor)}>
                      {msg.streaming && isLast ? 'Reasoning…' : 'Reasoning'}
                    </span>
                    <ChevronDown
                      size={10}
                      className="ml-auto text-sera-text-dim transition-transform group-open:rotate-180"
                    />
                  </summary>
                  <div className="relative ml-3">
                    <div className="pl-3 border-l border-sera-border text-[11.5px] text-sera-text-muted leading-relaxed whitespace-pre-wrap max-h-80 overflow-y-auto [scrollbar-width:thin]">
                      {thought.content}
                    </div>
                    <div className="absolute bottom-0 left-3 right-0 h-6 bg-gradient-to-t from-sera-surface to-transparent pointer-events-none" />
                  </div>
                </details>
              );
            }

            // ── Tool-call block ──────────────────────────────────────────────
            if (thought.stepType === 'tool-call' || thought.stepType === 'act') {
              // Prefer structured fields; fall back to parsing from content
              const toolName =
                thought.toolName ??
                (thought.content.split('\n')[0] ?? '')
                  .replace(/^(Tool|Calling tool):\s*/, '')
                  .replace(/\(.*/, '')
                  .trim();
              let paramDisplay = '';
              if (thought.toolArgs) {
                paramDisplay = JSON.stringify(thought.toolArgs, null, 2);
              } else {
                const lines = thought.content.split('\n');
                const rawParams = lines
                  .slice(1)
                  .join('\n')
                  .replace(/^Parameters:\s*/, '');
                try {
                  paramDisplay = JSON.stringify(JSON.parse(rawParams), null, 2);
                } catch {
                  paramDisplay = rawParams;
                }
              }
              return (
                <div
                  key={`${thought.timestamp}-${i}`}
                  className="animate-in fade-in slide-in-from-left-2 duration-200"
                >
                  <div className="flex items-center gap-1.5 mb-1">
                    <span className={cn('flex-shrink-0', stepColor('tool-call'))}>
                      {stepIcon('tool-call')}
                    </span>
                    <span className={cn('text-[11px] font-semibold', stepColor('tool-call'))}>
                      {toolName}
                    </span>
                  </div>
                  {paramDisplay && (
                    <pre className="ml-4 text-[10.5px] text-sera-text-muted leading-relaxed bg-sera-bg/60 border border-sera-border rounded px-2 py-1.5 overflow-x-auto whitespace-pre-wrap break-all [scrollbar-width:thin]">
                      {paramDisplay}
                    </pre>
                  )}
                </div>
              );
            }

            // ── Tool-result block ────────────────────────────────────────────
            if (thought.stepType === 'tool-result') {
              const raw = thought.content.startsWith('Result: ')
                ? thought.content.substring(8)
                : thought.content;

              type SearchResult = { title: string; url: string; text: string };
              let parsedResults: SearchResult[] | null = null;
              try {
                const parsed: unknown = JSON.parse(raw);
                if (
                  Array.isArray(parsed) &&
                  parsed.length > 0 &&
                  typeof parsed[0] === 'object' &&
                  parsed[0] !== null &&
                  'title' in parsed[0]
                ) {
                  parsedResults = parsed as SearchResult[];
                }
              } catch {
                /* not JSON */
              }

              if (parsedResults) {
                return (
                  <div
                    key={`${thought.timestamp}-${i}`}
                    className="animate-in fade-in slide-in-from-left-2 duration-200"
                  >
                    <div className="flex items-center gap-1.5 mb-1.5">
                      <span className={cn('flex-shrink-0', stepColor('tool-result'))}>
                        {stepIcon('tool-result')}
                      </span>
                      <span className={cn('text-[11px] font-semibold', stepColor('tool-result'))}>
                        {parsedResults.length} result{parsedResults.length !== 1 ? 's' : ''} fetched
                      </span>
                    </div>
                    <div className="ml-4 space-y-1.5">
                      {parsedResults.map((r, ri) => (
                        <div key={ri}>
                          <a
                            href={r.url}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-[11px] text-sera-accent hover:underline font-medium leading-tight block truncate"
                            title={r.url}
                          >
                            {r.title}
                          </a>
                          {r.text && r.text !== r.title && (
                            <p className="text-[10.5px] text-sera-text-muted leading-snug mt-0.5 line-clamp-2">
                              {r.text}
                            </p>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                );
              }

              return (
                <div
                  key={`${thought.timestamp}-${i}`}
                  className="flex items-start gap-2 animate-in fade-in slide-in-from-left-2 duration-200"
                >
                  <span className={cn('mt-0.5 flex-shrink-0', stepColor('tool-result'))}>
                    {stepIcon('tool-result')}
                  </span>
                  <div className="text-[11px] leading-relaxed min-w-0">
                    <span className={cn('font-semibold', stepColor('tool-result'))}>Result: </span>
                    <span className="text-sera-text-muted break-all">
                      {raw.length > 300 ? raw.substring(0, 300) + '…' : raw}
                    </span>
                  </div>
                </div>
              );
            }

            // ── Error block ──────────────────────────────────────────────────
            if (thought.stepType === 'error') {
              const errMeta = getStepMeta('error');
              return (
                <div
                  key={`${thought.timestamp}-${i}`}
                  className={cn(
                    'animate-in fade-in slide-in-from-left-2 duration-200 rounded-md px-3 py-2',
                    errMeta.bg
                  )}
                >
                  <div className="flex items-center gap-1.5">
                    <span className={cn(errMeta.color, 'flex-shrink-0')}>{stepIcon('error')}</span>
                    <span className={cn('text-[11px] font-semibold', errMeta.color)}>Error</span>
                  </div>
                  <p className="text-[11px] text-sera-error/80 mt-1 ml-4 leading-relaxed">
                    {thought.content}
                  </p>
                </div>
              );
            }

            // ── Generic step ─────────────────────────────────────────────────
            return (
              <div
                key={`${thought.timestamp}-${i}`}
                className="flex items-start gap-2 animate-in fade-in slide-in-from-left-2 duration-200"
              >
                <span className={cn('mt-0.5 flex-shrink-0', stepColor(thought.stepType))}>
                  {stepIcon(thought.stepType)}
                </span>
                <span className="text-[11px] text-sera-text-muted leading-relaxed flex-1">
                  {thought.content}
                </span>
              </div>
            );
          })}

          {msg.streaming && msg.thoughts.length === 0 && (
            <div className="flex items-center gap-2">
              <Loader2 size={11} className="animate-spin text-sera-accent" />
              <span className="text-[11px] text-sera-text-muted">Waiting for agent thoughts…</span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
