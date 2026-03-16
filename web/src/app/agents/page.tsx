'use client';

import { Bot, Plus, Settings as SettingsIcon, Play, Square } from 'lucide-react';
import { useState } from 'react';

interface Agent {
  id: string;
  name: string;
  role: string;
  model: string;
  status: 'running' | 'stopped';
}

interface AgentTemplate {
  id: string;
  name: string;
  category: string;
  description: string;
  categoryColor: string;
}

const mockRunningAgents: Agent[] = [
  { id: '1', name: 'Sera-Primary', role: 'Coordinator', model: 'gemini-3-flash', status: 'running' },
  { id: '2', name: 'Sera-Researcher', role: 'Researcher', model: 'gemini-3-flash', status: 'running' },
];

const agentTemplates: AgentTemplate[] = [
  {
    id: 'general',
    name: 'General Assistant',
    category: 'General',
    description: 'A versatile conversational agent that can help with everyday tasks, answer questions, and provide recommendations.',
    categoryColor: 'bg-sera-accent-soft text-sera-accent',
  },
  {
    id: 'code-helper',
    name: 'Code Helper',
    category: 'Development',
    description: 'A programming-focused agent that writes, reviews, and debugs code across multiple languages.',
    categoryColor: 'bg-purple-500/15 text-purple-400',
  },
  {
    id: 'researcher',
    name: 'Researcher',
    category: 'Research',
    description: 'An analytical agent that breaks down complex topics, synthesizes information, and provides cited summaries.',
    categoryColor: 'bg-blue-500/15 text-blue-400',
  },
  {
    id: 'writer',
    name: 'Writer',
    category: 'Creative',
    description: 'A creative writing agent that helps with drafting, editing, and improving written content of all kinds.',
    categoryColor: 'bg-pink-500/15 text-pink-400',
  },
  {
    id: 'devops',
    name: 'DevOps Engineer',
    category: 'Development',
    description: 'A systems-focused agent for CI/CD, infrastructure, Docker, and deployment troubleshooting.',
    categoryColor: 'bg-purple-500/15 text-purple-400',
  },
  {
    id: 'data-analyst',
    name: 'Data Analyst',
    category: 'Analytics',
    description: 'A data-focused agent that helps analyze datasets, create queries, and interpret statistical results.',
    categoryColor: 'bg-amber-500/15 text-amber-400',
  },
];

export default function AgentsPage() {
  const [agents] = useState<Agent[]>(mockRunningAgents);

  return (
    <div className="p-8 max-w-7xl mx-auto">
      {/* Header */}
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Agents</h1>
          <p className="text-sm text-sera-text-muted mt-1">Manage and monitor your autonomous agents</p>
        </div>
        <button className="sera-btn-primary">
          <Plus size={16} />
          New Agent
        </button>
      </div>

      {/* Running Agents */}
      {agents.length > 0 && (
        <section className="mb-10">
          <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4">
            Your Agents
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {agents.map((agent) => (
              <a
                key={agent.id}
                href={`/agents/${agent.id}`}
                className="sera-card p-4 group cursor-pointer"
              >
                <div className="flex items-start justify-between mb-3">
                  <div className="w-9 h-9 rounded-lg bg-sera-accent-soft flex items-center justify-center">
                    <Bot size={18} className="text-sera-accent" />
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="sera-badge-success">
                      {agent.status === 'running' ? 'Running' : 'Stopped'}
                    </span>
                    <button
                      className="p-1 rounded-md hover:bg-sera-surface-hover transition-colors"
                      onClick={(e) => { e.preventDefault(); }}
                    >
                      <SettingsIcon size={14} className="text-sera-text-dim" />
                    </button>
                  </div>
                </div>
                <h3 className="text-sm font-semibold text-sera-text group-hover:text-sera-accent transition-colors">
                  {agent.name}
                </h3>
                <p className="text-xs text-sera-text-muted mt-1">{agent.role}</p>
                <p className="text-[11px] text-sera-text-dim mt-2 font-mono">{agent.model}</p>
              </a>
            ))}
          </div>
        </section>
      )}

      {/* Agent Templates */}
      <section>
        <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4">
          Start a New Agent
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {agentTemplates.map((template) => (
            <button
              key={template.id}
              className="sera-card p-5 text-left group"
              onClick={() => console.log(`Launch agent: ${template.id}`)}
            >
              <div className="flex items-start justify-between mb-3">
                <h3 className="text-sm font-semibold text-sera-text group-hover:text-sera-accent transition-colors">
                  {template.name}
                </h3>
                <span className={`sera-badge ${template.categoryColor}`}>
                  {template.category}
                </span>
              </div>
              <p className="text-xs text-sera-text-muted leading-relaxed">
                {template.description}
              </p>
              <div className="mt-4 flex items-center gap-1.5 text-xs text-sera-text-dim group-hover:text-sera-accent transition-colors">
                <Play size={12} />
                <span>Launch</span>
              </div>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
