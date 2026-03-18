'use client';

import { useState, useEffect, useRef } from 'react';
import {
  Activity,
  MessageSquare,
  Terminal,
  Brain,
  Hash,
  User,
  Clock,
  ChevronRight,
  RefreshCw,
  Shield
} from 'lucide-react';
import { subscribeToChannel } from '../../lib/centrifugo';

interface AgentTemplate {
  name: string;
  displayName: string;
  role: string;
  tier: number;
  icon: string;
}

interface AgentChannels {
  thoughts: string;
  terminal: string;
  publishChannels: string[];
  subscribeChannels: string[];
  dmPeers: string[];
}

interface IntercomMessage {
  id: string;
  timestamp: string;
  source: {
    agent: string;
    circle: string;
  };
  type: string;
  payload: unknown;
  metadata: {
    securityTier: number;
  };
}

interface ThoughtEvent {
  timestamp: string;
  stepType: string;
  content: string;
  agentDisplayName: string;
}

export default function IntercomMonitorPage() {
  const [agents, setAgents] = useState<AgentTemplate[]>([]);
  const [selectedAgent, setSelectedAgent] = useState<AgentTemplate | null>(null);
  const [channels, setChannels] = useState<AgentChannels | null>(null);
  const [selectedChannel, setSelectedChannel] = useState<string | null>(null);
  const [messages, setMessages] = useState<unknown[]>([]);
  const [loading, setLoading] = useState(true);
  const [historyLoading, setHistoryLoading] = useState(false);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const unsubscribeRef = useRef<(() => void) | null>(null);

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Fetch agents on mount
  useEffect(() => {
    const fetchAgents = async () => {
      try {
        setLoading(true);
        const res = await fetch('/api/core/agents/templates');
        if (res.ok) {
          const data = await res.json();
          setAgents(data);
          if (data.length > 0) {
            setSelectedAgent(data[0]);
          }
        }
      } catch (err) {
        console.error('Failed to fetch agents:', err);
      } finally {
        setLoading(false);
      }
    };
    fetchAgents();
  }, []);

  // Fetch channels when selected agent changes
  useEffect(() => {
    if (!selectedAgent) return;

    const fetchChannels = async () => {
      try {
        const res = await fetch(`/api/core/intercom/channels?agent=${selectedAgent.name}`);
        if (res.ok) {
          const data = await res.json();
          setChannels(data);
          // Select thoughts by default
          setSelectedChannel(data.thoughts);
        }
      } catch (err) {
        console.error('Failed to fetch channels:', err);
      }
    };
    fetchChannels();
  }, [selectedAgent]);

  // Handle channel selection (History + Real-time)
  useEffect(() => {
    if (!selectedChannel) return;

    // Cleanup previous subscription
    if (unsubscribeRef.current) {
      unsubscribeRef.current();
      unsubscribeRef.current = null;
    }

    const fetchHistoryAndSubscribe = async () => {
      setHistoryLoading(true);
      setMessages([]);
      try {
        // 1. Fetch history
        const res = await fetch(`/api/core/intercom/history?channel=${selectedChannel}`);
        if (res.ok) {
          const data = await res.json();
          setMessages(data.messages || []);
        }

        // 2. Subscribe to real-time
        unsubscribeRef.current = subscribeToChannel(selectedChannel, (msg: unknown) => {
          setMessages(prev => [...prev, msg]);
        });
      } catch (err) {
        console.error('Failed to setup channel monitor:', err);
      } finally {
        setHistoryLoading(false);
      }
    };

    fetchHistoryAndSubscribe();

    return () => {
      if (unsubscribeRef.current) {
        unsubscribeRef.current();
        unsubscribeRef.current = null;
      }
    };
  }, [selectedChannel]);

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <RefreshCw size={24} className="animate-spin text-sera-accent" />
      </div>
    );
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Sidebar: Agents & Channels */}
      <div className="w-80 border-r border-sera-border bg-sera-bg flex flex-col flex-shrink-0">
        <div className="p-4 border-b border-sera-border">
          <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3 flex items-center gap-2">
            <User size={14} />
            Agent Monitor
          </h2>
          <select
            value={selectedAgent?.name || ''}
            onChange={(e) => {
              const agent = agents.find(a => a.name === e.target.value);
              if (agent) setSelectedAgent(agent);
            }}
            className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent"
          >
            {agents.map(agent => (
              <option key={agent.name} value={agent.name}>{agent.displayName}</option>
            ))}
          </select>
        </div>

        <div className="flex-1 overflow-y-auto p-2">
          {channels && (
            <div className="space-y-6">
              {/* System Channels */}
              <div>
                <h3 className="px-2 text-[10px] font-bold uppercase tracking-wider text-sera-text-dim mb-2">Core Streams</h3>
                <div className="space-y-0.5">
                  <ChannelItem
                    label="Thought Stream"
                    icon={<Brain size={14} />}
                    channel={channels.thoughts}
                    selected={selectedChannel === channels.thoughts}
                    onClick={() => setSelectedChannel(channels.thoughts)}
                  />
                  <ChannelItem
                    label="Terminal Output"
                    icon={<Terminal size={14} />}
                    channel={channels.terminal}
                    selected={selectedChannel === channels.terminal}
                    onClick={() => setSelectedChannel(channels.terminal)}
                  />
                </div>
              </div>

              {/* Pub/Sub Channels */}
              {(channels.publishChannels.length > 0 || channels.subscribeChannels.length > 0) && (
                <div>
                  <h3 className="px-2 text-[10px] font-bold uppercase tracking-wider text-sera-text-dim mb-2">Circle Channels</h3>
                  <div className="space-y-0.5">
                    {Array.from(new Set([...channels.publishChannels, ...channels.subscribeChannels])).map(ch => (
                      <ChannelItem
                        key={ch}
                        label={ch.split(':').pop() || ch}
                        icon={<Hash size={14} />}
                        channel={ch}
                        selected={selectedChannel === ch}
                        onClick={() => setSelectedChannel(ch)}
                      />
                    ))}
                  </div>
                </div>
              )}

              {/* DM Channels */}
              {channels.dmPeers.length > 0 && (
                <div>
                  <h3 className="px-2 text-[10px] font-bold uppercase tracking-wider text-sera-text-dim mb-2">Direct Messages</h3>
                  <div className="space-y-0.5">
                    {channels.dmPeers.map(ch => (
                      <ChannelItem
                        key={ch}
                        label={ch.split(':').pop() || ch}
                        icon={<MessageSquare size={14} />}
                        channel={ch}
                        selected={selectedChannel === ch}
                        onClick={() => setSelectedChannel(ch)}
                      />
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Main Area: Message Flow */}
      <div className="flex-1 flex flex-col bg-sera-bg-dark overflow-hidden">
        <div className="h-14 border-b border-sera-border bg-sera-bg flex items-center justify-between px-6 flex-shrink-0">
          <div className="flex items-center gap-3">
            <Activity size={18} className="text-sera-accent animate-pulse" />
            <div>
              <h1 className="text-sm font-semibold text-sera-text">Channel Monitor</h1>
              <p className="text-[10px] font-mono text-sera-text-dim">{selectedChannel}</p>
            </div>
          </div>
          <div className="flex items-center gap-4">
            {historyLoading && <RefreshCw size={14} className="animate-spin text-sera-text-dim" />}
            <span className="text-[10px] bg-sera-accent/10 text-sera-accent px-2 py-0.5 rounded-full font-bold uppercase tracking-tighter">
              Live Flow
            </span>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          {messages.length === 0 && !historyLoading ? (
            <div className="flex flex-col items-center justify-center h-full text-sera-text-dim space-y-4">
              <div className="w-16 h-16 rounded-full bg-sera-surface flex items-center justify-center">
                <Activity size={32} className="opacity-20" />
              </div>
              <p className="text-sm">No activity detected on this channel yet.</p>
            </div>
          ) : (
            messages.map((msg, i) => (
              <MessageCard key={msg.id || i} message={msg} />
            ))
          )}
          <div ref={messagesEndRef} />
        </div>
      </div>
    </div>
  );
}

function ChannelItem({ label, icon, selected, onClick }: {
  label: string,
  icon: React.ReactNode,
  channel: string,
  selected: boolean,
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      className={`
        w-full flex items-center gap-3 px-3 py-2 rounded-lg text-xs font-medium transition-all
        ${selected
          ? 'bg-sera-accent/10 text-sera-accent'
          : 'text-sera-text-muted hover:bg-sera-surface hover:text-sera-text'
        }
      `}
    >
      <span className={selected ? 'text-sera-accent' : 'text-sera-text-dim'}>
        {icon}
      </span>
      <span className="flex-1 text-left truncate">{label}</span>
      {selected && <ChevronRight size={14} />}
    </button>
  );
}

function MessageCard({ message }: { message: unknown }) {
  // Determine if it's a standard IntercomMessage or a ThoughtEvent/StreamToken
  const isIntercom = !!(message && typeof message === 'object' && 'source' in message && 'type' in message);

  if (isIntercom) {
    const msg = message as IntercomMessage;
    return (
      <div className="animate-in fade-in slide-in-from-bottom-2 duration-300">
        <div className="flex items-start gap-3">
          <div className="mt-1 w-2 h-2 rounded-full bg-sera-accent shrink-0" />
          <div className="flex-1 bg-sera-surface/40 border border-sera-border rounded-xl overflow-hidden">
            <div className="px-4 py-2 border-b border-sera-border bg-sera-surface/60 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="text-[11px] font-bold text-sera-accent uppercase tracking-wider">
                  {msg.type}
                </span>
                <span className="text-[10px] text-sera-text-dim font-mono">
                  {msg.source.agent} @ {msg.source.circle}
                </span>
              </div>
              <div className="flex items-center gap-2 text-[10px] text-sera-text-dim">
                <Clock size={10} />
                {new Date(msg.timestamp).toLocaleTimeString()}
              </div>
            </div>
            <div className="p-4 space-y-3">
              <pre className="text-[11.5px] text-sera-text leading-relaxed font-mono whitespace-pre-wrap bg-sera-bg-dark/50 p-3 rounded-lg border border-sera-border/50 overflow-x-auto">
                {JSON.stringify(msg.payload, null, 2)}
              </pre>
              <div className="flex items-center gap-3">
                <div className="flex items-center gap-1 text-[10px] text-sera-text-dim bg-sera-bg-dark/30 px-2 py-0.5 rounded border border-sera-border/30">
                  <Shield size={10} />
                  Tier {msg.metadata?.securityTier || 1}
                </div>
                <div className="text-[9px] font-mono text-sera-text-dim/60 truncate">
                  ID: {msg.id}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // ThoughtEvent or other
  const event = message as ThoughtEvent;
  return (
    <div className="animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div className="flex items-start gap-3">
        <div className="mt-1 w-2 h-2 rounded-full bg-sera-accent shrink-0" />
        <div className="flex-1 bg-sera-surface/40 border border-sera-border rounded-xl overflow-hidden">
          <div className="px-4 py-2 border-b border-sera-border bg-sera-surface/60 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="text-[11px] font-bold text-sera-accent uppercase tracking-wider">
                {event.stepType || 'event'}
              </span>
              <span className="text-[10px] text-sera-text-dim font-mono">
                {event.agentDisplayName || 'system'}
              </span>
            </div>
            <div className="flex items-center gap-2 text-[10px] text-sera-text-dim">
              <Clock size={10} />
              {event.timestamp ? new Date(event.timestamp).toLocaleTimeString() : '--:--'}
            </div>
          </div>
          <div className="p-4 text-[11.5px] text-sera-text leading-relaxed whitespace-pre-wrap">
            {typeof event.content === 'string' ? event.content : JSON.stringify(event, null, 2)}
          </div>
        </div>
      </div>
    </div>
  );
}
