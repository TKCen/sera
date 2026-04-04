import { useState } from 'react';
import { CheckCircle, XCircle, RefreshCw, Zap, Save } from 'lucide-react';
import { Button } from '@/components/ui/button';
import * as providersApi from '@/lib/api/providers';

interface DynamicProvider {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
}

interface TestResult {
  success: boolean;
  models: string[];
  error?: string;
}

interface AddDynamicProviderFormProps {
  onClose: () => void;
  onSuccess: () => void;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  addDynamic: any;
}

export function AddDynamicProviderForm({
  onClose,
  onSuccess,
  addDynamic,
}: AddDynamicProviderFormProps) {
  const [newDynamic, setNewDynamic] = useState<DynamicProvider>({
    id: '',
    name: '',
    baseUrl: 'http://host.docker.internal:1234/v1',
    apiKey: '',
  });
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [isTesting, setIsTesting] = useState(false);

  const handleTest = async () => {
    setIsTesting(true);
    setTestResult(null);
    try {
      const res = await providersApi.testDynamicConnection(newDynamic.baseUrl, newDynamic.apiKey);
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
  };

  const handleSave = () => {
    addDynamic.mutate({
      ...newDynamic,
      type: 'lm-studio',
      enabled: true,
      intervalMs: 60000,
    });
    onClose();
    setNewDynamic({
      id: '',
      name: '',
      baseUrl: 'http://host.docker.internal:1234/v1',
      apiKey: '',
    });
    setTestResult(null);
    onSuccess();
  };

  return (
    <div className="sera-card-static p-5 border-sera-accent/30 bg-sera-accent/5 animate-in zoom-in-95 duration-200">
      <div className="flex justify-between items-start mb-4">
        <h3 className="text-sm font-semibold text-sera-text">Add LM Studio Instance</h3>
        <button onClick={onClose} className="text-sera-text-dim hover:text-sera-text" type="button">
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
            Running in Docker? Use <code className="font-mono">host.docker.internal</code> instead
            of <code className="font-mono">localhost</code>
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
            {testResult.success ? <CheckCircle size={14} /> : <XCircle size={14} />}
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
          onClick={handleTest}
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
          onClick={handleSave}
        >
          <Save size={14} /> Save Provider
        </Button>
      </div>
    </div>
  );
}
