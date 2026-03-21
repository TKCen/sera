'use client';

import {
  CalendarClock,
  Plus,
  Play,
  Pause,
  Trash2,
  Edit2,
  RefreshCw,
  Search,
  Bot,
  AlertCircle,
} from 'lucide-react';
import { useState, useEffect, useRef } from 'react';
import { Button } from '@/components/ui/button';
import { Alert } from '@/components/ui/alert';
import { EmptyState } from '@/components/EmptyState';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Card } from '@/components/ui/card';

interface Schedule {
  id: string;
  agent_id: string;
  agent_name: string;
  template_name: string;
  name: string;
  cron: string;
  task: any;
  status: 'active' | 'paused';
  last_run: string | null;
  created_at: string;
}

interface AgentInstance {
  id: string;
  name: string;
  templateName: string;
}

export default function SchedulesPage() {
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [instances, setInstances] = useState<AgentInstance[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingSchedule, setEditingSchedule] = useState<Schedule | null>(null);
  const [formData, setFormData] = useState({
    agentId: '',
    name: '',
    cron: '',
    task: '{}',
  });
  const [formError, setFormError] = useState<string | null>(null);
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({});

  const formErrorRef = useRef<HTMLDivElement>(null);

  const fetchData = async () => {
    try {
      setLoading(true);
      const [sRes, iRes] = await Promise.all([
        fetch('/api/core/schedules'),
        fetch('/api/core/agents/instances'),
      ]);

      if (!sRes.ok) throw new Error('Failed to fetch schedules');
      if (!iRes.ok) throw new Error('Failed to fetch agent instances');

      const sData = await sRes.json();
      const iData = await iRes.json();

      setSchedules(sData);
      setInstances(iData);
      setError(null);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);
    try {
      let taskObj;
      try {
        taskObj = JSON.parse(formData.task);
      } catch {
        throw new Error('Invalid JSON in Task field');
      }

      const url = editingSchedule
        ? `/api/core/schedules/${editingSchedule.id}`
        : '/api/core/schedules';

      const method = editingSchedule ? 'PUT' : 'POST';

      const res = await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          ...formData,
          task: taskObj,
        }),
      });

      if (!res.ok) throw new Error('Failed to save schedule');

      setIsModalOpen(false);
      setEditingSchedule(null);
      setFormData({ agentId: '', name: '', cron: '', task: '{}' });
      fetchData();
    } catch (err: any) {
      setFormError(err.message);
      // Wait for React to render the error before focusing it
      setTimeout(() => {
        formErrorRef.current?.focus();
      }, 0);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Are you sure you want to delete this schedule?')) return;
    setActionErrors((prev) => ({ ...prev, [id]: '' }));
    try {
      const res = await fetch(`/api/core/schedules/${id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error('Failed to delete schedule');
      fetchData();
    } catch (err: any) {
      setActionErrors((prev) => ({ ...prev, [id]: err.message }));
    }
  };

  const toggleStatus = async (schedule: Schedule) => {
    setActionErrors((prev) => ({ ...prev, [schedule.id]: '' }));
    try {
      const newStatus = schedule.status === 'active' ? 'paused' : 'active';
      const res = await fetch(`/api/core/schedules/${schedule.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status: newStatus }),
      });
      if (!res.ok) throw new Error('Failed to update status');
      fetchData();
    } catch (err: any) {
      setActionErrors((prev) => ({ ...prev, [schedule.id]: err.message }));
    }
  };

  const openEditModal = (schedule: Schedule) => {
    setEditingSchedule(schedule);
    setFormData({
      agentId: schedule.agent_id,
      name: schedule.name,
      cron: schedule.cron,
      task: JSON.stringify(schedule.task, null, 2),
    });
    setIsModalOpen(true);
  };

  const resetModal = () => {
    setEditingSchedule(null);
    setFormData({ agentId: '', name: '', cron: '', task: '{}' });
    setFormError(null);
  };

  // Group schedules by agent
  const groupedSchedules = schedules.reduce(
    (acc, s) => {
      if (!acc[s.agent_id]) acc[s.agent_id] = { name: s.agent_name, items: [] };
      acc[s.agent_id].items.push(s);
      return acc;
    },
    {} as Record<string, { name: string; items: Schedule[] }>
  );

  return (
    <main className="p-8 max-w-6xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Schedules</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            Automate agent tasks on a recurring basis
          </p>
        </div>
        <Button
          onClick={() => {
            resetModal();
            setIsModalOpen(true);
          }}
          className="flex items-center gap-2"
        >
          <Plus size={16} />
          New Schedule
        </Button>
      </div>

      <div aria-live="polite" role="status" className="mb-6">
        {loading && (
          <div className="grid gap-4 mt-8">
            <Skeleton className="h-24 w-full" />
            <Skeleton className="h-24 w-full" />
            <Skeleton className="h-24 w-full" />
          </div>
        )}

        {error && (
          <Alert variant="error" className="mb-6">
            {error}
          </Alert>
        )}
      </div>

      {!loading && schedules.length === 0 && !error && (
        <EmptyState
          icon={<CalendarClock size={28} />}
          title="No schedules found"
          description="Set up automated workflows, recurring data collection, and periodic reports by creating your first schedule."
          action={
            <Button
              onClick={() => {
                resetModal();
                setIsModalOpen(true);
              }}
              variant="outline"
              className="mt-4"
            >
              <Plus size={16} className="mr-2" />
              Create Schedule
            </Button>
          }
        />
      )}

      {!loading &&
        Object.entries(groupedSchedules).map(([agentId, group]) => (
          <section key={agentId} className="mb-8">
            <h2 className="text-xs font-semibold uppercase tracking-widest text-sera-text-dim mb-4 flex items-center gap-2">
              <Bot size={14} />
              {group.name}
            </h2>
            <ul className="grid gap-4">
              {group.items.map((schedule) => (
                <li key={schedule.id}>
                  <Card className="p-5 group">
                   <div className="flex items-center justify-between">
                    <div className="flex-1">
                      <div className="flex items-center gap-3 mb-1">
                        <h3 className="text-sm font-semibold text-sera-text">{schedule.name}</h3>
                        <span
                          className={`sera-badge-${schedule.status === 'active' ? 'accent' : 'muted'} text-[10px]`}
                        >
                          {schedule.status}
                        </span>
                      </div>
                      <div className="flex items-center gap-4 text-xs text-sera-text-muted font-mono">
                        <span className="flex items-center gap-1">
                          <CalendarClock size={12} />
                          {schedule.cron}
                        </span>
                        {schedule.last_run && (
                          <span>Last run: {new Date(schedule.last_run).toLocaleString()}</span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 opacity-0 focus-within:opacity-100 group-hover:opacity-100 transition-opacity">
                      <button
                        onClick={() => toggleStatus(schedule)}
                        className="p-2 text-sera-text-dim hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent"
                        aria-label={schedule.status === 'active' ? 'Pause schedule' : 'Resume schedule'}
                        title={schedule.status === 'active' ? 'Pause' : 'Resume'}
                      >
                        {schedule.status === 'active' ? <Pause size={16} /> : <Play size={16} />}
                      </button>
                      <button
                        onClick={() => openEditModal(schedule)}
                        className="p-2 text-sera-text-dim hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent"
                        aria-label="Edit schedule"
                        title="Edit"
                      >
                        <Edit2 size={16} />
                      </button>
                      <button
                        onClick={() => handleDelete(schedule.id)}
                        className="p-2 text-sera-text-dim hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent"
                        aria-label="Delete schedule"
                        title="Delete"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                   </div>
                   {actionErrors[schedule.id] && (
                     <Alert variant="error" className="mt-4">
                       {actionErrors[schedule.id]}
                     </Alert>
                   )}
                  </Card>
                </li>
              ))}
            </ul>
          </section>
        ))}

      {/* Create/Edit Modal */}
      <Dialog open={isModalOpen} onOpenChange={setIsModalOpen}>
        <DialogContent className="max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{editingSchedule ? 'Edit Schedule' : 'Create New Schedule'}</DialogTitle>
            <DialogDescription>
              Configure the automated task schedule.
            </DialogDescription>
          </DialogHeader>

          {formError && (
            <div ref={formErrorRef} tabIndex={-1} className="outline-none">
              <Alert variant="error" className="mb-4">
                {formError}
              </Alert>
            </div>
          )}

          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label htmlFor="agentId" className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                Agent Instance
              </label>
              <select
                id="agentId"
                required
                value={formData.agentId}
                onChange={(e) => setFormData({ ...formData, agentId: e.target.value })}
                disabled={!!editingSchedule}
                className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent focus-visible:ring-2 focus-visible:ring-sera-accent"
              >
                <option value="">Select an agent...</option>
                {instances.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name} ({i.templateName})
                  </option>
                ))}
              </select>
            </div>

            <div>
              <label htmlFor="scheduleName" className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                Schedule Name
              </label>
              <input
                id="scheduleName"
                required
                type="text"
                value={formData.name}
                onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                placeholder="e.g. Daily Market Summary"
                className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent focus-visible:ring-2 focus-visible:ring-sera-accent"
              />
            </div>

            <div>
              <label htmlFor="cron" className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                Cron Expression
              </label>
              <input
                id="cron"
                required
                type="text"
                value={formData.cron}
                onChange={(e) => setFormData({ ...formData, cron: e.target.value })}
                placeholder="e.g. 0 9 * * *"
                aria-describedby="cron-help"
                className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent focus-visible:ring-2 focus-visible:ring-sera-accent font-mono"
              />
              <p id="cron-help" className="text-[10px] text-sera-text-muted mt-1">
                Standard crontab format (min hour dom month dow)
              </p>
            </div>

            <div>
              <label htmlFor="taskPayload" className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                Task Payload (JSON)
              </label>
              <textarea
                id="taskPayload"
                required
                rows={6}
                value={formData.task}
                onChange={(e) => setFormData({ ...formData, task: e.target.value })}
                className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent focus-visible:ring-2 focus-visible:ring-sera-accent font-mono"
              />
            </div>

            <div className="flex items-center gap-3 pt-4">
              <Button
                type="button"
                variant="ghost"
                onClick={() => setIsModalOpen(false)}
                className="flex-1"
              >
                Cancel
              </Button>
              <Button type="submit" variant="primary" className="flex-1">
                {editingSchedule ? 'Save Changes' : 'Create Schedule'}
              </Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>
    </main>
  );
}
