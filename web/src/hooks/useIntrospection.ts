import { useEffect, useMemo, useRef, useState } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';
import type { ThoughtEvent, AgentInstance } from '@/lib/api/types';

export interface IntrospectionMessage {
  id: string;
  timestamp: number;
  source: string; // agent name or "system"
  sourceId: string; // agent UUID or "system"
  type: 'thought' | 'system' | 'circle' | 'status';
  subType?: string; // e.g. "reasoning", "tool_call", "info", "error"
  content: string;
  metadata?: {
    toolName?: string;
    toolArgs?: Record<string, unknown>;
    severity?: string;
    circleId?: string;
  };
}

export type IntrospectionView =
  | { kind: 'global' }
  | { kind: 'circle'; circleId: string; agentIds: string[] }
  | { kind: 'agent'; agentId: string; agentName: string };

export function useIntrospection(view: IntrospectionView, agents: AgentInstance[]) {
  const { client } = useCentrifugoContext();
  const [messages, setMessages] = useState<IntrospectionMessage[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const subscriptionsRef = useRef<Map<string, any>>(new Map());

  // Map agent IDs to names for display
  const agentMap = useMemo(
    () => new Map(agents.map((a) => [a.id, a.display_name ?? a.name])),
    [agents]
  );

  useEffect(() => {
    if (!client) return;

    // Clean up all existing subscriptions
    const cleanup = () => {
      subscriptionsRef.current.forEach((sub) => {
        if (sub) {
          sub.unsubscribe?.();
          sub.removeAllListeners?.();
          client.removeSubscription?.(sub);
        }
      });
      subscriptionsRef.current.clear();
    };

    cleanup();

    // Reset messages when view changes
    setMessages([]);

    // Handler for thought events
    const handleThoughtPublication = (ctx: PublicationContext, agentId: string) => {
      const thought = ctx.data as ThoughtEvent;
      const agentName = agentMap.get(agentId) ?? agentId;
      const msg: IntrospectionMessage = {
        id: `thought-${agentId}-${thought.timestamp}`,
        timestamp: new Date(thought.timestamp).getTime(),
        source: agentName,
        sourceId: agentId,
        type: 'thought',
        subType: thought.stepType,
        content: thought.content,
        metadata: {
          toolName: thought.toolName,
          toolArgs: thought.toolArgs,
        },
      };
      setMessages((prev) => {
        const updated = [...prev, msg];
        return updated.length > 500 ? updated.slice(-500) : updated;
      });
    };

    // Handler for system events
    const handleSystemPublication = (ctx: PublicationContext) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const event = ctx.data as any;
      const msg: IntrospectionMessage = {
        id: `system-${event.timestamp ?? Date.now()}`,
        timestamp: new Date(event.timestamp ?? Date.now()).getTime(),
        source: 'system',
        sourceId: 'system',
        type: 'system',
        subType: event.type,
        content: typeof event.payload === 'string' ? event.payload : JSON.stringify(event.payload),
        metadata: {
          severity: event.severity,
        },
      };
      setMessages((prev) => {
        const updated = [...prev, msg];
        return updated.length > 500 ? updated.slice(-500) : updated;
      });
    };

    // Subscribe based on view
    if (view.kind === 'global') {
      // Subscribe to all agent thought streams
      agents.forEach((agent) => {
        const channel = `internal:agent:${agent.id}:thoughts`;
        const existing = client.getSubscription(channel);
        if (existing) {
          existing.unsubscribe();
          existing.removeAllListeners();
          client.removeSubscription(existing);
        }

        const sub = client.newSubscription(channel);
        sub.on('publication', (ctx: PublicationContext) => {
          handleThoughtPublication(ctx, agent.id);
        });
        sub.subscribe();
        subscriptionsRef.current.set(channel, sub);
      });

      // Subscribe to system events
      const systemChannel = 'system:events';
      const existingSystem = client.getSubscription(systemChannel);
      if (existingSystem) {
        existingSystem.unsubscribe();
        existingSystem.removeAllListeners();
        client.removeSubscription(existingSystem);
      }

      const systemSub = client.newSubscription(systemChannel);
      systemSub.on('publication', handleSystemPublication);
      systemSub.subscribe();
      subscriptionsRef.current.set(systemChannel, systemSub);
    } else if (view.kind === 'circle') {
      // Subscribe to agents in this circle
      view.agentIds.forEach((agentId) => {
        const channel = `internal:agent:${agentId}:thoughts`;
        const existing = client.getSubscription(channel);
        if (existing) {
          existing.unsubscribe();
          existing.removeAllListeners();
          client.removeSubscription(existing);
        }

        const sub = client.newSubscription(channel);
        sub.on('publication', (ctx: PublicationContext) => {
          handleThoughtPublication(ctx, agentId);
        });
        sub.subscribe();
        subscriptionsRef.current.set(channel, sub);
      });
    } else if (view.kind === 'agent') {
      // Subscribe to single agent
      const channel = `internal:agent:${view.agentId}:thoughts`;
      const existing = client.getSubscription(channel);
      if (existing) {
        existing.unsubscribe();
        existing.removeAllListeners();
        client.removeSubscription(existing);
      }

      const sub = client.newSubscription(channel);
      sub.on('publication', (ctx: PublicationContext) => {
        handleThoughtPublication(ctx, view.agentId);
      });
      sub.subscribe();
      subscriptionsRef.current.set(channel, sub);
    }

    return cleanup;
  }, [client, view, agents, agentMap]);

  return { messages };
}
