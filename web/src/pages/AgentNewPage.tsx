import { Link } from 'react-router';
import { ArrowLeft } from 'lucide-react';
import { AgentForm } from '@/components/AgentForm';

export default function AgentNewPage() {
  return (
    <div className="p-6 max-w-2xl">
      <Link
        to="/agents"
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text mb-6 transition-colors"
      >
        <ArrowLeft size={12} /> Agents
      </Link>
      <div className="sera-page-header">
        <h1 className="sera-page-title">New Agent</h1>
      </div>
      <AgentForm />
    </div>
  );
}
