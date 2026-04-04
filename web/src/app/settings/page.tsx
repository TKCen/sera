import { useState } from 'react';
import { Radio, Layers, Settings2, Sliders, Activity, Plus, Search } from 'lucide-react';
import {
  useProviders,
  useDynamicProviders,
  useDynamicProviderStatuses,
  useAddDynamicProvider,
  useRemoveDynamicProvider,
} from '@/hooks/useProviders';
import { useCircuitBreakers, useResetCircuitBreaker } from '@/hooks/useHealth';
import { Spinner } from '@/components/ui/spinner';
import { Button } from '@/components/ui/button';
import { CloudProviderSection } from '@/components/CloudProviderSection';
import { DynamicProviderCard } from '@/components/DynamicProviderCard';
import { GeneralTab } from '@/components/GeneralTab';
import { CircuitBreakersTab } from '@/components/CircuitBreakersTab';
import { EmbeddingsTab } from '@/components/EmbeddingsTab';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { ModelConfigRow } from '@/components/ModelConfigRow';
import { AddDynamicProviderForm } from '@/components/AddDynamicProviderForm';

type Tab = 'providers' | 'models' | 'general' | 'circuit-breakers' | 'embeddings';

function SettingsPageContent() {
  const [tab, setTab] = useState<Tab>('providers');
  const [showAddDynamic, setShowAddDynamic] = useState(false);

  const {
    data: providersData,
    isLoading: isLoadingProviders,
    isError: isErrorProviders,
    refetch: refetchProviders,
  } = useProviders();
  const {
    data: dynamicData,
    isLoading: isLoadingDynamic,
    isError: isErrorDynamic,
    refetch: refetchDynamic,
  } = useDynamicProviders();
  const { data: statusesData } = useDynamicProviderStatuses();
  const addDynamic = useAddDynamicProvider();
  const removeDynamic = useRemoveDynamicProvider();
  const { data: circuitBreakers, refetch: refetchCB } = useCircuitBreakers();
  const resetCB = useResetCircuitBreaker();

  const registeredModels = providersData?.providers ?? [];

  const isLoading = isLoadingProviders || isLoadingDynamic;
  const isError = isErrorProviders || isErrorDynamic;

  const handleRefetch = () => {
    void refetchProviders();
    void refetchDynamic();
  };

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'providers', label: 'Providers', icon: <Layers size={14} /> },
    { id: 'models', label: 'Models', icon: <Settings2 size={14} /> },
    { id: 'circuit-breakers', label: 'Circuit Breakers', icon: <Activity size={14} /> },
    { id: 'embeddings', label: 'Embeddings', icon: <Search size={14} /> },
    { id: 'general', label: 'General', icon: <Sliders size={14} /> },
  ];

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Settings</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            Configure providers, models, and system behavior
          </p>
        </div>
      </div>

      <div className="flex items-center gap-1 border-b border-sera-border mb-8">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex items-center gap-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors duration-150 ${
              tab === t.id
                ? 'border-sera-accent text-sera-accent'
                : 'border-transparent text-sera-text-muted hover:text-sera-text'
            }`}
          >
            {t.icon}
            {t.label}
          </button>
        ))}
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center py-20">
          <Spinner />
        </div>
      ) : isError ? (
        <div className="flex flex-col items-center justify-center p-8 border border-sera-border rounded-xl bg-sera-surface mt-4">
          <p className="text-sera-error mb-4">Failed to load provider settings.</p>
          <Button onClick={handleRefetch} variant="outline">
            Retry
          </Button>
        </div>
      ) : (
        <>
          {tab === 'providers' && (
            <div className="space-y-8 animate-in fade-in slide-in-from-bottom-2 duration-300">
              {/* Dynamic Providers Section */}
              <section>
                <div className="flex items-center justify-between mb-4">
                  <div className="flex items-center gap-2">
                    <Radio size={14} className="text-amber-400" />
                    <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
                      Dynamic Discovery
                    </h2>
                    <span className="text-[11px] text-sera-text-dim/60">
                      — LM Studio, Ollama, etc.
                    </span>
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
                  <AddDynamicProviderForm
                    onClose={() => setShowAddDynamic(false)}
                    onSuccess={() => void refetchDynamic()}
                    addDynamic={addDynamic}
                  />
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
                        <h3 className="text-sm font-medium text-sera-text mb-1">
                          No dynamic providers
                        </h3>
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

              {/* Cloud Providers Section */}
              <CloudProviderSection />
            </div>
          )}

          {tab === 'models' && (
            <div className="sera-card-static p-5">
              {registeredModels.length === 0 ? (
                <div className="text-center py-12 text-sera-text-dim text-sm">
                  No models registered. Add a provider in the Providers tab.
                </div>
              ) : (
                <div className="space-y-2">
                  {registeredModels.map((m) => {
                    const ext = m as unknown as Record<string, unknown>;
                    return (
                      <ModelConfigRow
                        key={m.modelName}
                        model={m}
                        authStatus={ext.authStatus as string}
                        contextWindow={ext.contextWindow as number | undefined}
                        maxTokens={ext.maxTokens as number | undefined}
                        contextStrategy={ext.contextStrategy as string | undefined}
                        contextHighWaterMark={ext.contextHighWaterMark as number | undefined}
                        reasoning={ext.reasoning as boolean | undefined}
                        onSaved={() => void refetchProviders()}
                      />
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {tab === 'circuit-breakers' && (
            <CircuitBreakersTab
              breakers={circuitBreakers ?? []}
              onReset={(p) => {
                void resetCB.mutateAsync(p).then(() => {
                  void refetchCB();
                });
              }}
              resetting={resetCB.isPending}
            />
          )}

          {tab === 'embeddings' && <EmbeddingsTab />}
          {tab === 'general' && <GeneralTab registeredModels={registeredModels} />}
        </>
      )}
    </div>
  );
}

export default function SettingsPage() {
  return (
    <ErrorBoundary fallbackMessage="The settings page encountered an error.">
      <SettingsPageContent />
    </ErrorBoundary>
  );
}
