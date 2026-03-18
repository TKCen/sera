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
  AlertCircle
} from 'lucide-react';
import { useState, useEffect } from 'react';

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

  const fetchData = async () => {
    try {
      setLoading(true);
      const [sRes, iRes] = await Promise.all([
        fetch('/api/core/schedules'),
        fetch('/api/core/agents/instances')
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
      alert(err.message);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Are you sure you want to delete this schedule?')) return;
    try {
      const res = await fetch(`/api/core/schedules/${id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error('Failed to delete schedule');
      fetchData();
    } catch (err: any) {
      alert(err.message);
    }
  };

  const toggleStatus = async (schedule: Schedule) => {
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
      alert(err.message);
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

  // Group schedules by agent
  const groupedSchedules = schedules.reduce((acc, s) => {
    if (!acc[s.agent_id]) acc[s.agent_id] = { name: s.agent_name, items: [] };
    acc[s.agent_id].items.push(s);
    return acc;
  }, {} as Record<string, { name: string; items: Schedule[] }>);

  return (
    <div className="p-8 max-w-6xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Schedules</h1>
          <p className="text-sm text-sera-text-muted mt-1">Automate agent tasks on a recurring basis</p>
        </div>
        <button
          onClick={() => {
            setEditingSchedule(null);
            setFormData({ agentId: '', name: '', cron: '', task: '{}' });
            setIsModalOpen(true);
          }}
          className="sera-btn-primary flex items-center gap-2 px-4 py-2 text-sm"
        >
          <Plus size={16} />
          New Schedule
        </button>
      </div>

      {loading && (
        <div className="flex items-center justify-center py-20">
          <RefreshCw size={24} className="animate-spin text-sera-accent" />
        </div>
      )}

      {error && (
        <div className="p-4 mb-6 border border-sera-error/30 bg-sera-error/5 rounded-lg flex items-center gap-3 text-sera-error text-sm">
          <AlertCircle size={18} />
          {error}
        </div>
      )}

      {!loading && schedules.length === 0 && (
        <div className="flex flex-col items-center justify-center py-24 sera-card-static border-dashed">
          <div className="w-16 h-16 rounded-2xl bg-sera-surface border border-sera-border flex items-center justify-center mb-5">
            <CalendarClock size={28} className="text-sera-text-dim" />
          </div>
          <h2 className="text-lg font-semibold text-sera-text mb-2">No schedules found</h2>
          <p className="text-sm text-sera-text-muted text-center max-w-md">
            Set up automated workflows, recurring data collection, and periodic reports by creating your first schedule.
          </p>
        </div>
      )}

      {!loading && Object.entries(groupedSchedules).map(([agentId, group]) => (
        <div key={agentId} className="mb-8">
          <h2 className="text-xs font-semibold uppercase tracking-widest text-sera-text-dim mb-4 flex items-center gap-2">
            <Bot size={14} />
            {group.name}
          </h2>
          <div className="grid gap-4">
            {group.items.map((schedule) => (
              <div key={schedule.id} className="sera-card p-5 flex items-center justify-between group">
                <div className="flex-1">
                  <div className="flex items-center gap-3 mb-1">
                    <h3 className="text-sm font-semibold text-sera-text">{schedule.name}</h3>
                    <span className={`sera-badge-${schedule.status === 'active' ? 'accent' : 'muted'} text-[10px]`}>
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

                <div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={() => toggleStatus(schedule)}
                    className="p-2 text-sera-text-dim hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors"
                    title={schedule.status === 'active' ? 'Pause' : 'Resume'}
                  >
                    {schedule.status === 'active' ? <Pause size={16} /> : <Play size={16} />}
                  </button>
                  <button
                    onClick={() => openEditModal(schedule)}
                    className="p-2 text-sera-text-dim hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors"
                    title="Edit"
                  >
                    <Edit2 size={16} />
                  </button>
                  <button
                    onClick={() => handleDelete(schedule.id)}
                    className="p-2 text-sera-text-dim hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors"
                    title="Delete"
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}

      {/* Create/Edit Modal */}
      {isModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-sera-bg/80 backdrop-blur-sm">
          <div className="sera-card-static w-full max-w-xl p-6 animate-in zoom-in-95 duration-200 overflow-y-auto max-h-[90vh]">
            <h3 className="text-lg font-semibold text-sera-text mb-4">
              {editingSchedule ? 'Edit Schedule' : 'Create New Schedule'}
            </h3>

            <form onSubmit={handleSubmit} className="space-y-4">
              <div>
                <label className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                  Agent Instance
                </label>
                <select
                  required
                  value={formData.agentId}
                  onChange={(e) => setFormData({ ...formData, agentId: e.target.value })}
                  disabled={!!editingSchedule}
                  className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent"
                >
                  <option value="">Select an agent...</option>
                  {instances.map(i => (
                    <option key={i.id} value={i.id}>{i.name} ({i.templateName})</option>
                  ))}
                </select>
              </div>

              <div>
                <label className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                  Schedule Name
                </label>
                <input
                  required
                  type="text"
                  value={formData.name}
                  onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                  placeholder="e.g. Daily Market Summary"
                  className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent"
                />
              </div>

              <div>
                <label className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                  Cron Expression
                </label>
                <input
                  required
                  type="text"
                  value={formData.cron}
                  onChange={(e) => setFormData({ ...formData, cron: e.target.value })}
                  placeholder="e.g. 0 9 * * *"
                  className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent font-mono"
                />
                <p className="text-[10px] text-sera-text-muted mt-1">Standard crontab format (min hour dom month dow)</p>
              </div>

              <div>
                <label className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                  Task Payload (JSON)
                </label>
                <textarea
                  required
                  rows={6}
                  value={formData.task}
                  onChange={(e) => setFormData({ ...formData, task: e.target.value })}
                  className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent font-mono"
                />
              </div>

              <div className="flex items-center gap-3 pt-4">
                <button
                  type="button"
                  onClick={() => setIsModalOpen(false)}
                  className="flex-1 sera-btn-ghost"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="flex-1 sera-btn-primary"
                >
                  {editingSchedule ? 'Save Changes' : 'Create Schedule'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
