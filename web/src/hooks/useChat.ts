import { useState, useEffect, useCallback, useRef } from 'react';
import { Centrifuge } from 'centrifuge';
import type { MessageProps } from '@/components/MessageBubble';

export function useChat() {
  const [messages, setMessages] = useState<MessageProps[]>([
    {
      id: 'init',
      role: 'assistant',
      content: 'Initializing neural links... Standby for input. System stability confirmed. Holographic interface active.',
    }
  ]);
  const [isConnected, setIsConnected] = useState(false);

  // Centrifuge reference
  const centrifugeRef = useRef<Centrifuge | null>(null);

  useEffect(() => {
    // Basic setup for Centrifuge client.
    // In a real application, token and endpoint would be configurable.
    const centrifuge = new Centrifuge('ws://localhost:8000/connection/websocket');
    centrifugeRef.current = centrifuge;

    centrifuge.on('connected', function(ctx) {
      setIsConnected(true);
      console.log('Centrifugo connected', ctx);
    });

    centrifuge.on('disconnected', function(ctx) {
      setIsConnected(false);
      console.log('Centrifugo disconnected', ctx);
    });

    const sub = centrifuge.newSubscription('thought_stream');

    sub.on('publication', function(ctx) {
      // Assuming ctx.data contains { id: string, content: string }
      // This is a naive implementation; you'd typically stream text incrementally
      const { id, content, done } = ctx.data;

      setMessages(prev => {
        const existingMessageIndex = prev.findIndex(m => m.id === id);

        if (existingMessageIndex >= 0) {
          const newMessages = [...prev];
          newMessages[existingMessageIndex] = {
            ...newMessages[existingMessageIndex],
            content: newMessages[existingMessageIndex].content + (content || ''),
            isThinking: !done
          };
          return newMessages;
        } else {
          // If it's a new message from the stream
          return [...prev, {
            id,
            role: 'assistant',
            content: content || '',
            isThinking: !done
          }];
        }
      });
    });

    sub.subscribe();
    centrifuge.connect();

    return () => {
      sub.unsubscribe();
      centrifuge.disconnect();
    };
  }, []);

  const sendMessage = useCallback(async (content: string) => {
    if (!content.trim()) return;

    const userMessageId = `user-${Date.now()}`;
    const assistantMessageId = `assistant-${Date.now()}`;

    // Add user message
    setMessages(prev => [
      ...prev,
      { id: userMessageId, role: 'user', content }
    ]);

    // Add placeholder "Thinking..." message
    setMessages(prev => [
      ...prev,
      { id: assistantMessageId, role: 'assistant', content: '', isThinking: true }
    ]);

    try {
      const response = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: content, id: assistantMessageId }),
      });

      if (!response.ok) {
        throw new Error('Network response was not ok');
      }

      // We expect the actual text stream to come through Centrifugo,
      // but in case there is no streaming or a fallback is needed:
      const data = await response.json();

      if (data && data.content) {
         setMessages(prev => prev.map(m =>
           m.id === assistantMessageId
             ? { ...m, content: data.content, isThinking: false }
             : m
         ));
      }
    } catch (error) {
      console.error('Failed to send message:', error);
      setMessages(prev => prev.map(m =>
        m.id === assistantMessageId
          ? { ...m, content: 'Error: Connection lost or failed to process prompt.', isThinking: false }
          : m
      ));
    }
  }, []);

  return {
    messages,
    sendMessage,
    isConnected
  };
}
