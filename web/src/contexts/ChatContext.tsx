import { createContext, useContext, useState, useRef, useEffect, useCallback, ReactNode } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';
import { sendChatStream } from '@/lib/api/chat';
import { toast } from '@/lib/toast';
import type { Message, MessageThought } from '@/lib/api/types';

interface TokenPayload {
  token: string;
  done: boolean;
  messageId?: string;
  error?: string;
}

interface ThoughtPayload {
  timestamp: string;
  stepType: string;
  content: string;
  agentId: string;
  agentDisplayName: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
}

interface ChatContextType {
  messages: Message[];
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  streaming: boolean;
  setStreaming: React.Dispatch<React.SetStateAction<boolean>>;
  streamingMsgId: React.MutableRefObject<string | null>;
  messageIdRef: React.MutableRefObject<string | null>;
  expandedThoughts: Set<string>;
  setExpandedThoughts: React.Dispatch<React.SetStateAction<Set<string>>>;
  queueCount: number;
  handleSend: (text: string, selectedAgent: string, selectedAgentId: string, sessionId: string | null, fetchSessions: () => void, setSessionId: (id: string | null) => void) => Promise<void>;
  handleCancel: () => void;
  setupSubscriptions: (selectedAgent: string, selectedAgentId: string, fetchSessions: () => void) => () => void;
}

const ChatContext = createContext<ChatContextType | undefined>(undefined);

export function ChatProvider({ children }: { children: ReactNode }) {
  const { client: centrifugoClient } = useCentrifugoContext();
  const [messages, setMessages] = useState<Message[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [expandedThoughts, setExpandedThoughts] = useState<Set<string>>(new Set());
  const [queueCount, setQueueCount] = useState(0);

  const streamingMsgId = useRef<string | null>(null);
  const messageIdRef = useRef<string | null>(null);
  const messageQueue = useRef<string[]>([]);

  const handleSend = useCallback(
    async (text: string, selectedAgent: string, selectedAgentId: string, sessionId: string | null, fetchSessions: () => void, setSessionId: (id: string | null) => void) => {
      if (!text || !selectedAgent) return;
      if (streaming) {
        messageQueue.current.push(text);
        setQueueCount(messageQueue.current.length);
        return;
      }
      const userMsgId = crypto.randomUUID();
      const agentMsgId = crypto.randomUUID();
      const userMsg: Message = {
        id: userMsgId,
        role: 'user',
        content: text,
        thoughts: [],
        streaming: false,
        createdAt: new Date(),
      };
      const agentMsg: Message = {
        id: agentMsgId,
        role: 'agent',
        content: '',
        thoughts: [],
        streaming: true,
        createdAt: new Date(),
      };
      setMessages((prev) => [...prev, userMsg, agentMsg]);
      setStreaming(true);
      setExpandedThoughts((prev) => new Set(prev).add(agentMsgId));
      streamingMsgId.current = agentMsgId;
      try {
        const { sessionId: newSessionId, messageId } = await sendChatStream(
          selectedAgent,
          text,
          sessionId ?? undefined,
          selectedAgentId || undefined
        );
        setSessionId(newSessionId);
        messageIdRef.current = messageId;
        void fetchSessions();
      } catch (err) {
        const errMsg = err instanceof Error ? err.message : 'Failed to send message';
        toast.error(errMsg);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === agentMsgId ? { ...m, content: `Error: ${errMsg}`, streaming: false } : m
          )
        );
        setStreaming(false);
        streamingMsgId.current = null;
        messageIdRef.current = null;
      }
    },
    [streaming]
  );

  const handleCancel = useCallback(() => {
    if (!streaming) return;
    setMessages((prev) =>
      prev.map((m) =>
        m.id === streamingMsgId.current
          ? { ...m, content: m.content + '\n\n*(cancelled)*', streaming: false }
          : m
      )
    );
    setStreaming(false);
    streamingMsgId.current = null;
    messageIdRef.current = null;
  }, [streaming]);

  const setupSubscriptions = useCallback((selectedAgent: string, selectedAgentId: string, fetchSessions: () => void) => {
    const channelKey = selectedAgentId || selectedAgent;
    if (!centrifugoClient || !channelKey) return () => {};

    const tokenChannel = `tokens:${channelKey}`;
    const thoughtChannel = `thoughts:${channelKey}`;

    // Helper to clear existing
    const clearSub = (channel: string) => {
      const existing = centrifugoClient.getSubscription(channel);
      if (existing) {
        existing.unsubscribe();
        existing.removeAllListeners();
        centrifugoClient.removeSubscription(existing);
      }
    };

    clearSub(tokenChannel);
    clearSub(thoughtChannel);

    const tokenSub = centrifugoClient.newSubscription(tokenChannel);
    tokenSub.on('publication', (ctx: PublicationContext) => {
      const { token, done, messageId, error } = ctx.data as TokenPayload;
      if (messageId != null && messageIdRef.current != null && messageId !== messageIdRef.current) return;
      if (!streamingMsgId.current) return;
      setMessages((prev) => {
        const idx = prev.findIndex((m) => m.id === streamingMsgId.current);
        if (idx === -1) return prev;
        const updated = [...prev];
        if (error) {
          updated[idx] = { ...updated[idx]!, content: `**Error:** ${error}`, streaming: false };
        } else {
          updated[idx] = { ...updated[idx]!, content: updated[idx]!.content + token, streaming: !done };
        }
        return updated;
      });
      if (done) {
        setTimeout(() => {
          setStreaming(false);
          streamingMsgId.current = null;
          void fetchSessions();
        }, 500);
      }
    });
    tokenSub.subscribe();

    const thoughtSub = centrifugoClient.newSubscription(thoughtChannel);
    thoughtSub.on('publication', (ctx: PublicationContext) => {
      const event = ctx.data as ThoughtPayload;
      if (!streamingMsgId.current) return;
      const thought: MessageThought = {
        timestamp: event.timestamp,
        stepType: event.stepType,
        content: event.content,
        ...(event.toolName ? { toolName: event.toolName } : {}),
        ...(event.toolArgs ? { toolArgs: event.toolArgs } : {}),
      };
      setMessages((prev) =>
        prev.map((msg) =>
          msg.id === streamingMsgId.current ? { ...msg, thoughts: [...msg.thoughts, thought] } : msg
        )
      );
    });
    thoughtSub.subscribe();

    return () => {
      tokenSub.unsubscribe();
      tokenSub.removeAllListeners();
      centrifugoClient.removeSubscription(tokenSub);
      thoughtSub.unsubscribe();
      thoughtSub.removeAllListeners();
      centrifugoClient.removeSubscription(thoughtSub);
    };
  }, [centrifugoClient]);

  // Handle queue draining
  const prevStreaming = useRef(false);
  useEffect(() => {
    if (prevStreaming.current && !streaming && messageQueue.current.length > 0) {
      // This needs access to selectedAgent etc, maybe move logic to the hook or pass as callback
    }
    prevStreaming.current = streaming;
  }, [streaming]);

  return (
    <ChatContext.Provider
      value={{
        messages,
        setMessages,
        streaming,
        setStreaming,
        streamingMsgId,
        messageIdRef,
        expandedThoughts,
        setExpandedThoughts,
        queueCount,
        handleSend,
        handleCancel,
        setupSubscriptions,
      }}
    >
      {children}
    </ChatContext.Provider>
  );
}

export function useChat() {
  const context = useContext(ChatContext);
  if (!context) throw new Error('useChat must be used within ChatProvider');
  return context;
}
