import { useState } from 'react';
import { Plus, XCircle, RefreshCw, Zap, Save, Radio } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { DynamicProviderCard } from '@/components/DynamicProviderCard';
import {
  useDynamicProviders,
  useDynamicProviderStatuses,
  useAddDynamicProvider,
  useRemoveDynamicProvider,
} from '@/hooks/useProviders';
import { testDynamicConnection } from '@/lib/api/providers';

export function DynamicDiscoverySection() {
  const [showAddDynamic, setShowAddDynamic] = useState(false);
  const [newDynamic, setNewDynamic] = useState({
    id: '',
    name: '',
    baseUrl: 'http://host.docker.internal:1234/v1',
    apiKey: '',
  });
  const [testResult, setTestResult] = useState<{
    success: boolean;
    models: string[];
    error?: string;
  } | null>(null);
  const [isTesting, setIsTesting] = useState(false);

  const { data: dynamicData, isLoading: isLoadingDynamic, refetch: refetchDynamic } = useDynamicProviders();
  const { data: statusesData } = useDynamicProviderStatuses();
  const addDynamic = useAddDynamicProvider();
  const removeDynamic = useRemoveDynamicProvider();

  const handleRefetch = () => {
    void refetchDynamic();
  };

  return (
    <section>
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Radio size={14} className="text-amber-400" />
          <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
            Dynamic Discovery
          </h2>
          <span className="text-[11px] text-sera-text-dim/60">— LM Studio, Ollama, etc.</span>
        </div>
        <Button
          size="sm"
          className="h-8 text-[11px] gap-1.5 bg-sera-accent/10 hover:bg-sera-accent/20 text-sera-accent border border-sera-accent/20"
          onClick={() => setShowAddDynamic(true)}
        >
          <Plus size={14} /> Add Provider
        </Button>
      </div>

      {showAddDynamic && (
        <div className="sera-card-static p-5 mb-4 border-sera-accent/30 bg-sera-accent/5 animate-in zoom-in-95 duration-200">
          <div className="flex justify-between items-start mb-4">
            <h3 className="text-sm font-semibold text-sera-text">Add LM Studio Instance</h3>
            <button
              onClick={() => setShowAddDynamic(false)}
              className="text-sera-text-dim hover:text-sera-text"
            >
              <XCircle size={16} />
            </button>
          </div>

          <div className="grid grid-cols-2 gap-4 mb-4">
            <div className="space-y-1.5">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                Provider Name
              </label>
              <input
                type="text"
                placeholder="e.g. Local LM Studio"
                value={newDynamic.name}
                onChange={(e) => setNewDynamic({ ...newDynamic, name: e.target.value })}
                className="sera-input text-xs"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                Unique ID
              </label>
              <input
                type="text"
                placeholder="e.g. lmstudio-1"
                value={newDynamic.id}
                onChange={(e) =>
                  setNewDynamic({
                    ...newDynamic,
                    id: e.target.value.toLowerCase().replace(/\s+/g, '-'),
                  })
                }
                className="sera-input text-xs font-mono"
              />
            </div>
            <div className="space-y-1.5 col-span-2">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                Base URL (with /v1)
              </label>
              <input
                type="text"
                value={newDynamic.baseUrl}
                onChange={(e) => setNewDynamic({ ...newDynamic, baseUrl: e.target.value })}
                className="sera-input text-xs font-mono"
              />
              <p className="text-[10px] text-sera-text-dim mt-0.5">
                Running in Docker? Use <code className="font-mono">host.docker.internal</code>{' '}
                instead of <code className="font-mono">localhost</code>
              </p>
            </div>
            <div className="space-y-1.5 col-span-2">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                API Key <span className="text-sera-text-dim/50">(optional)</span>
              </label>
              <input
                type="password"
                value={newDynamic.apiKey}
                onChange={(e) => setNewDynamic({ ...newDynamic, apiKey: e.target.value })}
                className="sera-input text-xs"
              />
            </div>
          </div>

          {testResult && (
            <div
              className={`mb-4 overflow-hidden rounded-lg border text-xs ${
                testResult.success
                  ? 'bg-sera-success/10 border-sera-success/20 text-sera-success'
                  : 'bg-sera-error/10 border-sera-error/20 text-sera-error'
              }`}
            >
              <div className="p-3 flex items-start gap-2">
                {testResult.success ? <Zap size={14} className="text-sera-success" /> : <XCircle size={14} className="text-sera-error" />}
                <div>
                  <p className="font-semibold">
                    {testResult.success ? 'Connection successful' : 'Connection failed'}
                  </p>
                  {!testResult.success && <p className="mt-0.5 opacity-90">{testResult.error}</p>}
                  {testResult.success && (
                    <p className="mt-1 opacity-90">
                      Found {testResult.models.length} model(s): {testResult.models.join(', ')}
                    </p>
                  )}
                </div>
              </div>
            </div>
          )}

          <div className="flex gap-3">
            <Button
              variant="outline"
              className="flex-1 text-xs h-10"
              disabled={isTesting || !newDynamic.baseUrl}
              onClick={async () => {
                setIsTesting(true);
                setTestResult(null);
                try {
                  const res = await testDynamicConnection(newDynamic.baseUrl, newDynamic.apiKey);
                  setTestResult(res);
                } catch (err: unknown) {
                  setTestResult({
                    success: false,
                    models: [],
                    error: err instanceof Error ? err.message : String(err),
                  });
                } finally {
                  setIsTesting(false);
                }
              }}
            >
              {isTesting ? <RefreshCw className="animate-spin" size={14} /> : <Zap size={14} />}
              Test & Discover
            </Button>
            <Button
              className="flex-1 text-xs bg-sera-accent hover:bg-sera-accent-hover text-sera-bg h-10"
              disabled={
                !(
                  newDynamic.name &&
                  newDynamic.id &&
                  newDynamic.baseUrl &&
                  testResult?.success &&
                  !addDynamic.isPending
                )
              }
              onClick={() => {
                addDynamic.mutate({
                  ...newDynamic,
                  type: 'lm-studio',
                  enabled: true,
                  intervalMs: 60000,
                });
                setShowAddDynamic(false);
                setNewDynamic({
                  id: '',
                  name: '',
                  baseUrl: 'http://host.docker.internal:1234/v1',
                  apiKey: '',
                });
                setTestResult(null);
              }}
            >
              <Save size={14} /> Save Provider
            </Button>
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {(dynamicData?.dynamicProviders ?? []).map((p) => (
          <DynamicProviderCard
            key={p.id}
            provider={p}
            status={statusesData?.statuses.find((s) => s.id === p.id)}
            onRemove={(id) => removeDynamic.mutate(id)}
          />
        ))}
        {!isLoadingDynamic &&
          (dynamicData?.dynamicProviders ?? []).length === 0 &&
          !showAddDynamic && (
            <div className="col-span-full sera-card-static border-dashed border-sera-border p-10 text-center">
              <div className="w-12 h-12 rounded-full bg-sera-surface-hover flex items-center justify-center mx-auto mb-4">
                <Radio size={20} className="text-sera-text-dim" />
              </div>
              <h3 className="text-sm font-medium text-sera-text mb-1">No dynamic providers</h3>
              <p className="text-xs text-sera-text-muted mb-4">
                Add an LM Studio or Ollama instance to discover models automatically.
              </p>
              <Button
                size="sm"
                variant="outline"
                className="text-xs gap-1.5"
                onClick={() => setShowAddDynamic(true)}
              >
                <Plus size={14} /> Configure Now
              </Button>
            </div>
          )}
      </div>
    </section>
  );
}
