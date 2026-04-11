import { useState, useMemo } from 'react';
import { useAgentLogs } from '@/hooks/useAgents';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function AgentDetailLogsTab({ id }: { id: string }) {
  const { data: logs, isLoading, refetch } = useAgentLogs(id);
  const [levelFilter, setLevelFilter] = useState<string>('');
  const [searchText, setSearchText] = useState('');

  const filteredLogs = useMemo(() => {
    if (!logs) return '';
    const lines = logs.split('\n');
    return lines
      .filter((line) => {
        if (levelFilter) {
          const upper = levelFilter.toUpperCase();
          if (!line.toUpperCase().includes(upper)) return false;
        }
        if (searchText) {
          if (!line.toLowerCase().includes(searchText.toLowerCase())) return false;
        }
        return true;
      })
      .join('\n');
  }, [logs, levelFilter, searchText]);

  return (
    <div className="p-6 flex flex-col gap-3 h-full">
      <div className="flex items-center gap-3 flex-wrap">
        <div className="flex items-center gap-1 border border-sera-border rounded-lg p-0.5">
          {(['', 'error', 'warn', 'info', 'debug'] as const).map((level) => (
            <button
              key={level}
              onClick={() => setLevelFilter(level)}
              className={cn(
                'px-2 py-1 rounded-md text-[11px] font-medium transition-colors',
                levelFilter === level
                  ? level === 'error'
                    ? 'bg-red-500/20 text-red-400'
                    : level === 'warn'
                      ? 'bg-yellow-500/20 text-yellow-400'
                      : 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:text-sera-text'
              )}
            >
              {level || 'All'}
            </button>
          ))}
        </div>
        <input
          type="text"
          placeholder="Search logs…"
          value={searchText}
          onChange={(e) => setSearchText(e.target.value)}
          className="sera-input text-xs flex-1 min-w-[120px] max-w-[240px]"
        />
        <div className="ml-auto flex items-center gap-2">
          <span className="text-xs text-sera-text-muted">Auto-refreshes every 3s</span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              void refetch();
            }}
          >
            Refresh
          </Button>
        </div>
      </div>
      {isLoading ? (
        <TabLoading />
      ) : (
        <pre className="flex-1 sera-card-static p-4 text-xs font-mono text-sera-text leading-relaxed overflow-auto whitespace-pre">
          {filteredLogs || 'No logs.'}
        </pre>
      )}
    </div>
  );
}
