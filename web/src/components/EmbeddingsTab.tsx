import { useEffect, useState } from 'react';
import {
  Zap,
  CheckCircle,
  XCircle,
  Save,
  Search,
  AlertTriangle,
} from 'lucide-react';
import {
  useEmbeddingConfig,
  useEmbeddingStatus,
  useUpdateEmbeddingConfig,
  useTestEmbeddingConfig,
  useEmbeddingModels,
  EMBEDDING_PROVIDERS,
  type EmbeddingProvider,
} from '@/hooks/useEmbedding';
import { Spinner } from '@/components/ui/spinner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';

export function EmbeddingsTab() {
  const {
    data: config,
    isLoading: configLoading,
    isError: configError,
    refetch: refetchConfig,
  } = useEmbeddingConfig();
  const { data: status } = useEmbeddingStatus();
  const updateConfig = useUpdateEmbeddingConfig();
  const testConfig = useTestEmbeddingConfig();

  const [provider, setProvider] = useState<EmbeddingProvider>('ollama');
  const [model, setModel] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [dimension, setDimension] = useState(768);
  const [initialized, setInitialized] = useState(false);
  const [testResult, setTestResult] = useState<{
    ok: boolean;
    latencyMs: number;
    dimension?: number;
    error?: string;
  } | null>(null);
  const [saveResult, setSaveResult] = useState<{
    dimensionChanged?: boolean;
    warning?: string;
  } | null>(null);

  const { data: modelsData } = useEmbeddingModels(provider, baseUrl);

  // Initialize form from loaded config
  useEffect(() => {
    if (config && !initialized) {
      setProvider(config.provider);
      setModel(config.model);
      setBaseUrl(config.baseUrl);
      setDimension(config.dimension);
      setInitialized(true);
    }
  }, [config, initialized]);

  const handleTest = async () => {
    setTestResult(null);
    const result = await testConfig.mutateAsync({
      provider,
      model,
      baseUrl,
      dimension,
      ...(apiKey ? { apiKey } : {}),
    });
    setTestResult(result);
  };

  const handleSave = async () => {
    setSaveResult(null);
    const result = await updateConfig.mutateAsync({
      provider,
      model,
      baseUrl,
      dimension,
      ...(apiKey ? { apiKey } : {}),
    });
    setSaveResult(result);
    setTestResult(result.testResult);
  };

  if (configLoading) return <Spinner />;

  if (configError) {
    return (
      <div className="flex flex-col items-center justify-center p-8 border border-sera-border rounded-xl bg-sera-surface">
        <p className="text-sera-error mb-4">Failed to load embedding settings.</p>
        <Button onClick={() => void refetchConfig()} variant="outline">
          Retry
        </Button>
      </div>
    );
  }

  const needsApiKey = provider === 'openai' || provider === 'openai-compatible';
  const needsBaseUrl = provider !== 'openai';

  return (
    <div className="space-y-6">
      {/* Status Card */}
      <div className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Embedding Status
        </h3>
        <div className="flex items-center gap-4 text-xs">
          <Badge variant={status?.available ? 'success' : 'default'}>
            {status?.available ? 'Active' : status?.configured ? 'Offline' : 'Not configured'}
          </Badge>
          {status?.model && (
            <span className="text-sera-text-muted">
              {status.provider}/{status.model} ({status.dimension}d)
            </span>
          )}
        </div>
      </div>

      {/* Configuration Form */}
      <div className="sera-card-static p-4 space-y-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider">
          Embedding Configuration
        </h3>

        {/* Provider */}
        <div>
          <label className="block text-xs text-sera-text-muted mb-1">Provider</label>
          <select
            value={provider}
            onChange={(e) => {
              const p = e.target.value as EmbeddingProvider;
              setProvider(p);
              // Auto-fill default baseUrl
              if (p === 'ollama') setBaseUrl('http://host.docker.internal:11434');
              else if (p === 'lm-studio') setBaseUrl('http://host.docker.internal:1234');
              else if (p === 'openai') setBaseUrl('https://api.openai.com');
              else setBaseUrl('');
            }}
            className="sera-input text-xs w-full"
          >
            {EMBEDDING_PROVIDERS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </div>

        {/* Base URL */}
        {needsBaseUrl && (
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">Base URL</label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="http://host.docker.internal:11434"
              className="sera-input text-xs w-full font-mono"
            />
          </div>
        )}

        {/* API Key */}
        {needsApiKey && (
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">API Key</label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={config?.apiKey ? '(configured — enter to change)' : 'sk-...'}
              className="sera-input text-xs w-full font-mono"
            />
          </div>
        )}

        {/* Model */}
        <div>
          <label className="block text-xs text-sera-text-muted mb-1">Model</label>
          <input
            type="text"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="nomic-embed-text"
            className="sera-input text-xs w-full font-mono"
          />
          {/* Discovered models as chips */}
          {modelsData?.models && modelsData.models.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mt-2">
              {modelsData.models.map((m) => (
                <button
                  key={m.id}
                  onClick={() => {
                    setModel(m.id);
                    if (m.dimension) setDimension(m.dimension);
                  }}
                  className={`px-2 py-0.5 rounded text-[11px] border transition-colors ${
                    model === m.id
                      ? 'bg-sera-accent-soft border-sera-accent text-sera-accent'
                      : 'border-sera-border text-sera-text-muted hover:text-sera-text hover:border-sera-text-muted'
                  }`}
                >
                  {m.id} {m.dimension ? `(${m.dimension}d)` : ''}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Dimension */}
        <div>
          <label className="block text-xs text-sera-text-muted mb-1">Vector Dimension</label>
          <input
            type="number"
            value={dimension}
            onChange={(e) => setDimension(parseInt(e.target.value, 10) || 768)}
            className="sera-input text-xs w-32 font-mono"
          />
          <p className="text-[10px] text-sera-text-dim mt-1">
            Auto-filled for known models. Changing dimension requires re-indexing.
          </p>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-3 pt-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => void handleTest()}
            disabled={testConfig.isPending || !model || !baseUrl}
          >
            {testConfig.isPending ? <Spinner /> : <Zap size={13} />}
            Test Connection
          </Button>
          <Button
            size="sm"
            onClick={() => void handleSave()}
            disabled={updateConfig.isPending || !model || !baseUrl}
          >
            {updateConfig.isPending ? <Spinner /> : <Save size={13} />}
            Save
          </Button>
        </div>

        {/* Test Result */}
        {testResult && (
          <div
            className={`p-3 rounded-lg text-xs ${
              testResult.ok
                ? 'bg-sera-success/10 border border-sera-success/20 text-sera-success'
                : 'bg-sera-error/10 border border-sera-error/20 text-sera-error'
            }`}
          >
            {testResult.ok ? (
              <span className="flex items-center gap-2">
                <CheckCircle size={14} />
                Connected — {testResult.latencyMs}ms latency, {testResult.dimension}d vectors
              </span>
            ) : (
              <span className="flex items-center gap-2">
                <XCircle size={14} />
                Failed: {testResult.error}
              </span>
            )}
          </div>
        )}

        {/* Dimension Change Warning */}
        {saveResult?.dimensionChanged && (
          <div className="p-3 rounded-lg text-xs bg-yellow-500/10 border border-yellow-500/20 text-yellow-400 flex items-start gap-2">
            <AlertTriangle size={14} className="mt-0.5 flex-shrink-0" />
            <span>{saveResult.warning}</span>
          </div>
        )}
      </div>
    </div>
  );
}
