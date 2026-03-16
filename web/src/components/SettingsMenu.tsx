'use client';

import { useState, useEffect } from 'react';
import { Settings, Save, X, RefreshCw } from 'lucide-react';

interface LLMConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

export default function SettingsMenu() {
  const [isOpen, setIsOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [config, setConfig] = useState<LLMConfig>({
    baseUrl: '',
    apiKey: '',
    model: ''
  });

  useEffect(() => {
    if (isOpen) {
      fetchConfig();
    }
  }, [isOpen]);

  const fetchConfig = async () => {
    setLoading(true);
    try {
      const res = await fetch('http://localhost:3001/api/config/llm');
      const data = await res.json();
      setConfig(data);
    } catch (err) {
      console.error('Failed to fetch config:', err);
    } finally {
      setLoading(false);
    }
  };

  const saveConfig = async () => {
    setLoading(true);
    try {
      const res = await fetch('http://localhost:3001/api/config/llm', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(config),
      });
      if (res.ok) {
        setIsOpen(false);
      }
    } catch (err) {
      console.error('Failed to save config:', err);
    } finally {
      setLoading(false);
    }
  };

  if (!isOpen) {
    return (
      <button 
        onClick={() => setIsOpen(true)}
        className="glass-panel p-2 hover:bg-white/10 transition-colors text-primary"
      >
        <Settings size={20} />
      </button>
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
      <div className="glass-panel w-full max-w-md p-6 space-y-6 relative overflow-hidden hologram-flicker">
        <div className="flex items-center justify-between border-b border-primary/20 pb-4">
          <h2 className="text-xl font-mono font-bold text-primary glow-text tracking-tighter">
            SYSTEM_CONFIG::LLM_PROVIDER
          </h2>
          <button onClick={() => setIsOpen(false)} className="text-muted-foreground hover:text-white">
            <X size={20} />
          </button>
        </div>

        <div className="space-y-4 font-mono">
          <div className="space-y-2">
            <label className="text-xs text-muted-foreground uppercase tracking-widest">Base API URL</label>
            <input 
              type="text"
              value={config.baseUrl}
              onChange={(e) => setConfig({ ...config, baseUrl: e.target.value })}
              placeholder="http://host.docker.internal:1234/v1"
              className="w-full bg-input border border-primary/20 rounded-md py-2 px-3 text-sm focus:outline-none focus:border-primary/50"
            />
          </div>

          <div className="space-y-2">
            <label className="text-xs text-muted-foreground uppercase tracking-widest">API Key</label>
            <input 
              type="password"
              value={config.apiKey}
              onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
              placeholder="lm-studio"
              className="w-full bg-input border border-primary/20 rounded-md py-2 px-3 text-sm focus:outline-none focus:border-primary/50"
            />
          </div>

          <div className="space-y-2">
            <label className="text-xs text-muted-foreground uppercase tracking-widest">Model Identifier</label>
            <input 
              type="text"
              value={config.model}
              onChange={(e) => setConfig({ ...config, model: e.target.value })}
              placeholder="model-name"
              className="w-full bg-input border border-primary/20 rounded-md py-2 px-3 text-sm focus:outline-none focus:border-primary/50"
            />
          </div>
        </div>

        <div className="flex gap-3 pt-4">
          <button 
            onClick={saveConfig}
            disabled={loading}
            className="flex-1 bg-primary/10 hover:bg-primary/20 text-primary border border-primary/30 py-2 rounded-md font-mono text-xs flex items-center justify-center gap-2 transition-all hover:scale-[1.02]"
          >
            {loading ? <RefreshCw className="animate-spin" size={14} /> : <Save size={14} />}
            COMMIT_CHANGES
          </button>
        </div>

        {/* Decorative corner */}
        <div className="absolute top-0 right-0 w-8 h-8 pointer-events-none">
          <div className="absolute top-0 right-0 w-full h-1 bg-primary/40" />
          <div className="absolute top-0 right-0 w-1 h-full bg-primary/40" />
        </div>
      </div>
    </div>
  );
}
