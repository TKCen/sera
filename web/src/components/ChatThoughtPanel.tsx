import React from 'react';
import {
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
} from 'lucide-react';
import { cn } from '@/lib/utils';

export interface MessageThought {
  timestamp: string;
  stepType: string;
  content: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
}

export interface Message {
  id: string;
  role: 'user' | 'agent';
  content: string;
  thoughts: MessageThought[];
  streaming: boolean;
  createdAt: Date;
}

// ── Thought step icons & colours ─────────────────────────────────────────────

const STEP_ICONS: Record<string, React.ReactNode> = {
  observe: <Eye size={11} />,
  plan: <Map size={11} />,
  act: <Zap size={11} />,
  reflect: <RotateCcw size={11} />,
  'tool-call': <Wrench size={11} />,
  'tool-result': <CheckCircle2 size={11} />,
  reasoning: <Brain size={11} />,
};

const STEP_COLORS: Record<string, string> = {
  observe: 'text-blue-400',
  plan: 'text-amber-400',
  act: 'text-emerald-400',
  reflect: 'text-purple-400',
  'tool-call': 'text-cyan-400',
  'tool-result': 'text-teal-400',
  reasoning: 'text-violet-400',
};

interface ChatThoughtPanelProps {
  msg: Message;
  showThinking: boolean;
  isExpanded: boolean;
  onToggleThoughts: (id: string) => void;
}

export function ChatThoughtPanel({
  msg,
  showThinking,
  isExpanded,
  onToggleThoughts,
}: ChatThoughtPanelProps) {
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
        <span>{msg.streaming ? 'Thinking…' : 'Thought process'}</span>
        <ChevronDown
          size={12}
          className={cn('transition-transform duration-200', isExpanded && 'rotate-180')}
        />
      </button>

      <div
        className={cn(
          'overflow-hidden transition-all duration-300',
          isExpanded ? 'max-h-[1200px] opacity-100 mt-2' : 'max-h-0 opacity-0'
        )}
      >
        <div
          className={cn(
            'pl-3 border-l-2 py-1 space-y-2.5 transition-colors duration-300',
            msg.streaming ? 'border-sera-accent/50' : 'border-sera-border'
          )}
        >
          {msg.thoughts.map((thought, i) => {
            // ── Reasoning block ──────────────────────────────────────────────
            if (thought.stepType === 'reasoning') {
              const isLast = i === msg.thoughts.length - 1;
              return (
                <details
                  key={`${thought.timestamp}-${i}`}
                  className="group animate-in fade-in duration-300"
                  open
                >
                  <summary className="flex items-center gap-1.5 cursor-pointer list-none select-none mb-2">
                    <span
                      className={cn(
                        'text-violet-400 flex-shrink-0',
                        msg.streaming && isLast && 'animate-pulse'
                      )}
                    >
                      <Brain size={11} />
                    </span>
                    <span className="text-[11px] font-semibold text-violet-300">
                      {msg.streaming && isLast ? 'Reasoning…' : 'Reasoning'}
                    </span>
                    <ChevronDown
                      size={10}
                      className="ml-auto text-violet-400/60 transition-transform group-open:rotate-180"
                    />
                  </summary>
                  <div className="relative ml-3">
                    <div className="pl-3 border-l border-violet-400/25 text-[11.5px] text-sera-text-muted leading-relaxed whitespace-pre-wrap max-h-80 overflow-y-auto [scrollbar-width:thin]">
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
                    <span
                      className={cn(
                        'flex-shrink-0',
                        STEP_COLORS['tool-call'] ?? STEP_COLORS['act']
                      )}
                    >
                      {STEP_ICONS['tool-call'] ?? STEP_ICONS['act']}
                    </span>
                    <span className="text-[11px] font-semibold text-cyan-300">{toolName}</span>
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
                      <span className={cn('flex-shrink-0', STEP_COLORS['tool-result'])}>
                        {STEP_ICONS['tool-result']}
                      </span>
                      <span className="text-[11px] font-semibold text-teal-300">
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
                  <span className={cn('mt-0.5 flex-shrink-0', STEP_COLORS['tool-result'])}>
                    {STEP_ICONS['tool-result']}
                  </span>
                  <div className="text-[11px] leading-relaxed min-w-0">
                    <span className="font-semibold text-teal-300">Result: </span>
                    <span className="text-sera-text-muted break-all">
                      {raw.length > 300 ? raw.substring(0, 300) + '…' : raw}
                    </span>
                  </div>
                </div>
              );
            }

            // ── Generic step ─────────────────────────────────────────────────
            return (
              <div
                key={`${thought.timestamp}-${i}`}
                className="flex items-start gap-2 animate-in fade-in slide-in-from-left-2 duration-200"
              >
                <span
                  className={cn(
                    'mt-0.5 flex-shrink-0',
                    STEP_COLORS[thought.stepType] ?? 'text-sera-text-muted'
                  )}
                >
                  {STEP_ICONS[thought.stepType] ?? <Brain size={11} />}
                </span>
                <span className="text-[11px] text-sera-text-muted leading-relaxed">
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
