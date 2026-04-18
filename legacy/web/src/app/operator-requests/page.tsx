import { useState } from 'react';
import { toast } from 'sonner';
import {
  Inbox,
  CheckCircle2,
  XCircle,
  Clock,
  AlertCircle,
  ChevronDown,
  ChevronUp,
  MessageSquareReply,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { useOperatorRequests, useRespondToRequest } from '@/hooks/useOperatorRequests';
import type { OperatorRequest } from '@/lib/api/operator-requests';
import { cn } from '@/lib/utils';

type StatusFilter = 'all' | 'pending' | 'approved' | 'rejected' | 'resolved';

const STATUS_TABS: { value: StatusFilter; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'pending', label: 'Pending' },
  { value: 'approved', label: 'Approved' },
  { value: 'rejected', label: 'Rejected' },
  { value: 'resolved', label: 'Resolved' },
];

function StatusBadge({ status }: { status: OperatorRequest['status'] }) {
  const variants: Record<OperatorRequest['status'], { icon: React.ReactNode; className: string }> =
    {
      pending: {
        icon: <Clock size={10} />,
        className: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
      },
      approved: {
        icon: <CheckCircle2 size={10} />,
        className: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30',
      },
      rejected: {
        icon: <XCircle size={10} />,
        className: 'bg-red-500/15 text-red-400 border-red-500/30',
      },
      resolved: {
        icon: <AlertCircle size={10} />,
        className: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
      },
    };
  const v = variants[status];
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium border',
        v.className
      )}
    >
      {v.icon}
      {status}
    </span>
  );
}

function TypeBadge({ type }: { type: string }) {
  return (
    <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-mono font-medium bg-sera-accent/10 text-sera-accent border border-sera-accent/20">
      {type}
    </span>
  );
}

