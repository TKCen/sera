import { CalendarClock } from 'lucide-react';

export default function SchedulesPage() {
  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Schedules</h1>
          <p className="text-sm text-sera-text-muted mt-1">Automate agent tasks on a recurring basis</p>
        </div>
      </div>

      <div className="flex flex-col items-center justify-center py-24">
        <div className="w-16 h-16 rounded-2xl bg-sera-surface border border-sera-border flex items-center justify-center mb-5">
          <CalendarClock size={28} className="text-sera-text-dim" />
        </div>
        <h2 className="text-lg font-semibold text-sera-text mb-2">Coming soon</h2>
        <p className="text-sm text-sera-text-muted text-center max-w-md">
          Schedule agents to run at specific times or intervals. Set up automated workflows,
          recurring data collection, and periodic reports.
        </p>
      </div>
    </div>
  );
}
