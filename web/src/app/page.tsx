'use client';

import { useState, useEffect } from 'react';
import SettingsMenu from '@/components/SettingsMenu';

const TypingMessage = ({ text, sender }: { text: string, sender: string }) => {
  const [displayedText, setDisplayedText] = useState('');

  useEffect(() => {
    let i = 0;
    const interval = setInterval(() => {
      setDisplayedText(text.slice(0, i));
      i++;
      if (i > text.length) clearInterval(interval);
    }, 30);
    return () => clearInterval(interval);
  }, [text]);

  return (
    <div className="flex gap-4">
      <span className="text-secondary font-bold">{sender}:</span>
      <p className="text-primary/90">{displayedText}<span className="animate-pulse">_</span></p>
    </div>
  );
};

export default function Home() {
  const [uptime, setUptime] = useState('00h 00m 00s');

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
            <TypingMessage
              sender="SERA"
              text="Initializing neural links... Standby for input. System stability confirmed. Holographic interface active."
            />
          </div>
          
          <div className="p-4 border-t border-white/5 bg-black/20">
            <div className="relative">
              <input 
                type="text" 
                placeholder="PROMPT SERA..."
                className="w-full bg-input border border-primary/20 rounded-md py-3 pl-4 pr-12 text-sm font-mono focus:outline-none focus:border-primary/50 transition-colors placeholder:text-muted-foreground/50"
              />
              <div className="absolute right-3 top-1/2 -translate-y-1/2 text-[10px] font-mono bg-primary/10 text-primary px-2 py-1 rounded border border-primary/20">
                ↵ ENTER
              </div>
            </div>
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
