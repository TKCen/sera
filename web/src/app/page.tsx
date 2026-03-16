'use client';

import { useState, useEffect, useRef } from 'react';
import { Centrifuge } from 'centrifuge';
import SettingsMenu from '@/components/SettingsMenu';

const TypingMessage = ({ text, sender, animate = true }: { text: string, sender: string, animate?: boolean }) => {
  const [displayedText, setDisplayedText] = useState(animate ? '' : text);

  useEffect(() => {
    if (!animate) {
      setDisplayedText(text);
      return;
    }

    let i = 0;
    const interval = setInterval(() => {
      setDisplayedText(text.slice(0, i));
      i++;
      if (i > text.length) clearInterval(interval);
    }, 15);
    return () => clearInterval(interval);
  }, [text, animate]);

  return (
    <div className="flex gap-4">
      <span className="text-secondary font-bold whitespace-nowrap">{sender}:</span>
      <p className="text-primary/90 whitespace-pre-wrap font-mono">
        {displayedText}
        {animate && <span className="animate-pulse">_</span>}
      </p>
    </div>
  );
};

export default function Home() {
  const [uptime, setUptime] = useState('00h 00m 00s');
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<{ sender: string, text: string, animate?: boolean }[]>([
    { sender: 'SERA', text: 'Initializing neural links... Standby for input. System stability confirmed. Holographic interface active.', animate: true }
  ]);
  const [loading, setLoading] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom
  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages]);

  useEffect(() => {
    const startTime = Date.now();
    const interval = setInterval(() => {
      const seconds = Math.floor((Date.now() - startTime) / 1000);
      const h = Math.floor(seconds / 3600).toString().padStart(2, '0');
      const m = Math.floor((seconds % 3600) / 60).toString().padStart(2, '0');
      const s = (seconds % 60).toString().padStart(2, '0');
      setUptime(`${h}h ${m}m ${s}s`);
    }, 1000);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    // Setup Centrifugo
    const centrifugeUrl = process.env.NEXT_PUBLIC_CENTRIFUGO_URL || 'ws://localhost:10001/connection/websocket';
    const centrifuge = new Centrifuge(centrifugeUrl);

    centrifuge.on('connected', (ctx) => {
      console.log('Centrifugo connected', ctx);
    });

    centrifuge.on('disconnected', (ctx) => {
      console.log('Centrifugo disconnected', ctx);
    });

    const sub = centrifuge.newSubscription('chat');

    sub.on('publication', (ctx) => {
      const chunk = ctx.data.chunk;
      if (chunk) {
        setMessages((prev) => {
          // If the last message is from SERA (Stream), append to it immutably
          const newMessages = [...prev];
          const lastMsg = newMessages[newMessages.length - 1];
          if (lastMsg && lastMsg.sender === 'SERA (Stream)') {
            newMessages[newMessages.length - 1] = {
              ...lastMsg,
              text: lastMsg.text + chunk,
            };
          } else {
            newMessages.push({ sender: 'SERA (Stream)', text: chunk, animate: false });
          }
          return newMessages;
        });
      }
    });

    sub.subscribe();
    centrifuge.connect();

    return () => {
      sub.unsubscribe();
      centrifuge.disconnect();
    };
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim() || loading) return;

    const userMessage = input;
    setInput('');
    setMessages(prev => [...prev, { sender: 'USER', text: userMessage, animate: false }]);
    setLoading(true);

    try {
      const res = await fetch('/api/chat', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ message: userMessage }),
      });

      if (!res.ok) {
        throw new Error('Failed to send message');
      }

      await res.json();
      // Remove duplicate final message since it streamed

    } catch (err) {
      console.error(err);
      setMessages(prev => [...prev, { sender: 'SYSTEM', text: 'Error connecting to core orchestrator.', animate: true }]);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="max-w-6xl mx-auto py-12 space-y-8">
      <header className="flex items-center justify-between border-b border-primary/20 pb-6 mb-12">
        <div>
          <h1 className="text-5xl font-mono font-bold tracking-tighter text-primary glow-text glitch-hover inline-block">
            SERA_CORE_v1.0
          </h1>
          <p className="text-muted-foreground font-mono mt-2 flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-primary animate-pulse" />
            SYSTEM_UPTIME: {uptime}
          </p>
        </div>
        <div className="flex gap-4">
          <div className="glass-panel px-4 py-2 flex items-center gap-3">
            <span className="text-xs font-mono text-muted-foreground">SANDBOX_STATUS</span>
            <span className="text-xs font-mono text-green-400">STABLE</span>
          </div>
          <div className="glass-panel px-4 py-2 flex items-center gap-3">
            <span className="text-xs font-mono text-muted-foreground">THOUGHT_SYNC</span>
            <span className="text-xs font-mono text-primary">CONNECTED</span>
          </div>
          <SettingsMenu />
        </div>
      </header>

      <div className="grid grid-cols-12 gap-6 h-[600px]">
        {/* Sidebar / File Explorer Placeholder */}
        <aside className="col-span-3 glass-panel p-4 flex flex-col">
          <div className="flex items-center justify-between mb-4 pb-2 border-b border-white/5">
            <span className="text-xs font-mono font-bold tracking-widest text-muted-foreground">WORKSPACE</span>
            <span className="text-[10px] font-mono bg-primary/10 text-primary px-1.5 py-0.5 rounded">INIT</span>
          </div>
          <div className="flex-1 flex items-center justify-center border-2 border-dashed border-white/5 rounded-lg">
            <p className="text-xs font-mono text-muted-foreground/30 uppercase tracking-widest">Scanning_FS...</p>
          </div>
        </aside>

        {/* Main Terminal / Chat Area */}
        <section className="col-span-9 glass-panel flex flex-col relative overflow-hidden hologram-flicker">
          <div className="flex-1 p-6 font-mono text-sm space-y-4 overflow-y-auto">
            {messages.map((msg, idx) => (
              <TypingMessage
                key={idx}
                sender={msg.sender}
                text={msg.text}
                animate={msg.animate}
              />
            ))}
            <div ref={messagesEndRef} />
          </div>
          
          <div className="p-4 border-t border-white/5 bg-black/20">
            <form onSubmit={handleSubmit} className="relative">
              <input 
                type="text" 
                value={input}
                onChange={(e) => setInput(e.target.value)}
                disabled={loading}
                placeholder="PROMPT SERA..."
                className="w-full bg-input border border-primary/20 rounded-md py-3 pl-4 pr-12 text-sm font-mono focus:outline-none focus:border-primary/50 transition-colors placeholder:text-muted-foreground/50 disabled:opacity-50"
              />
              <button
                type="submit"
                disabled={loading || !input.trim()}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-[10px] font-mono bg-primary/10 text-primary px-2 py-1 rounded border border-primary/20 hover:bg-primary/20 transition-colors disabled:opacity-50"
              >
                {loading ? 'WAIT' : '↵ ENTER'}
              </button>
            </form>
          </div>
        </section>
      </div>

      <footer className="mt-12 text-center">
        <p className="text-[10px] font-mono text-muted-foreground/50 uppercase tracking-[0.2em]">
          Powered by Centrifugo // Sandboxed by Docker // Designed for the Homelab
        </p>
      </footer>
    </div>
  );
}
