'use client';

import { useParams } from 'next/navigation';
import { useState } from 'react';
import { Bot, MessageSquare, BookOpen, Clock, ArrowLeft } from 'lucide-react';
import Link from 'next/link';

type Tab = 'sessions' | 'knowledge';

interface Session {
  id: string;
  title: string;
  lastMessage: string;
  timestamp: string;
  messageCount: number;
}

interface KnowledgeItem {
  id: string;
  title: string;
  type: 'memory' | 'document' | 'archive';
  updated: string;
  summary: string;
}

// Mock data — will be replaced with real API calls
const mockSessions: Session[] = [
  {
    id: 's1',
    title: 'Debugging Docker Build',
    lastMessage: 'The issue was with the module resolution in the Dockerfile…',
    timestamp: '2 hours ago',
    messageCount: 24,
  },
  {
    id: 's2',
    title: 'Architecture Discussion',
    lastMessage: 'I recommend using the event-driven approach for…',
    timestamp: '5 hours ago',
    messageCount: 18,
  },
  {
    id: 's3',
    title: 'Code Review — API Routes',
    lastMessage: 'The error handling looks good, but consider adding…',
    timestamp: 'Yesterday',
    messageCount: 12,
  },
];

const mockKnowledge: KnowledgeItem[] = [
  {
    id: 'k1',
    title: 'Project Architecture',
    type: 'memory',
    updated: '1 hour ago',
    summary: 'Sera uses a monorepo structure with core (Node.js + TypeScript) and web (Next.js) packages.',
  },
  {
    id: 'k2',
    title: 'User Preferences',
    type: 'memory',
    updated: '3 hours ago',
    summary: 'Prefers TypeScript, dark mode, and modern sleek UI design. Uses Docker extensively.',
  },
  {
    id: 'k3',
    title: 'Homelab Docker Setup',
    type: 'document',
    updated: 'Yesterday',
    summary: 'Documentation on the Docker Compose configuration for the homelab environment.',
  },
];

const agentNames: Record<string, { name: string; role: string }> = {
  '1': { name: 'Sera-Primary', role: 'Coordinator' },
  '2': { name: 'Sera-Researcher', role: 'Researcher' },
};

export default function AgentDetailPage() {
  const params = useParams();
  const agentId = params.id as string;
  const [activeTab, setActiveTab] = useState<Tab>('sessions');

  const agent = agentNames[agentId] || { name: `Agent ${agentId}`, role: 'Worker' };

  const tabs: { id: Tab; label: string; icon: React.ReactNode; count: number }[] = [
    { id: 'sessions', label: 'Sessions', icon: <MessageSquare size={15} />, count: mockSessions.length },
    { id: 'knowledge', label: 'Knowledge', icon: <BookOpen size={15} />, count: mockKnowledge.length },
  ];

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Header */}
      <div className="mb-8">
        <Link
          href="/agents"
          className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4"
        >
          <ArrowLeft size={14} />
          Back to Agents
        </Link>

        <div className="flex items-center gap-4">
          <div className="w-12 h-12 rounded-xl bg-sera-accent-soft flex items-center justify-center">
            <Bot size={24} className="text-sera-accent" />
          </div>
          <div>
            <div className="flex items-center gap-3">
              <h1 className="sera-page-title">{agent.name}</h1>
              <span className="sera-badge-success">Running</span>
            </div>
            <p className="text-sm text-sera-text-muted mt-0.5">{agent.role}</p>
          </div>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-sera-border mb-6">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`
              flex items-center gap-2 px-4 py-3 text-sm font-medium
              border-b-2 transition-colors duration-150
              ${activeTab === tab.id
                ? 'border-sera-accent text-sera-accent'
                : 'border-transparent text-sera-text-muted hover:text-sera-text'
              }
            `}
          >
            {tab.icon}
            {tab.label}
            <span className={`
              text-[11px] px-1.5 py-0.5 rounded-md
              ${activeTab === tab.id
                ? 'bg-sera-accent-soft text-sera-accent'
                : 'bg-sera-surface text-sera-text-dim'
              }
            `}>
              {tab.count}
            </span>
          </button>
        ))}
      </div>

      {/* Tab Content */}
      {activeTab === 'sessions' && (
        <div className="space-y-2">
          {mockSessions.map((session) => (
            <div
              key={session.id}
              className="sera-card p-4 cursor-pointer group"
            >
              <div className="flex items-start justify-between">
                <div className="flex-1 min-w-0">
                  <h3 className="text-sm font-medium text-sera-text group-hover:text-sera-accent transition-colors">
                    {session.title}
                  </h3>
                  <p className="text-xs text-sera-text-muted mt-1 truncate">
                    {session.lastMessage}
                  </p>
                </div>
                <div className="flex items-center gap-3 ml-4 flex-shrink-0">
                  <span className="text-[11px] text-sera-text-dim flex items-center gap-1">
                    <MessageSquare size={11} />
                    {session.messageCount}
                  </span>
                  <span className="text-[11px] text-sera-text-dim flex items-center gap-1">
                    <Clock size={11} />
                    {session.timestamp}
                  </span>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {activeTab === 'knowledge' && (
        <div className="space-y-2">
          {mockKnowledge.map((item) => (
            <div
              key={item.id}
              className="sera-card p-4 cursor-pointer group"
            >
              <div className="flex items-start justify-between mb-2">
                <div className="flex items-center gap-2">
                  <h3 className="text-sm font-medium text-sera-text group-hover:text-sera-accent transition-colors">
                    {item.title}
                  </h3>
                  <span className="sera-badge-muted">{item.type}</span>
                </div>
                <span className="text-[11px] text-sera-text-dim flex items-center gap-1">
                  <Clock size={11} />
                  {item.updated}
                </span>
              </div>
              <p className="text-xs text-sera-text-muted leading-relaxed">
                {item.summary}
              </p>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