function RequestCard({
  request,
  onRespond,
  isPending,
}: {
  request: OperatorRequest;
  onRespond: (id: string, action: 'approved' | 'rejected' | 'resolved', response?: string) => void;
  isPending: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [showRespond, setShowRespond] = useState(false);
  const [responseText, setResponseText] = useState('');

  const createdAt = new Date(request.createdAt).toLocaleString();

  function handleAction(action: 'approved' | 'rejected' | 'resolved') {
    onRespond(request.id, action, responseText || undefined);
    setShowRespond(false);
    setResponseText('');
  }

  return (
    <div className="sera-card p-5 flex flex-col gap-3">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-3 min-w-0">
          <div className="w-9 h-9 rounded-lg bg-sera-accent/10 flex items-center justify-center flex-shrink-0">
            <Inbox size={16} className="text-sera-accent" />
          </div>
          <div className="min-w-0">
            <h3 className="text-sm font-medium text-sera-text truncate">{request.title}</h3>
            <p className="text-[11px] text-sera-text-muted mt-0.5 truncate">
              {request.agentName ?? request.agentId}
            </p>
          </div>
        </div>
        <StatusBadge status={request.status} />
      </div>

      <div className="flex items-center gap-2 flex-wrap">
        <TypeBadge type={request.type} />
        <span className="text-[11px] text-sera-text-dim">{createdAt}</span>
      </div>

      {/* Payload toggle */}
      <button
        onClick={() => setExpanded((e) => !e)}
        className="flex items-center gap-1 text-[11px] text-sera-text-muted hover:text-sera-text transition-colors"
      >
        {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
        {expanded ? 'Hide payload' : 'Show payload'}
      </button>

      {expanded && (
        <pre className="text-[11px] text-sera-text-muted bg-sera-bg rounded-lg p-3 overflow-x-auto border border-sera-border/40 font-mono">
          {JSON.stringify(request.payload, null, 2)}
        </pre>
      )}

      {request.response && (
        <div className="text-[11px] text-sera-text-muted bg-sera-bg rounded-lg p-3 border border-sera-border/40">
          <span className="text-sera-text-dim font-medium">Response: </span>
          {typeof request.response === 'object'
            ? JSON.stringify(request.response)
            : String(request.response)}
        </div>
      )}

      {/* Actions for pending requests */}
      {request.status === 'pending' && (
        <div className="flex flex-col gap-2 pt-2 border-t border-sera-border/30">
          {showRespond && (
            <textarea
              value={responseText}
              onChange={(e) => setResponseText(e.target.value)}
              placeholder="Optional response message..."
              rows={2}
              className="w-full text-xs bg-sera-bg border border-sera-border rounded-lg px-3 py-2 text-sera-text placeholder:text-sera-text-dim resize-none focus:outline-none focus:ring-1 focus:ring-sera-accent"
            />
          )}
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-[11px] gap-1 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/10"
              disabled={isPending}
              onClick={() => handleAction('approved')}
            >
              <CheckCircle2 size={12} /> Approve
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-[11px] gap-1 text-red-400 hover:text-red-300 hover:bg-red-500/10"
              disabled={isPending}
              onClick={() => handleAction('rejected')}
            >
              <XCircle size={12} /> Reject
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-[11px] gap-1"
              disabled={isPending}
              onClick={() => setShowRespond((s) => !s)}
            >
              <MessageSquareReply size={12} /> Respond
            </Button>
            {showRespond && responseText && (
              <Button
                size="sm"
                variant="ghost"
                className="h-7 text-[11px] gap-1 text-blue-400 hover:text-blue-300 hover:bg-blue-500/10"
                disabled={isPending}
                onClick={() => handleAction('resolved')}
              >
                Send
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function StatBox({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="sera-card-static px-4 py-3 text-center">
      <div className="text-lg font-bold text-sera-text">{value}</div>
      <div className="text-[10px] uppercase tracking-wider text-sera-text-dim">{label}</div>
    </div>
  );
}

export default function OperatorRequestsPage() {
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const { data: requests, isLoading } = useOperatorRequests(
    statusFilter === 'all' ? undefined : statusFilter
  );
  const respond = useRespondToRequest();

  function handleRespond(
    id: string,
    action: 'approved' | 'rejected' | 'resolved',
    response?: string
  ) {
    respond.mutate(
      { id, action, response },
      {
        onSuccess: () => toast.success(`Request ${action}`),
        onError: (err) => toast.error(err instanceof Error ? err.message : 'Action failed'),
      }
    );
  }

  const all = requests ?? [];
  const pendingCount = all.filter((r) => r.status === 'pending').length;

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Operator Requests</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            Requests from SERA agents requiring operator attention.
          </p>
        </div>
      </div>

      {!isLoading && requests && requests.length > 0 && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6 mt-6">
          <StatBox label="Total" value={requests.length} />
          <StatBox label="Pending" value={pendingCount} />
          <StatBox
            label="Approved"
            value={requests.filter((r) => r.status === 'approved').length}
          />
          <StatBox
            label="Rejected"
            value={requests.filter((r) => r.status === 'rejected').length}
          />
        </div>
      )}

      {/* Filter tabs */}
      <div className="flex items-center gap-1 mt-6 mb-4 border-b border-sera-border">
        {STATUS_TABS.map((tab) => (
          <button
            key={tab.value}
            onClick={() => setStatusFilter(tab.value)}
            className={cn(
              'px-3 py-2 text-xs font-medium transition-colors border-b-2 -mb-px',
              statusFilter === tab.value
                ? 'border-sera-accent text-sera-accent'
                : 'border-transparent text-sera-text-muted hover:text-sera-text'
            )}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {isLoading ? (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-48 rounded-xl" />
          ))}
        </div>
      ) : all.length === 0 ? (
        <EmptyState
          icon={<Inbox size={24} />}
          title="No operator requests"
          description="When agents require operator input, their requests will appear here."
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {all.map((req) => (
            <RequestCard
              key={req.id}
              request={req}
              onRespond={handleRespond}
              isPending={respond.isPending}
            />
          ))}
        </div>
      )}
    </div>
  );
}
