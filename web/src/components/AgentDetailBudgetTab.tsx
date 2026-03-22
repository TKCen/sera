import { useState } from 'react';
import { toast } from 'sonner';
import { Edit2, Check, X, RotateCw } from 'lucide-react';
import { useAgentBudget, usePatchAgentBudget, useResetAgentBudget } from '@/hooks/useUsage';
import { BudgetBar } from '@/components/BudgetBar';
import { Button } from '@/components/ui/button';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function BudgetTab({ id }: { id: string }) {
  const { data: budget, isLoading, refetch } = useAgentBudget(id);
  const patchBudget = usePatchAgentBudget(id);
  const resetBudget = useResetAgentBudget(id);

  const [editingHour, setEditingHour] = useState(false);
  const [editingDay, setEditingDay] = useState(false);
  const [hourVal, setHourVal] = useState('');
  const [dayVal, setDayVal] = useState('');

  const startEditHour = () => {
    setHourVal(String(budget?.maxLlmTokensPerHour ?? ''));
    setEditingHour(true);
  };

  const startEditDay = () => {
    setDayVal(String(budget?.maxLlmTokensPerDay ?? ''));
    setEditingDay(true);
  };

  const saveHour = async () => {
    const val = hourVal === '' ? null : Number(hourVal);
    try {
      await patchBudget.mutateAsync({ maxLlmTokensPerHour: val });
      toast.success('Hourly limit updated');
    } catch {
      toast.error('Failed to update hourly limit');
    }
    setEditingHour(false);
  };

  const saveDay = async () => {
    const val = dayVal === '' ? null : Number(dayVal);
    try {
      await patchBudget.mutateAsync({ maxLlmTokensPerDay: val });
      toast.success('Daily limit updated');
    } catch {
      toast.error('Failed to update daily limit');
    }
    setEditingDay(false);
  };

  const handleReset = async () => {
    try {
      await resetBudget.mutateAsync();
      toast.success('Budget counters reset');
    } catch {
      toast.error('Failed to reset budget');
    }
  };

  if (isLoading) return <TabLoading />;

  const hourPct = budget?.maxLlmTokensPerHour
    ? (budget.currentHourTokens / budget.maxLlmTokensPerHour) * 100
    : 0;
  const dayPct = budget?.maxLlmTokensPerDay
    ? (budget.currentDayTokens / budget.maxLlmTokensPerDay) * 100
    : 0;
  const exceeded = hourPct >= 100 || dayPct >= 100;

  return (
    <div className="p-6 space-y-6 max-w-xl">
      {exceeded && (
        <div className="px-4 py-3 rounded-lg bg-sera-error/10 border border-sera-error/30 text-sera-error text-sm font-medium">
          Budget exceeded — agent requests are being rejected until the period resets or the budget
          is adjusted.
        </div>
      )}

      <div className="sera-card-static p-5 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-sera-text">Token Budget</h3>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              void handleReset();
            }}
            disabled={resetBudget.isPending}
          >
            <RotateCw size={13} />
            Reset Counters
          </Button>
        </div>

        {/* Hourly */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-sera-text-muted uppercase tracking-wider">
              Hourly Limit
            </span>
            {!editingHour ? (
              <button
                onClick={startEditHour}
                className="flex items-center gap-1 text-xs text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <Edit2 size={11} />
                {budget?.maxLlmTokensPerHour !== undefined
                  ? budget.maxLlmTokensPerHour.toLocaleString()
                  : 'Unlimited'}
              </button>
            ) : (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={hourVal}
                  onChange={(e) => setHourVal(e.target.value)}
                  placeholder="unlimited"
                  className="sera-input text-xs w-32"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void saveHour();
                  }}
                  className="text-sera-success hover:opacity-80"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingHour(false)}
                  className="text-sera-text-dim hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              </div>
            )}
          </div>
          <BudgetBar
            label="This hour"
            current={budget?.currentHourTokens ?? 0}
            limit={budget?.maxLlmTokensPerHour}
          />
        </div>

        {/* Daily */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-sera-text-muted uppercase tracking-wider">
              Daily Limit
            </span>
            {!editingDay ? (
              <button
                onClick={startEditDay}
                className="flex items-center gap-1 text-xs text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <Edit2 size={11} />
                {budget?.maxLlmTokensPerDay !== undefined
                  ? budget.maxLlmTokensPerDay.toLocaleString()
                  : 'Unlimited'}
              </button>
            ) : (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={dayVal}
                  onChange={(e) => setDayVal(e.target.value)}
                  placeholder="unlimited"
                  className="sera-input text-xs w-32"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void saveDay();
                  }}
                  className="text-sera-success hover:opacity-80"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingDay(false)}
                  className="text-sera-text-dim hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              </div>
            )}
          </div>
          <BudgetBar
            label="Today"
            current={budget?.currentDayTokens ?? 0}
            limit={budget?.maxLlmTokensPerDay}
          />
        </div>
      </div>

      <button
        onClick={() => {
          void refetch();
        }}
        className="text-xs text-sera-text-dim hover:text-sera-text transition-colors"
      >
        Refresh usage counters
      </button>
    </div>
  );
}
