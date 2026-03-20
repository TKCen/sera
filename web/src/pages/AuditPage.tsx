import { useState, useCallback } from 'react';
import { useSearchParams } from 'react-router';
import { ShieldAlert, ChevronDown, ChevronRight, Download, CheckCircle2, XCircle, Filter } from 'lucide-react';
import { useAuditEvents, useVerifyAuditChain } from '@/hooks/useAudit';
import { useAuth } from '@/contexts/AuthContext';
import { getAuditExportUrl } from '@/lib/api/audit';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { Skeleton } from '@/components/ui/skeleton';
import type { AuditEvent } from '@/lib/api/types';

const PAGE_SIZE = 50;

function fmtTs(iso: string): string {
  return new Date(iso).toLocaleString();
}

function EventRow({ event }: { event: AuditEvent }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <>
      <tr
        className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors cursor-pointer"
        onClick={() => setExpanded((e) => !e)}
      >
        <td className="py-3 px-4">
          <div className="flex items-center gap-1">
            {expanded ? <ChevronDown size={12} className="text-sera-text-dim" /> : <ChevronRight size={12} className="text-sera-text-dim" />}
            <span className="text-xs font-mono text-sera-text-dim">{fmtTs(event.timestamp)}</span>
          </div>
        </td>
        <td className="py-3 px-4">
          <span className="text-sm text-sera-text">{event.actorName ?? event.actorId}</span>
          <span className="text-[10px] text-sera-text-dim block">{event.actorType}</span>
        </td>
        <td className="py-3 px-4">
          <span className="text-sm font-mono text-sera-accent">{event.eventType}</span>
        </td>
        <td className="py-3 px-4">
          {event.resourceType && (
            <span className="text-xs text-sera-text-muted">
              {event.resourceType}
              {event.resourceId && <span className="font-mono ml-1 text-sera-text-dim">#{event.resourceId}</span>}
            </span>
          )}
        </td>
        <td className="py-3 px-4">
          <Badge variant={event.status === 'success' ? 'success' : 'error'}>{event.status}</Badge>
        </td>
      </tr>
      {expanded && (
        <tr className="border-b border-sera-border/50 bg-sera-bg/50">
          <td colSpan={5} className="px-8 py-4">
            <pre className="text-xs font-mono text-sera-text leading-relaxed overflow-x-auto whitespace-pre-wrap">
              {JSON.stringify(event.payload ?? {}, null, 2)}
            </pre>
            {event.hash && (
              <p className="text-[10px] text-sera-text-dim mt-2 font-mono">hash: {event.hash}</p>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

export default function AuditPage() {
  const { roles } = useAuth();
  const [searchParams] = useSearchParams();

  const isAdmin = roles.includes('admin');

  // Filters
  const [actorId, setActorId] = useState(searchParams.get('agentId') ?? '');
  const [eventType, setEventType] = useState('');
  const [resourceType, setResourceType] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [search, setSearch] = useState('');
  const [page, setPage] = useState(1);
  const [showFilters, setShowFilters] = useState(false);

  const [verifyResult, setVerifyResult] = useState<{ valid: boolean; brokenAtSequence?: number; checkedCount: number } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportFormat, setExportFormat] = useState<'jsonl' | 'csv'>('jsonl');

  const verify = useVerifyAuditChain();

  const { data, isLoading } = useAuditEvents({
    actorId: actorId || undefined,
    eventType: eventType || undefined,
    resourceType: resourceType || undefined,
    from: from || undefined,
    to: to || undefined,
    search: search || undefined,
    page,
    pageSize: PAGE_SIZE,
  });

  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 1;

  const handleVerify = useCallback(async () => {
    const result = await verify.mutateAsync();
    setVerifyResult(result);
  }, [verify]);

  const handleExport = useCallback(async () => {
    setExporting(true);
    try {
      const API_BASE_URL = import.meta.env.VITE_API_URL ?? '';
      const urlPath = getAuditExportUrl(exportFormat);
      const url = `${API_BASE_URL}/api${urlPath}`;

      const authHeader = (() => {
        const devKey = import.meta.env.VITE_DEV_API_KEY;
        if (devKey) return `Bearer ${devKey}`;
        const t = sessionStorage.getItem('sera_session_token');
        return t ? `Bearer ${t}` : '';
      })();

      const response = await fetch(url, {
        headers: authHeader ? { Authorization: authHeader } : {},
      });

      if (!response.ok || !response.body) {
        throw new Error(`Export failed: ${response.statusText}`);
      }

      const reader = response.body.getReader();
      const chunks: Uint8Array[] = [];
      let totalSize = 0;

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
        totalSize += value.length;
      }

      const merged = new Uint8Array(totalSize);
      let offset = 0;
      for (const chunk of chunks) {
        merged.set(chunk, offset);
        offset += chunk.length;
      }

      const mimeType = exportFormat === 'csv' ? 'text/csv' : 'application/x-ndjson';
      const blob = new Blob([merged], { type: mimeType });
      const blobUrl = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = blobUrl;
      a.download = `audit-export.${exportFormat === 'jsonl' ? 'jsonl' : 'csv'}`;
      a.click();
      URL.revokeObjectURL(blobUrl);
    } catch (err) {
      console.error('Export error', err);
    } finally {
      setExporting(false);
    }
  }, [exportFormat]);

  if (!isAdmin) {
    return (
      <div className="p-8 flex flex-col items-center justify-center gap-4 mt-20">
        <ShieldAlert size={40} className="text-sera-error" />
        <h2 className="text-lg font-semibold text-sera-text">Access Restricted</h2>
        <p className="text-sm text-sera-text-muted text-center max-w-sm">
          The audit log is only accessible to operators with the <strong>admin</strong> role.
          Contact your system administrator if you need access.
        </p>
      </div>
    );
  }

  return (
    <div className="p-8 max-w-7xl mx-auto space-y-6">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Audit Log</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            {data ? `${data.total.toLocaleString()} events` : 'Loading…'}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowFilters((s) => !s)}
            className={`sera-btn-ghost flex items-center gap-1.5 px-3 py-2 text-xs border border-sera-border ${showFilters ? 'text-sera-accent border-sera-accent/40' : ''}`}
          >
            <Filter size={13} />
            Filters
          </button>

          <Button
            size="sm"
            variant="ghost"
            onClick={() => { void handleVerify(); }}
            disabled={verify.isPending}
          >
            {verify.isPending ? <Spinner /> : <CheckCircle2 size={13} />}
            Verify Chain
          </Button>

          <div className="flex items-center gap-1 border border-sera-border rounded-lg overflow-hidden">
            <select
              value={exportFormat}
              onChange={(e) => setExportFormat(e.target.value as 'jsonl' | 'csv')}
              className="bg-sera-surface text-xs text-sera-text-muted px-2 py-2 outline-none"
            >
              <option value="jsonl">JSONL</option>
              <option value="csv">CSV</option>
            </select>
            <button
              onClick={() => { void handleExport(); }}
              disabled={exporting}
              className="flex items-center gap-1 px-3 py-2 text-xs text-sera-text-muted hover:text-sera-text hover:bg-sera-surface-hover transition-colors"
            >
              <Download size={13} />
              {exporting ? 'Exporting…' : 'Export'}
            </button>
          </div>
        </div>
      </div>

      {/* Verify result */}
      {verifyResult && (
        <div className={`flex items-center gap-3 px-4 py-3 rounded-lg border text-sm ${
          verifyResult.valid
            ? 'bg-sera-success/10 border-sera-success/30 text-sera-success'
            : 'bg-sera-error/10 border-sera-error/30 text-sera-error'
        }`}>
          {verifyResult.valid
            ? <CheckCircle2 size={16} />
            : <XCircle size={16} />}
          {verifyResult.valid
            ? `Chain integrity verified — ${verifyResult.checkedCount} events checked`
            : `Chain broken at sequence #${verifyResult.brokenAtSequence}`}
          <button onClick={() => setVerifyResult(null)} className="ml-auto text-current opacity-60 hover:opacity-100">
            <XCircle size={14} />
          </button>
        </div>
      )}

      {/* Filters */}
      {showFilters && (
        <div className="sera-card-static p-4 grid grid-cols-2 lg:grid-cols-3 gap-4">
          <div className="space-y-1">
            <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">Actor / Agent ID</label>
            <input
              type="text"
              value={actorId}
              onChange={(e) => { setActorId(e.target.value); setPage(1); }}
              placeholder="agent-name or user-id"
              className="sera-input text-xs w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">Event Type</label>
            <input
              type="text"
              value={eventType}
              onChange={(e) => { setEventType(e.target.value); setPage(1); }}
              placeholder="e.g. llm.request"
              className="sera-input text-xs w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">Resource Type</label>
            <input
              type="text"
              value={resourceType}
              onChange={(e) => { setResourceType(e.target.value); setPage(1); }}
              placeholder="e.g. agent, task"
              className="sera-input text-xs w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">From</label>
            <input
              type="datetime-local"
              value={from}
              onChange={(e) => { setFrom(e.target.value); setPage(1); }}
              className="sera-input text-xs w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">To</label>
            <input
              type="datetime-local"
              value={to}
              onChange={(e) => { setTo(e.target.value); setPage(1); }}
              className="sera-input text-xs w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">Full-text Search</label>
            <input
              type="text"
              value={search}
              onChange={(e) => { setSearch(e.target.value); setPage(1); }}
              placeholder="action or resource ID"
              className="sera-input text-xs w-full"
            />
          </div>
        </div>
      )}

      {/* Table */}
      <div className="sera-card-static overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                <th className="text-left py-3 px-4">Timestamp</th>
                <th className="text-left py-3 px-4">Actor</th>
                <th className="text-left py-3 px-4">Event Type</th>
                <th className="text-left py-3 px-4">Resource</th>
                <th className="text-left py-3 px-4">Status</th>
              </tr>
            </thead>
            <tbody>
              {isLoading ? (
                Array.from({ length: 8 }).map((_, i) => (
                  <tr key={i} className="border-b border-sera-border/50">
                    {Array.from({ length: 5 }).map((_, j) => (
                      <td key={j} className="py-3 px-4">
                        <Skeleton className="h-4 w-full" />
                      </td>
                    ))}
                  </tr>
                ))
              ) : (data?.events ?? []).length === 0 ? (
                <tr>
                  <td colSpan={5} className="py-12 text-center text-sera-text-dim text-sm">
                    No audit events found.
                  </td>
                </tr>
              ) : (
                (data?.events ?? []).map((ev) => (
                  <EventRow key={ev.id} event={ev} />
                ))
              )}
            </tbody>
          </table>
        </div>

        {/* Pagination */}
        {totalPages > 1 && (
          <div className="flex items-center justify-between px-4 py-3 border-t border-sera-border">
            <span className="text-xs text-sera-text-dim">
              Page {page} of {totalPages} — {data?.total.toLocaleString()} events
            </span>
            <div className="flex items-center gap-1">
              <Button size="sm" variant="ghost" onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={page === 1}>
                Prev
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setPage((p) => Math.min(totalPages, p + 1))} disabled={page === totalPages}>
                Next
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
