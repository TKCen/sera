import { useState, useEffect } from 'react';
import { Save } from 'lucide-react';
import * as providersApi from '@/lib/api/providers';
import { Button } from '@/components/ui/button';

export function GeneralTab({
  registeredModels,
}: {
  registeredModels: Array<{ modelName: string; description?: string; provider?: string }>;
}) {
  const [defaultModel, setDefaultModelLocal] = useState('');
  const [saving, setSaving] = useState(false);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    providersApi
      .getDefaultModel()
      .then((res) => {
        if (res.defaultModel) setDefaultModelLocal(res.defaultModel);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, []);

  return (
    <div className="sera-card-static p-6 space-y-6 max-w-xl animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div>
        <h3 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase mb-4">
          Default Model
        </h3>
        <p className="text-xs text-sera-text-muted mb-3">
          Agents configured with &ldquo;default&rdquo; as their model will use this model.
        </p>
        <div className="flex gap-3 items-end">
          <div className="flex-1 space-y-1.5">
            <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
              Model
            </label>
            {loaded ? (
              <select
                value={defaultModel}
                onChange={(e) => setDefaultModelLocal(e.target.value)}
                className="sera-input text-xs"
              >
                <option value="">— No default —</option>
                {registeredModels.map((m) => (
                  <option key={m.modelName} value={m.modelName}>
                    {m.description ?? m.modelName}
                  </option>
                ))}
              </select>
            ) : (
              <div className="h-9 bg-sera-surface rounded animate-pulse" />
            )}
          </div>
          <Button
            size="sm"
            className="h-9 text-xs gap-1.5 bg-sera-accent hover:bg-sera-accent-hover text-sera-bg"
            disabled={saving || !defaultModel}
            onClick={() => {
              setSaving(true);
              providersApi
                .setDefaultModel(defaultModel)
                .then(() => setSaving(false))
                .catch(() => setSaving(false));
            }}
          >
            <Save size={13} /> Save
          </Button>
        </div>
      </div>
      <div className="border-t border-sera-border pt-6">
        <h3 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase mb-3">
          System Info
        </h3>
        <div className="space-y-2 text-xs">
          <div className="flex justify-between">
            <span className="text-sera-text-muted">Platform</span>
            <span className="text-sera-text">SERA v1.0</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sera-text-muted">Runtime</span>
            <span className="text-sera-text">Node.js 22 + TypeScript</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sera-text-muted">Frontend</span>
            <span className="text-sera-text">Vite + React Router v7</span>
          </div>
        </div>
      </div>
    </div>
  );
}
